//! Mid-loop web search and page fetch.
//!
//! Pre-turn SearXNG injection still runs when the server selects a lookup.
//! These tools let the model refine a query or read a cited page afterwards.

use async_trait::async_trait;
use reqwest::redirect::Policy;
use serde_json::{Value, json};
use std::time::Duration;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::{WorkspaceScope, truncate};
use crate::config::WebSearchConfig;
use crate::services::searxng::{SearxngClient, format_search_context, sanitize_query};
use crate::utils::url::validate_public_url;

const MAX_FETCH_BYTES: usize = 1_048_576;
const MAX_FETCH_CHARS: usize = 8_000;
const FETCH_TIMEOUT_SECS: u64 = 20;

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    let config = scope.state.config().web_search.clone();
    if !config.enabled || config.query_url.trim().is_empty() {
        return;
    }
    registry.register(std::sync::Arc::new(WebSearchTool { config }));
    registry.register(std::sync::Arc::new(FetchUrlTool));
}

struct WebSearchTool {
    config: WebSearchConfig,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the public web via SearXNG. Use this to refine a query, search after reading \
         other results, or look up something the pre-turn context did not cover. Cite URLs."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Keep it short; do not paste files."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(self.config.timeout_secs.saturating_add(5))
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        let query = match params.get("query").and_then(Value::as_str) {
            Some(query) if !query.trim().is_empty() => sanitize_query(query),
            _ => {
                return Ok(ToolResult::error(
                    "Missing required string argument 'query'",
                ));
            }
        };
        if query.is_empty() {
            return Ok(ToolResult::error(
                "Search query was empty after sanitizing.",
            ));
        }
        let client = match SearxngClient::new(self.config.clone()) {
            Ok(client) => client,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };
        match client.search(&query).await {
            Ok(hits) if hits.is_empty() => {
                Ok(ToolResult::success("No web search results for that query."))
            }
            Ok(hits) => Ok(ToolResult::success(format_search_context(&hits))),
            Err(error) => {
                tracing::warn!(%error, "web_search failed");
                Ok(ToolResult::error(
                    "The web search failed. Try a different query or use the supplied context.",
                ))
            }
        }
    }
}

struct FetchUrlTool;

#[async_trait]
impl Tool for FetchUrlTool {
    fn name(&self) -> &str {
        "fetch_url"
    }

    fn description(&self) -> &str {
        "Fetch a public HTTP(S) page and return cleaned text. Use after web_search to read a \
         cited URL. Does not access private or authenticated services."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Absolute http or https URL to fetch."
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(FETCH_TIMEOUT_SECS + 5)
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        let url = match params.get("url").and_then(Value::as_str) {
            Some(url) if !url.trim().is_empty() => url.trim(),
            _ => return Ok(ToolResult::error("Missing required string argument 'url'")),
        };
        Ok(fetch_public_url(url).await)
    }
}

async fn fetch_public_url(raw: &str) -> ToolResult {
    let url = match validate_public_url(raw) {
        Ok(url) => url,
        Err(error) => return ToolResult::error(error),
    };

    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .redirect(Policy::limited(3))
        .user_agent("zone-server/fetch-url");
    if let Ok(proxy) = std::env::var("TOOL_RUNNER_PROXY_URL")
        && !proxy.trim().is_empty()
        && let Ok(proxy) = reqwest::Proxy::all(proxy)
    {
        builder = builder.proxy(proxy);
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => return ToolResult::error(error.to_string()),
    };

    let response = match client.get(url.clone()).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, url = %url, "fetch_url request failed");
            return ToolResult::error("Could not fetch that URL.");
        }
    };
    if !response.status().is_success() {
        return ToolResult::error(format!("Fetch returned HTTP {}.", response.status()));
    }
    if let Err(error) = validate_public_url(response.url().as_str()) {
        return ToolResult::error(error);
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !content_type.is_empty()
        && !content_type.contains("text/")
        && !content_type.contains("json")
        && !content_type.contains("xml")
        && !content_type.contains("html")
    {
        return ToolResult::error("That URL did not return readable text.");
    }

    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "fetch_url body failed");
            return ToolResult::error("Could not read the page body.");
        }
    };
    if bytes.len() > MAX_FETCH_BYTES {
        return ToolResult::error("The page is larger than 1 MB and was not read.");
    }
    let raw = String::from_utf8_lossy(&bytes);
    let text = if content_type.contains("html") || looks_like_html(&raw) {
        html_to_text(&raw)
    } else {
        collapse_whitespace(&raw)
    };
    if text.is_empty() {
        return ToolResult::success("The page had no readable text.");
    }
    ToolResult::success(format!("{}\n\n{}", url, truncate(&text, MAX_FETCH_CHARS)))
}

fn looks_like_html(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<!DOCTYPE") || trimmed.starts_with("<html") || trimmed.starts_with("<HTML")
}

fn html_to_text(html: &str) -> String {
    let document = scraper::Html::parse_document(html);
    let mut parts = Vec::new();
    if let Some(body) = scraper::Selector::parse("body")
        .ok()
        .and_then(|selector| document.select(&selector).next())
    {
        collect_text(body, &mut parts);
    } else {
        collect_text(document.root_element(), &mut parts);
    }
    collapse_whitespace(&parts.join(" "))
}

fn collect_text(element: scraper::ElementRef<'_>, parts: &mut Vec<String>) {
    if matches!(element.value().name(), "script" | "style" | "noscript") {
        return;
    }
    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        } else if let Some(child) = scraper::ElementRef::wrap(child) {
            collect_text(child, parts);
        }
    }
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_drops_scripts() {
        let text = html_to_text(
            "<html><body><h1>Title</h1><script>alert(1)</script><p>Hello   world</p></body></html>",
        );
        assert_eq!(text, "Title Hello world");
        assert!(!text.contains("alert"));
    }
}
