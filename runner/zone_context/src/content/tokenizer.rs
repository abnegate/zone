//! Token estimation utilities
//!
//! Provides fast token counting for content sizing decisions.

use super::code_chunker::{
    ChunkConfig, CodeLanguage, chunk_code_with_config, code_chunk_to_text_chunk,
};

/// Default token budget for `TokenBudget` / Partial adapter walks.
/// This is not a source-index or fetch-size cap.
pub const DEFAULT_TOKEN_BUDGET: usize = 100_000;

/// Maximum chunk size in tokens for embedding
pub const MAX_CHUNK_TOKENS: usize = 512;

/// Hard cap on characters sent to an embedding model.
///
/// Identifier-heavy code is denser than the 4-chars-per-token heuristic, and
/// nomic-embed-text in Ollama often has a 2048-token context. 3000 characters
/// stays inside that window with prefix overhead.
pub const MAX_EMBED_CHARS: usize = 3000;

/// Overlap between chunks in tokens
pub const CHUNK_OVERLAP_TOKENS: usize = 50;

/// Estimate token count from text (fast heuristic)
///
/// Uses a simple character-based estimation that works well for mixed
/// English/code content. More accurate than character count alone.
///
/// # Algorithm
/// - GPT-4 tokenizer averages ~4 chars per token for English text
/// - Code tends to be ~3.5 chars per token due to shorter identifiers
/// - We use 4 chars per token as a conservative estimate
pub fn estimate_tokens(text: &str) -> usize {
    // Simple heuristic: ~4 characters per token
    // This is conservative and works well for mixed content
    text.len().saturating_add(3) / 4
}

/// Estimate tokens from byte size (for pre-fetch estimation)
///
/// Used when we know the file size but haven't fetched content yet.
pub fn estimate_tokens_from_bytes(bytes: usize) -> usize {
    // Assume UTF-8, mostly ASCII content
    bytes.div_ceil(4)
}

/// Content-type aware token estimation
///
/// Adjusts the estimate based on content type characteristics.
pub fn estimate_tokens_for_type(content: &str, content_type: &str) -> usize {
    let base = estimate_tokens(content);

    // Adjust based on content type
    match content_type {
        t if t.contains("code") || t.ends_with("/rust") || t.ends_with("/python") => {
            // Code is slightly denser
            base * 9 / 10
        }
        t if t.contains("html") || t.contains("xml") => {
            // HTML/XML has lots of tags
            base * 6 / 10
        }
        t if t.contains("json") => {
            // JSON has structural overhead
            base * 7 / 10
        }
        t if t.contains("markdown") => {
            // Markdown is close to plain text
            base
        }
        _ => base,
    }
}

/// Split text into chunks suitable for embedding
///
/// Creates overlapping chunks to preserve context across chunk boundaries.
pub fn chunk_text(text: &str, max_tokens: usize, overlap_tokens: usize) -> Vec<TextChunk> {
    // Validate inputs
    if max_tokens == 0 || overlap_tokens >= max_tokens {
        return vec![TextChunk {
            index: 0,
            text: text.to_string(),
            start_offset: 0,
            end_offset: text.len(),
        }];
    }

    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    if total_chars == 0 {
        return chunks;
    }

    // Estimate chars per token
    let chars_per_token = 4;
    let max_chars = max_tokens * chars_per_token;
    let overlap_chars = overlap_tokens * chars_per_token;

    let mut start = 0;
    let mut chunk_index = 0;

    while start < total_chars {
        let mut end = (start + max_chars).min(total_chars);

        // Try to break at a natural boundary (newline, sentence end, word boundary)
        if end < total_chars {
            end = find_break_point(&chars, start, end);
        }

        let chunk_text: String = chars[start..end].iter().collect();
        let byte_start = text.char_indices().nth(start).map(|(i, _)| i).unwrap_or(0);
        let byte_end = if end >= total_chars {
            text.len()
        } else {
            text.char_indices()
                .nth(end)
                .map(|(i, _)| i)
                .unwrap_or(text.len())
        };

        chunks.push(TextChunk {
            index: chunk_index,
            text: chunk_text,
            start_offset: byte_start,
            end_offset: byte_end,
        });

        chunk_index += 1;

        // Move start, accounting for overlap
        let advance = if end - start > overlap_chars {
            end - start - overlap_chars
        } else {
            end - start
        };

        start += advance;

        // Prevent infinite loop
        if advance == 0 {
            break;
        }
    }

    chunks
}

