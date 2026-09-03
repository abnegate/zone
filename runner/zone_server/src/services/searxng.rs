//! SearXNG client for chat web search
//!
//! Queries go to whatever URL is configured. In Docker that is SearXNG
//! sharing Gluetun's network namespace at `http://gluetun:8080`.

use serde::Deserialize;
use std::time::Duration;

use crate::config::WebSearchConfig;

/// Truncate user messages so a pasted file cannot become the search query.
const MAX_QUERY_CHARS: usize = 500;

const USER_AGENT: &str = "zone-server/web-search";

/// Prefix the console appends when inlining attached files into `content`.
const ATTACHED_FILE_MARKER: &str = "\n\nAttached file:";

/// SearXNG request error. Chat treats these as non-fatal.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("Search request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Search returned HTTP {0}")]
    Status(u16),
}

/// One result row to inject into the model prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngResult>,
}

#[derive(Debug, Deserialize)]
struct SearxngResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

/// HTTP client for one SearXNG instance.
pub struct SearxngClient {
    http: reqwest::Client,
    config: WebSearchConfig,
}

impl SearxngClient {
    pub fn new(config: WebSearchConfig) -> Result<Self, SearchError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self { http, config })
    }

    /// Query SearXNG and return at most `result_count` hits.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>, SearchError> {
        let query = sanitize_query(query);
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let url = build_search_url(&self.config.query_url, &query);
        let response = self.http.get(&url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(SearchError::Status(status.as_u16()));
        }

        let body: SearxngResponse = response.json().await?;
        Ok(body
            .results
            .into_iter()
            .filter_map(|result| {
                let url = result.url.filter(|u| !u.is_empty())?;
                let title = result
                    .title
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| url.clone());
                Some(SearchHit {
                    title,
                    url,
                    snippet: result.content.unwrap_or_default(),
                })
            })
            .take(self.config.result_count)
            .collect())
    }
}

/// Build a prompt block the model can cite.
pub fn format_search_context(hits: &[SearchHit]) -> String {
    let mut text = String::from(
        "Web search results (via SearXNG). Use these for current information and cite the URLs:\n\n",
    );
    for (index, hit) in hits.iter().enumerate() {
        text.push_str(&format!("{}. {}\n   {}\n", index + 1, hit.title, hit.url));
        if !hit.snippet.is_empty() {
            text.push_str(&format!("   {}\n", hit.snippet));
        }
        text.push('\n');
    }
    text
}

/// Drop attached-file blocks and cap length so the query stays a question.
pub fn sanitize_query(content: &str) -> String {
    let without_attachments = content
        .split(ATTACHED_FILE_MARKER)
        .next()
        .unwrap_or(content);
    without_attachments
        .trim()
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Substitute `<query>` / `{query}` in the configured template, or append `q=`.
pub fn build_search_url(template: &str, query: &str) -> String {
    let encoded = urlencoding::encode(query);
    if template.contains("<query>") {
        template.replace("<query>", encoded.as_ref())
    } else if template.contains("{query}") {
        template.replace("{query}", encoded.as_ref())
    } else {
        let separator = if template.contains('?') { '&' } else { '?' };
        format!("{template}{separator}q={encoded}&format=json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_SEARXNG_QUERY_URL;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn build_search_url_replaces_placeholders() {
        assert_eq!(
            build_search_url(DEFAULT_SEARXNG_QUERY_URL, "open source"),
            "http://gluetun:8080/search?q=open%20source&format=json"
        );
        assert_eq!(
            build_search_url("http://gluetun:8080/search?q={query}&format=json", "a&b"),
            "http://gluetun:8080/search?q=a%26b&format=json"
        );
        assert_eq!(
            build_search_url("http://gluetun:8080/search", "hello"),
            "http://gluetun:8080/search?q=hello&format=json"
        );
        assert_eq!(
            build_search_url("http://gluetun:8080/search?lang=en", "hello"),
            "http://gluetun:8080/search?lang=en&q=hello&format=json"
        );
    }

    #[test]
    fn sanitize_query_strips_attachments_and_caps_length() {
        let content = "What is Rust?\n\nAttached file: notes.md\n```md\nsecret\n```";
        assert_eq!(sanitize_query(content), "What is Rust?");
        assert!(sanitize_query("   ").is_empty());
        assert_eq!(sanitize_query(&"x".repeat(600)).len(), MAX_QUERY_CHARS);
    }

    #[test]
    fn format_search_context_lists_hits() {
        let text = format_search_context(&[SearchHit {
            title: "Rust".to_string(),
            url: "https://www.rust-lang.org/".to_string(),
            snippet: "A language.".to_string(),
        }]);
        assert!(text.contains("Web search results (via SearXNG)"));
        assert!(text.contains("1. Rust"));
        assert!(text.contains("https://www.rust-lang.org/"));
        assert!(text.contains("A language."));
    }

    fn test_client(query_url: String, result_count: usize) -> SearxngClient {
        SearxngClient::new(WebSearchConfig {
            enabled: true,
            query_url,
            result_count,
            timeout_secs: 5,
        })
        .expect("client")
    }

    #[tokio::test]
    async fn search_parses_searxng_json_and_respects_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("q", "open source"))
            .and(query_param("format", "json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {"title": "Example", "url": "https://example.com", "content": "A snippet"},
                    {"title": "Other", "url": "https://other.test", "content": ""},
                    {"title": "Skipped", "url": "https://skip.test", "content": "too many"}
                ]
            })))
            .mount(&server)
            .await;

        let client = test_client(format!("{}/search?q=<query>&format=json", server.uri()), 2);
        let hits = client.search("open source").await.expect("search");
        assert_eq!(
            hits,
            vec![
                SearchHit {
                    title: "Example".to_string(),
                    url: "https://example.com".to_string(),
                    snippet: "A snippet".to_string(),
                },
                SearchHit {
                    title: "Other".to_string(),
                    url: "https://other.test".to_string(),
                    snippet: String::new(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn search_skips_results_without_urls() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {"title": "No url", "content": "x"},
                    {"title": "", "url": "https://ok.test", "content": "ok"}
                ]
            })))
            .mount(&server)
            .await;

        let client = test_client(format!("{}/search?q=<query>&format=json", server.uri()), 5);
        let hits = client.search("q").await.expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "https://ok.test");
        assert_eq!(hits[0].url, "https://ok.test");
    }

    #[tokio::test]
    async fn search_returns_status_error_on_http_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let client = test_client(format!("{}/search?q=<query>&format=json", server.uri()), 5);
        let err = client.search("q").await.expect_err("http error");
        assert!(matches!(err, SearchError::Status(429)));
    }

    #[tokio::test]
    async fn search_returns_empty_for_blank_query() {
        let client = test_client(
            "http://127.0.0.1:1/search?q=<query>&format=json".to_string(),
            5,
        );
        assert!(client.search("   ").await.expect("empty").is_empty());
    }
}
