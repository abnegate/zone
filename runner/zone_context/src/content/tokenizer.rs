//! Token estimation utilities
//!
//! Provides fast token counting for content sizing decisions.

use abnegate_index::chunk_text;
use abnegate_index::estimate_tokens;

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

const BYTES_PER_TOKEN: usize = 4;

const CHARACTERS_PER_TOKEN: usize = 4;

const SPLIT_OVERLAP_DIVISOR: usize = 10;

const DENSITY_SCALE: usize = 10;

const CODE_DENSITY: usize = 9;

const MARKUP_DENSITY: usize = 6;

const JSON_DENSITY: usize = 7;

/// Identifier-heavy code averages about two characters per token, so 3/2
/// leaves 25% headroom.
const EMBED_CHARACTERS_PER_TOKEN_NUMERATOR: usize = 3;

const EMBED_CHARACTERS_PER_TOKEN_DENOMINATOR: usize = 2;

const MINIMUM_EMBED_CHARACTERS: usize = 512;

/// Estimate tokens from byte size (for pre-fetch estimation)
///
/// Used when we know the file size but haven't fetched content yet.
pub fn estimate_tokens_from_bytes(bytes: usize) -> usize {
    bytes.div_ceil(BYTES_PER_TOKEN)
}

/// Content-type aware token estimation
///
/// Adjusts the estimate based on content type characteristics.
pub fn estimate_tokens_for_type(content: &str, content_type: &str) -> usize {
    let base = estimate_tokens(content);
    match content_type {
        t if t.contains("code") || t.ends_with("/rust") || t.ends_with("/python") => {
            base * CODE_DENSITY / DENSITY_SCALE
        }
        t if t.contains("html") || t.contains("xml") => base * MARKUP_DENSITY / DENSITY_SCALE,
        t if t.contains("json") => base * JSON_DENSITY / DENSITY_SCALE,
        _ => base,
    }
}

/// Character budget for one embedding request
pub fn embed_char_budget(model_max_tokens: usize) -> usize {
    let conservative = model_max_tokens
        .saturating_mul(EMBED_CHARACTERS_PER_TOKEN_NUMERATOR)
        .saturating_div(EMBED_CHARACTERS_PER_TOKEN_DENOMINATOR);
    conservative.clamp(MINIMUM_EMBED_CHARACTERS, MAX_EMBED_CHARS)
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
    let max_tokens = max_chars.div_ceil(CHARACTERS_PER_TOKEN).max(1);
    let overlap = (max_tokens / SPLIT_OVERLAP_DIVISOR)
        .max(1)
        .min(max_tokens.saturating_sub(1));
    chunk_text(text, max_tokens, overlap)
        .into_iter()
        .map(|chunk| chunk.text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            html_estimate < plain_estimate,
            "markup should estimate fewer tokens than plain text"
        );
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
}
