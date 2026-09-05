//! Turn a chat utterance into a keyword query that can actually hit code.

use std::collections::HashSet;

/// A user question plus the identifier-focused query used for FTS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewrittenQuery {
    pub original: String,
    pub keyword: String,
    pub identifiers: Vec<String>,
}

/// Extract symbols, paths, and code-like tokens so keyword search is not
/// English-stemmed into mush (`should_skip_blob` must not become `shouldskipblob`).
pub fn rewrite_query(query: &str) -> RewrittenQuery {
    let identifiers = extract_identifiers(query);
    // Keep identifiers intact. websearch_to_tsquery ANDs unquoted words, and
    // `to_tsvector('english', 'should_skip_blob')` stores one lexeme — splitting
    // on `_` would miss the hit. OR multiple identifiers so a path + symbol
    // query does not require both tokens in the same chunk.
    let keyword = if identifiers.is_empty() {
        fts_tokens(query)
    } else {
        identifiers.join(" OR ")
    };
    RewrittenQuery {
        original: query.to_string(),
        keyword,
        identifiers,
    }
}

/// Characters PostgreSQL `websearch_to_tsquery` can usefully see for code.
pub fn sanitize_search_query(query: &str) -> String {
    const MAX_QUERY_LEN: usize = 500;
    let truncated = if query.len() > MAX_QUERY_LEN {
        query
            .char_indices()
            .take_while(|(i, _)| *i < MAX_QUERY_LEN)
            .last()
            .map(|(i, c)| &query[..i + c.len_utf8()])
            .unwrap_or("")
    } else {
        query
    };

    truncated
        .chars()
        .filter(|c| {
            c.is_alphanumeric()
                || c.is_whitespace()
                || matches!(*c, '-' | '"' | '_' | '.' | '/' | ':')
        })
        .collect()
}

fn extract_identifiers(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut push = |token: String| {
        if token.len() < 2 || !seen.insert(token.clone()) {
            return;
        }
        out.push(token);
    };

    for token in tokenize(query) {
        if is_path(&token)
            || is_rust_path(&token)
            || is_snake(&token)
            || is_kebab(&token)
            || is_camel(&token)
        {
            push(token);
        }
    }
    out
}

fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in query.chars() {
        if c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-') {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_path(token: &str) -> bool {
    token.contains('/')
}

fn is_rust_path(token: &str) -> bool {
    token.contains("::")
}

fn is_snake(token: &str) -> bool {
    token.contains('_')
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

fn is_kebab(token: &str) -> bool {
    token.contains('-')
        && !token.starts_with('-')
        && !token.ends_with('-')
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

fn is_camel(token: &str) -> bool {
    if token.contains('_')
        || token.contains('-')
        || token.contains('/')
        || token.contains(':')
        || token.contains('.')
    {
        return false;
    }
    let mut lower = false;
    let mut upper_after_lower = false;
    for c in token.chars() {
        if c.is_ascii_lowercase() {
            lower = true;
        } else if c.is_ascii_uppercase() && lower {
            upper_after_lower = true;
        }
    }
    upper_after_lower && token.chars().any(|c| c.is_ascii_alphabetic())
}

fn fts_tokens(query: &str) -> String {
    query
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .filter(|part| part.len() >= 2)
        .map(|part| part.replace('_', " "))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Qwen3-Embedding is instruction-aware; document vectors stay raw.
pub fn embed_query_text(model: &str, query: &str) -> String {
    if model_family(model).contains("qwen") {
        format!(
            "Instruct: Given a search query, retrieve relevant code passages that answer it.\nQuery: {query}"
        )
    } else {
        query.to_string()
    }
}

fn model_family(model: &str) -> String {
    model.split(':').next().unwrap_or(model).to_ascii_lowercase()
}

/// Additive RRF-scale boost when a hit literally contains an extracted identifier.
/// Rank-1 RRF is ~0.01; a symbol in the chunk must beat a mediocre semantic neighbor.
pub fn identifier_match_boost(uri: &str, title: &str, text: &str, identifiers: &[String]) -> f32 {
    if identifiers.is_empty() {
        return 0.0;
    }
    let uri_l = uri.to_ascii_lowercase();
    let title_l = title.to_ascii_lowercase();
    let mut boost: f32 = 0.0;
    for id in identifiers {
        if text.contains(id) {
            boost += 0.01;
            continue;
        }
        let id_l = id.to_ascii_lowercase();
        if !id_l.is_empty() && uri_l.contains(&id_l) {
            boost += 0.006;
        } else if !id_l.is_empty() && title_l.contains(&id_l) {
            boost += 0.004;
        }
    }
    boost.min(0.02)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_snake_and_path() {
        let rewritten = rewrite_query(
            "What does should_skip_blob do when a GitHub file SHA is unchanged in content/mod.rs?",
        );
        assert!(rewritten.identifiers.iter().any(|i| i == "should_skip_blob"));
        assert!(rewritten.identifiers.iter().any(|i| i.contains("content/mod.rs")));
        assert!(rewritten.keyword.contains("should_skip_blob"));
        assert!(rewritten.keyword.contains("OR"));
    }

    #[test]
    fn extracts_kebab_and_api_path() {
        let rewritten = rewrite_query(
            "What dimension does get_model_dimension return for nomic-embed-text at /api/embeddings?",
        );
        assert!(rewritten.identifiers.iter().any(|i| i == "nomic-embed-text"));
        assert!(rewritten.identifiers.iter().any(|i| i == "get_model_dimension"));
        assert!(rewritten.identifiers.iter().any(|i| i == "/api/embeddings" || i.contains("api/embeddings")));
    }

    #[test]
    fn extracts_camel_case() {
        let rewritten = rewrite_query("Where is GitHubAdapter constructed?");
        assert!(rewritten.identifiers.iter().any(|i| i == "GitHubAdapter"));
    }

    #[test]
    fn sanitize_keeps_underscores() {
        assert_eq!(sanitize_search_query("should_skip_blob"), "should_skip_blob");
    }

    #[test]
    fn qwen_queries_get_an_instruction() {
        let text = embed_query_text("qwen3-embedding:0.6b", "should_skip_blob");
        assert!(text.contains("Instruct:"));
        assert!(text.contains("should_skip_blob"));
        assert_eq!(embed_query_text("nomic-embed-text", "hello"), "hello");
    }

    #[test]
    fn identifier_boost_prefers_exact_symbol_hits() {
        let ids = vec!["should_skip_blob".to_string()];
        let in_chunk = identifier_match_boost(
            "github://zone/other.rs",
            "other",
            "fn should_skip_blob() {}",
            &ids,
        );
        let semantic_neighbor =
            identifier_match_boost("github://zone/auth.ts", "auth", "login form", &ids);
        assert!(in_chunk > semantic_neighbor);
        assert!(in_chunk >= 0.01);
    }
}