/// A text chunk with position information
#[derive(Debug, Clone)]
pub struct TextChunk {
    /// Index of this chunk (0-based)
    pub index: usize,
    /// The chunk text
    pub text: String,
    /// Start byte offset in original text
    pub start_offset: usize,
    /// End byte offset in original text
    pub end_offset: usize,
}

impl TextChunk {
    /// Estimate token count for this chunk
    pub fn token_count(&self) -> usize {
        estimate_tokens(&self.text)
    }
}

/// Find a natural break point for chunking
fn find_break_point(chars: &[char], start: usize, max_end: usize) -> usize {
    // Look backwards from max_end for a good break point
    let search_start = if max_end > start + 100 {
        max_end - 100
    } else {
        start
    };

    // Priority: paragraph break > sentence end > line break > word boundary
    for i in (search_start..max_end).rev() {
        if chars[i] == '\n' && i + 1 < chars.len() && chars[i + 1] == '\n' {
            return i + 2; // Paragraph break
        }
    }

    for i in (search_start..max_end).rev() {
        if (chars[i] == '.' || chars[i] == '!' || chars[i] == '?')
            && i + 1 < chars.len()
            && chars[i + 1].is_whitespace()
        {
            return i + 1; // Sentence end
        }
    }

    for i in (search_start..max_end).rev() {
        if chars[i] == '\n' {
            return i + 1; // Line break
        }
    }

    for i in (search_start..max_end).rev() {
        if chars[i].is_whitespace() {
            return i + 1; // Word boundary
        }
    }

    max_end
}

/// Character budget for one embedding request
pub fn embed_char_budget(model_max_tokens: usize) -> usize {
    // Assume ~2 chars/token for identifier-heavy code and keep 25% headroom
    let conservative = model_max_tokens.saturating_mul(3).saturating_div(2);
    conservative.clamp(512, MAX_EMBED_CHARS)
}

/// Truncate `text` to `max_chars` on a char boundary
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

/// Split text so each piece fits in `max_chars` (prefers token-aware breaks)
pub fn split_for_embedding(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let max_tokens = max_chars.div_ceil(4).max(1);
    let overlap = (max_tokens / 10).max(1).min(max_tokens.saturating_sub(1));
    chunk_text(text, max_tokens, overlap)
        .into_iter()
        .map(|chunk| chunk.text)
        .collect()
}

