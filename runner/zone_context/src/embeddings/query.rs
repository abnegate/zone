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
    let nl_terms = expand_nl_terms(query);
    let keyword = if identifiers.is_empty() {
        if nl_terms.is_empty() {
            fts_tokens(query)
        } else {
            nl_terms.join(" OR ")
        }
    } else {
        let ident = expand_keyword_idents(&identifiers);
        let extra: Vec<String> = nl_terms
            .into_iter()
            .filter(|term| {
                !ident
                    .split_whitespace()
                    .any(|part| part.eq_ignore_ascii_case(term))
            })
            .collect();
        if extra.is_empty() {
            ident
        } else {
            format!("{ident} OR {}", extra.join(" OR "))
        }
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
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

fn is_kebab(token: &str) -> bool {
    token.contains('-')
        && !token.starts_with('-')
        && !token.ends_with('-')
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
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
    let mut humps = 0u32;
    for c in token.chars() {
        if c.is_ascii_lowercase() {
            lower = true;
        } else if c.is_ascii_uppercase() && lower {
            humps += 1;
            lower = false;
        }
    }
    // One short hump (`GitHub`, `Ollama`) is a proper noun, not a type name.
    (humps >= 2 || (humps >= 1 && token.len() >= 10))
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

fn expand_keyword_idents(identifiers: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    let mut push = |token: String| {
        if token.len() < 2 || !seen.insert(token.clone()) {
            return;
        }
        terms.push(token);
    };
    for id in identifiers {
        push(id.clone());
        if is_screaming_snake(id) {
            push(id.to_ascii_lowercase());
            for part in id.split('_') {
                if part.len() >= 5 && !is_generic_ident_part(part) {
                    push(part.to_ascii_lowercase());
                }
            }
        }
        if let Some(snake) = request_style_snake(id) {
            push(snake);
        }
    }
    terms.join(" OR ")
}

fn is_screaming_snake(token: &str) -> bool {
    token.contains('_')
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_generic_ident_part(part: &str) -> bool {
    matches!(
        part.to_ascii_lowercase().as_str(),
        "source"
            | "target"
            | "config"
            | "default"
            | "value"
            | "index"
            | "server"
            | "client"
            | "worker"
            | "handle"
            | "result"
            | "error"
            | "request"
            | "state"
            | "embed"
            | "query"
    )
}

fn request_style_snake(token: &str) -> Option<String> {
    const SUFFIXES: &[&str] = &["Request", "Response", "Error", "Params", "Options", "Args"];
    for suffix in SUFFIXES {
        if let Some(prefix) = token.strip_suffix(suffix)
            && let Some(snake) = camel_to_snake(prefix)
            && snake.contains('_')
        {
            return Some(snake);
        }
    }
    None
}

fn camel_to_snake(token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (index, ch) in token.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn fts_tokens(query: &str) -> String {
    query
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .filter(|part| part.len() >= 2)
        .map(|part| part.replace('_', " "))
        .collect::<Vec<_>>()
        .join(" ")
}

/// English FTS ANDs unquoted words, so NL questions miss `verify_signature`
/// unless we OR distinctive tokens and snake-case bridges (`cors` + `origins`
/// → `cors_origins`) that already appear in indexed `symbol:` / Signature
/// lines. Identifier queries stay on `expand_keyword_idents`.
pub fn nl_bridge_terms(query: &str) -> Vec<String> {
    expand_nl_terms(query)
}

/// Distinctive NL tokens used to decide which chunk of a file answers the question.
pub fn nl_content_tokens(query: &str) -> Vec<String> {
    let identifiers: HashSet<String> = extract_identifiers(query)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    let mut seen = HashSet::new();
    let mut tokens = Vec::new();
    for token in tokenize(query) {
        let token = token.to_ascii_lowercase();
        if identifiers.contains(&token)
            || is_nl_stopword(&token)
            || !token.chars().any(|c| c.is_ascii_alphabetic())
            || !keep_nl_pair_token(&token)
            || !seen.insert(token.clone())
        {
            continue;
        }
        tokens.push(token);
        if tokens.len() == 8 {
            break;
        }
    }
    tokens
}

/// Adjacent content words from the original question, including `not`.
/// Used to prefer the chunk that literally answers ("not configured").
pub fn nl_question_phrases(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in tokenize(query) {
        let token = token.to_ascii_lowercase();
        if is_phrase_skip(&token) || !token.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        tokens.push(token);
    }
    let mut seen = HashSet::new();
    let mut phrases = Vec::new();
    for pair in tokens.windows(2) {
        if !keep_nl_pair_token(&pair[0]) && !keep_nl_pair_token(&pair[1]) {
            continue;
        }
        if is_nl_stopword(&pair[0]) && is_nl_stopword(&pair[1]) {
            continue;
        }
        let phrase = format!("{} {}", pair[0], pair[1]);
        if phrase.len() >= 8 && seen.insert(phrase.clone()) {
            phrases.push(phrase);
        }
    }
    phrases
}

fn expand_nl_terms(query: &str) -> Vec<String> {
    let tokens = nl_content_tokens(query);
    let mut stems = Vec::new();
    for token in &tokens {
        if let Some(stem) = question_verb_stem(token)
            && stem != token.as_str()
        {
            stems.push(stem.to_string());
        }
    }
    let mut seen: HashSet<String> = tokens.iter().cloned().collect();
    let mut terms = Vec::new();
    for token in &tokens {
        if keep_nl_singleton(token) {
            terms.push(token.clone());
        }
    }
    for stem in &stems {
        if keep_nl_singleton(stem) && seen.insert(stem.clone()) {
            terms.push(stem.clone());
        }
    }
    let mut push_pair = |left: &str, right: &str| {
        if !allow_pair_side(left) || !allow_pair_side(right) {
            return;
        }
        let snake = format!("{left}_{right}");
        if snake.len() >= 8 && seen.insert(snake.clone()) {
            terms.push(snake);
        }
    };
    for pair in tokens.windows(2) {
        push_pair(&pair[0], &pair[1]);
    }
    if let (Some(first), Some(last)) = (tokens.first(), tokens.last())
        && first != last
    {
        push_pair(first, last);
    }
    for stem in &stems {
        for token in &tokens {
            if stem != token {
                push_pair(stem, token);
                push_pair(token, stem);
            }
        }
    }
    terms
}

fn question_verb_stem(token: &str) -> Option<&str> {
    Some(match token {
        "derived" | "derive" => "derive",
        "authorized" | "authorizing" => "authorize",
        "configured" => "configure",
        "trimmed" | "trim" => "trim",
        _ => return None,
    })
}

fn keep_nl_pair_token(token: &str) -> bool {
    token.len() >= 4
        || matches!(
            token,
            "cors" | "jwt" | "smtp" | "mcp" | "hmac" | "aes" | "gcm" | "sha" | "sql" | "key"
        )
}

fn allow_pair_side(token: &str) -> bool {
    token.len() >= 4 || matches!(token, "key" | "cors" | "aes" | "gcm" | "jwt" | "mcp" | "sql")
}

fn is_phrase_skip(token: &str) -> bool {
    matches!(
        token,
        "how"
            | "does"
            | "do"
            | "did"
            | "the"
            | "a"
            | "an"
            | "is"
            | "are"
            | "was"
            | "were"
            | "when"
            | "what"
            | "where"
            | "why"
            | "who"
            | "which"
            | "for"
            | "from"
            | "with"
            | "that"
            | "this"
    )
}

fn keep_nl_singleton(token: &str) -> bool {
    if matches!(
        token,
        "generated" | "serving" | "prevent" | "unbounded" | "growth" | "often" | "wake" | "stored"
    ) {
        return false;
    }
    token.len() >= 6
        || matches!(
            token,
            "cors" | "jwt" | "smtp" | "mcp" | "hmac" | "aes" | "gcm"
        )
}

fn is_nl_stopword(token: &str) -> bool {
    matches!(
        token,
        "how"
            | "does"
            | "do"
            | "did"
            | "the"
            | "a"
            | "an"
            | "is"
            | "are"
            | "was"
            | "were"
            | "when"
            | "what"
            | "where"
            | "why"
            | "who"
            | "which"
            | "for"
            | "from"
            | "with"
            | "that"
            | "this"
            | "into"
            | "onto"
            | "than"
            | "then"
            | "just"
            | "used"
            | "using"
            | "after"
            | "before"
            | "about"
            | "over"
            | "under"
            | "and"
            | "or"
            | "not"
            | "can"
            | "could"
            | "should"
            | "would"
            | "will"
            | "its"
            | "their"
            | "our"
            | "must"
            | "have"
            | "has"
            | "been"
            | "being"
            | "all"
            | "any"
            | "each"
            | "return"
            | "returned"
            | "returns"
            | "implement"
            | "implemented"
            | "please"
            | "tell"
            | "me"
            | "on"
            | "in"
            | "at"
            | "of"
            | "to"
            | "by"
            | "up"
            | "out"
            | "off"
            | "if"
            | "as"
            | "so"
            | "no"
            | "yes"
            | "server"
            | "client"
    )
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
    model
        .split(':')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase()
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
        assert!(
            rewritten
                .identifiers
                .iter()
                .any(|i| i == "should_skip_blob")
        );
        assert!(
            rewritten
                .identifiers
                .iter()
                .any(|i| i.contains("content/mod.rs"))
        );
        assert!(rewritten.keyword.contains("should_skip_blob"));
        assert!(
            !rewritten.identifiers.iter().any(|i| i == "GitHub"),
            "proper nouns are not type identifiers: {:?}",
            rewritten.identifiers
        );
        assert!(rewritten.keyword.contains("OR") || rewritten.keyword == "should_skip_blob");
    }

    #[test]
    fn extracts_kebab_and_api_path() {
        let rewritten = rewrite_query(
            "What dimension does get_model_dimension return for nomic-embed-text at /api/embeddings?",
        );
        assert!(
            rewritten
                .identifiers
                .iter()
                .any(|i| i == "nomic-embed-text")
        );
        assert!(
            rewritten
                .identifiers
                .iter()
                .any(|i| i == "get_model_dimension")
        );
        assert!(
            rewritten
                .identifiers
                .iter()
                .any(|i| i == "/api/embeddings" || i.contains("api/embeddings"))
        );
    }

    #[test]
    fn extracts_camel_case() {
        let rewritten = rewrite_query("Where is GitHubAdapter constructed?");
        assert!(rewritten.identifiers.iter().any(|i| i == "GitHubAdapter"));
    }

    #[test]
    fn keyword_expands_request_types_and_env_consts() {
        let request = rewrite_query("What fields does CreateKnowledgeRequest require?");
        assert!(request.keyword.contains("CreateKnowledgeRequest"));
        assert!(request.keyword.contains("create_knowledge"));
        assert!(request.identifiers.iter().all(|i| i != "create_knowledge"));

        let env = rewrite_query("What is SOURCE_RESYNC_POLL_SECS used for?");
        assert!(env.keyword.contains("SOURCE_RESYNC_POLL_SECS"));
        assert!(env.keyword.contains("source_resync_poll_secs"));
        assert!(env.keyword.contains("resync"));
        assert!(!env.keyword.split_whitespace().any(|t| t == "source"));
    }

    #[test]
    fn sanitize_keeps_underscores() {
        assert_eq!(
            sanitize_search_query("should_skip_blob"),
            "should_skip_blob"
        );
    }

    #[test]
    fn qwen_queries_get_an_instruction() {
        let text = embed_query_text("qwen3-embedding:0.6b", "should_skip_blob");
        assert!(text.contains("Instruct:"));
        assert!(text.contains("should_skip_blob"));
        assert_eq!(embed_query_text("nomic-embed-text", "hello"), "hello");
    }

    #[test]
    fn nl_query_ors_distinctive_tokens_and_snake_bridges() {
        let webhook = rewrite_query("How does the server verify a GitHub webhook signature?");
        assert!(webhook.identifiers.is_empty());
        assert!(webhook.keyword.contains(" OR "));
        assert!(webhook.keyword.contains("webhook"));
        assert!(webhook.keyword.contains("signature"));
        assert!(
            webhook.keyword.contains("webhook_signature")
                || webhook.keyword.contains("verify_signature")
        );

        let cors = rewrite_query("When does the API allow all CORS origins?");
        assert!(cors.keyword.to_ascii_lowercase().contains("cors"));
        assert!(cors.keyword.contains("cors_origins"));

        let rate = rewrite_query("How does the rate limiter prevent unbounded memory growth?");
        assert!(rate.keyword.contains("rate_limiter"));

        let refresh = rewrite_query("How often does the knowledge refresh worker wake up?");
        assert!(refresh.keyword.contains("knowledge_refresh"));

        let email = rewrite_query("What error is returned when SMTP is not configured?");
        assert!(email.keyword.contains("smtp"));
        assert!(
            !email.keyword.split(" OR ").any(|term| term == "error"),
            "bare 'error' floods error.rs: {}",
            email.keyword
        );
        assert!(
            nl_question_phrases("What error is returned when SMTP is not configured?")
                .iter()
                .any(|phrase| phrase == "not configured")
        );

        let derive = rewrite_query("Where is the AES-256 encryption key derived?");
        assert!(
            derive.keyword.contains("derive_key"),
            "keyword {}",
            derive.keyword
        );

        let ident = rewrite_query("What does should_skip_blob do when a file SHA is unchanged?");
        assert!(ident.keyword.contains("should_skip_blob"));
        assert!(ident.keyword.contains("unchanged"));
        assert!(!ident.keyword.contains("what_does"));
        assert!(
            nl_bridge_terms(&ident.original)
                .iter()
                .any(|term| term == "unchanged")
        );
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