/// Smart chunking that uses code-aware chunking for code files
/// and text-based chunking for other content
///
/// This function detects the content type and applies the appropriate chunking strategy:
/// - For code files with recognized extensions, uses tree-sitter based semantic chunking
/// - For other content, falls back to text-based chunking with natural boundaries
pub fn smart_chunk(
    content: &str,
    content_type: &str,
    extension: Option<&str>,
    file_path: Option<&str>,
    max_tokens: usize,
    overlap_tokens: usize,
) -> Vec<TextChunk> {
    let language = if let Some(ext) = extension {
        CodeLanguage::from_extension(ext)
    } else {
        CodeLanguage::from_content_type(content_type)
    };

    match language {
        CodeLanguage::Unknown => chunk_text(content, max_tokens, overlap_tokens),
        _ => {
            let code_chunks = chunk_code_with_config(
                content,
                language,
                &ChunkConfig {
                    max_tokens,
                    file_path: file_path.map(str::to_string),
                    ..ChunkConfig::default()
                },
            );
            code_chunks
                .into_iter()
                .map(code_chunk_to_text_chunk)
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        // "hello" = 5 chars, should be ~1-2 tokens
        let tokens = estimate_tokens("hello");
        assert!((1..=2).contains(&tokens));
    }

    #[test]
    fn test_estimate_tokens_longer() {
        // 100 chars should be ~25 tokens
        let text = "a".repeat(100);
        let tokens = estimate_tokens(&text);
        assert_eq!(tokens, 25); // (100 + 3) / 4 = 25
    }

    #[test]
    fn test_estimate_tokens_from_bytes() {
        assert_eq!(estimate_tokens_from_bytes(100), 25);
        assert_eq!(estimate_tokens_from_bytes(1000), 250);
    }

    #[test]
    fn test_estimate_tokens_for_type_html() {
        let html = "<html><body><p>Hello world</p></body></html>";
        let plain_estimate = estimate_tokens(html);
        let html_estimate = estimate_tokens_for_type(html, "text/html");

        // HTML estimate should be lower due to tag overhead
        assert!(html_estimate < plain_estimate);
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 100, 10);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_small() {
        let text = "Hello world";
        let chunks = chunk_text(text, 100, 10);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
        assert_eq!(chunks[0].start_offset, 0);
        assert_eq!(chunks[0].end_offset, text.len());
    }

    #[test]
    fn test_chunk_text_multiple() {
        // Create text that needs multiple chunks
        let text = "This is sentence one. This is sentence two. This is sentence three. This is sentence four.";
        let chunks = chunk_text(text, 10, 2); // ~40 chars per chunk

        assert!(chunks.len() > 1);

        // First chunk should start at 0
        assert_eq!(chunks[0].start_offset, 0);

        // All text should be covered
        let total_covered: usize = chunks.iter().map(|c| c.text.len()).sum();
        // Due to overlap, total covered will be >= original length
        assert!(total_covered >= text.len());
    }

    #[test]
    fn test_chunk_text_break_at_sentence() {
        let text = "First sentence. Second sentence. Third sentence.";
        let chunks = chunk_text(text, 8, 1); // ~32 chars

        // Should break at sentence boundaries
        for chunk in &chunks {
            // Chunks should end at natural boundaries (period + space or end)
            let text = &chunk.text;
            if !text.is_empty() && chunk.end_offset < 48 {
                // Check it ends cleanly
                assert!(
                    text.ends_with(". ") || text.ends_with('.') || text.ends_with(' '),
                    "Chunk should end at boundary: '{}'",
                    text
                );
            }
        }
    }

    #[test]
    fn test_text_chunk_token_count() {
        let chunk = TextChunk {
            index: 0,
            text: "Hello world this is a test".to_string(),
            start_offset: 0,
            end_offset: 26,
        };

        let tokens = chunk.token_count();
        assert!(tokens > 0);
        assert_eq!(tokens, estimate_tokens(&chunk.text));
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_TOKEN_BUDGET, 100_000);
        assert_eq!(MAX_CHUNK_TOKENS, 512);
        assert_eq!(CHUNK_OVERLAP_TOKENS, 50);
        assert_eq!(MAX_EMBED_CHARS, 3000);
    }

    #[test]
    fn test_truncate_chars_respects_unicode() {
        let text = "héllo🌍world";
        assert_eq!(truncate_chars(text, 0), "");
        assert_eq!(truncate_chars(text, 6), "héllo🌍");
        assert_eq!(truncate_chars(text, 100), text);
    }

    #[test]
    fn test_split_for_embedding_keeps_small_text() {
        let pieces = split_for_embedding("short", 32);
        assert_eq!(pieces, vec!["short".to_string()]);
    }

    #[test]
    fn test_split_for_embedding_covers_large_text() {
        let text = "word ".repeat(2000);
        let pieces = split_for_embedding(&text, 200);
        assert!(pieces.len() > 1);
        assert!(pieces.iter().all(|p| p.chars().count() <= 220));
        let joined: String = pieces.concat();
        assert!(joined.contains("word"));
    }

    #[test]
    fn test_embed_char_budget_clamps() {
        assert_eq!(embed_char_budget(128), 512);
        assert_eq!(embed_char_budget(8192), MAX_EMBED_CHARS);
        assert_eq!(embed_char_budget(1024), 1536);
    }

    #[test]
    fn test_smart_chunk_with_rust_extension() {
        let code = r#"
fn hello() {
    println!("Hello");
}

fn world() {
    println!("World");
}
"#;
        let chunks = smart_chunk(code, "text/plain", Some("rs"), Some("src/lib.rs"), 512, 50);

        // Should use code-aware chunking and produce at least one chunk
        // Note: Small functions may be merged together per Rule 4
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_smart_chunk_with_python_extension() {
        let code = r#"
def hello():
    print("Hello")

def world():
    print("World")
"#;
        let chunks = smart_chunk(code, "text/plain", Some("py"), Some("app.py"), 512, 50);

        // Should use code-aware chunking
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_smart_chunk_with_content_type() {
        let code = r#"
function hello() {
    console.log("Hello");
}
"#;
        let chunks = smart_chunk(code, "text/javascript", None, Some("index.js"), 512, 50);

        // Should detect JavaScript from content type
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_smart_chunk_fallback_to_text() {
        let text = "This is plain text. It should use text-based chunking.";
        let chunks = smart_chunk(text, "text/plain", None, None, 512, 50);

        // Should fall back to text chunking
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
    }

    #[test]
    fn test_smart_chunk_unknown_extension() {
        let text = "Some content with unknown extension.";
        let chunks = smart_chunk(text, "text/plain", Some("xyz"), None, 512, 50);

        // Should fall back to text chunking
        assert_eq!(chunks.len(), 1);
    }
}
