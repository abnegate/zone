//! Web adapter for fetching and parsing HTML content from URLs
//!
//! Fetches through [`abnegate_http::PublicClient`], which refuses internal
//! targets by URL, by resolved address and on every redirect hop.

use abnegate_http::{PublicClient, TargetPolicy, public_client_builder};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::adapters::{ProgressCallback, RateLimitConfig, SourceAdapter};
use crate::content::{
    ContentCategory, ContentItem, ContentMetadata, FetchConfig, FetchResult, FetchStrategy,
};
use crate::error::{ContextError, Result};
use zone_core::Source;

const USER_AGENT: &str = "zone-context/1.0";
const TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

/// Configuration for Web sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// URL to fetch
    pub url: String,
    /// Crawl depth (0 = single page only, default)
    #[serde(default)]
    pub depth: Option<u32>,
    /// URL patterns to include when crawling
    #[serde(default)]
    pub include_patterns: Option<Vec<String>>,
    /// URL patterns to exclude when crawling
    #[serde(default)]
    pub exclude_patterns: Option<Vec<String>>,
    /// CSS selector for content extraction (optional)
    #[serde(default)]
    pub selector: Option<String>,
}

/// Web source adapter
#[derive(Debug, Clone)]
pub struct WebAdapter {
    policy: TargetPolicy,
}

impl Default for WebAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WebAdapter {
    /// Create a Web adapter that refuses every target inside the deployment
    pub fn new() -> Self {
        Self {
            policy: TargetPolicy::Public,
        }
    }

    /// Create a Web adapter that reaches loopback, for a local mock server
    #[cfg(test)]
    pub fn new_for_testing() -> Self {
        Self {
            policy: TargetPolicy::ExceptMetadata,
        }
    }

    /// Parse web config from source
    fn parse_config(&self, source: &Source) -> Result<WebConfig> {
        serde_json::from_value(source.config.clone())
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid Web config: {}", e)))
    }

    fn validate_url(&self, url: &str) -> Result<Url> {
        self.policy
            .validate(url)
            .map_err(|error| ContextError::InvalidSourceConfig(error.to_string()))
    }

    fn client(&self) -> Result<PublicClient> {
        public_client_builder(TIMEOUT)
            .with_target_policy(self.policy)
            .configure(|builder| {
                builder
                    .user_agent(USER_AGENT)
                    .connect_timeout(CONNECT_TIMEOUT)
            })
            .build()
            .map_err(|error| ContextError::Config(format!("Failed to build HTTP client: {error}")))
    }

    /// Fetch HTML content from a URL, following only same-origin redirects
    async fn fetch_url(&self, url: &Url) -> Result<String> {
        if self.policy == TargetPolicy::Public && !zone_core::vpn::allows_public() {
            return Err(ContextError::adapter(
                "web",
                zone_core::vpn::OFFLINE.to_string(),
            ));
        }

        let response = self
            .client()?
            .get(url.as_str())
            .map_err(|error| ContextError::InvalidSourceConfig(error.to_string()))?
            .send()
            .await
            .map_err(fetch_error)?;

        if response.url().origin() != url.origin() {
            return Err(ContextError::InvalidSourceConfig(
                "Cross-origin redirects are not allowed for security".to_string(),
            ));
        }

        if !response.status().is_success() {
            return Err(ContextError::adapter(
                "web",
                format!("HTTP error: {}", response.status()),
            ));
        }

        if let Some(length) = response.content_length()
            && length > MAXIMUM_RESPONSE_BYTES
        {
            return Err(ContextError::ContentTooLarge {
                size_bytes: length as usize,
                max_bytes: MAXIMUM_RESPONSE_BYTES as usize,
            });
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.is_empty()
            && !content_type.contains("text/html")
            && !content_type.contains("text/plain")
            && !content_type.contains("application/xhtml")
        {
            return Err(ContextError::adapter(
                "web",
                format!("URL does not return HTML content (got: {})", content_type),
            ));
        }

        response
            .text()
            .await
            .map_err(|e| ContextError::adapter("web", format!("Failed to read response: {}", e)))
    }

    /// Extract text content from HTML
    fn extract_text(&self, html: &str, selector: Option<&str>) -> Result<String> {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);

        let text = if let Some(selector_str) = selector {
            // Use custom selector
            let selector = Selector::parse(selector_str)
                .map_err(|e| ContextError::Parse(format!("Invalid CSS selector: {:?}", e)))?;

            document
                .select(&selector)
                .map(|el| el.text().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            // Extract all text from body
            if let Ok(body_selector) = Selector::parse("body") {
                document
                    .select(&body_selector)
                    .map(|el| el.text().collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            } else {
                // Fallback: extract all text
                document.root_element().text().collect::<Vec<_>>().join(" ")
            }
        };

        // Clean up whitespace
        let cleaned = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(cleaned)
    }

    /// Extract title from HTML
    fn extract_title(&self, html: &str) -> String {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);

        if let Ok(title_selector) = Selector::parse("title")
            && let Some(title_el) = document.select(&title_selector).next()
        {
            return title_el
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
        }

        "Untitled".to_string()
    }
}

#[async_trait]
impl SourceAdapter for WebAdapter {
    fn source_type(&self) -> &str {
        "web"
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        // Conservative rate limiting for politeness
        RateLimitConfig {
            requests_per_second: 2.0,
            burst_size: 5,
            retry_after_429: true,
            max_retries: 3,
            backoff_base_ms: 1000,
        }
    }

    async fn verify(&self, source: &Source) -> Result<()> {
        let config = self.parse_config(source)?;

        // Validate URL
        self.validate_url(&config.url)?;

        // Validate selector if provided
        if let Some(ref selector) = config.selector {
            use scraper::Selector;
            Selector::parse(selector).map_err(|e| {
                ContextError::InvalidSourceConfig(format!("Invalid CSS selector: {:?}", e))
            })?;
        }

        // Note: We don't actually fetch the URL during verify to avoid unnecessary requests
        // The URL validation above is sufficient for verification

        Ok(())
    }

    async fn estimate_tokens(&self, source: &Source) -> Result<usize> {
        let config = self.parse_config(source)?;

        // For single-page fetch (depth 0 or 1), estimate based on average page size
        let depth = config.depth.unwrap_or(0);

        if depth == 0 {
            // Single page: estimate ~5000 tokens (conservative)
            Ok(5000)
        } else {
            // Multiple pages: estimate based on depth
            // Conservative estimate: 5000 tokens per page, exponential growth with depth
            let pages_estimate = (1..=depth).map(|d| 10_usize.pow(d)).sum::<usize>();
            Ok(pages_estimate * 5000)
        }
    }

    async fn fetch(
        &self,
        source: &Source,
        _fetch_config: &FetchConfig,
        strategy: FetchStrategy,
        progress: &dyn ProgressCallback,
    ) -> Result<FetchResult> {
        let config = self.parse_config(source)?;

        // Validate URL
        let url = self.validate_url(&config.url)?;

        // For now, only support single-page fetch (depth 0)
        // Multi-page crawling can be added later
        let depth = config.depth.unwrap_or(0);
        if depth > 0 {
            return Err(ContextError::adapter(
                "web",
                "Multi-page crawling is not yet implemented. Use depth=0 for single page fetch.",
            ));
        }

        let mut result = FetchResult::new(source.id, false);

        match strategy {
            FetchStrategy::Full
            | FetchStrategy::Partial { .. }
            | FetchStrategy::Progressive { .. } => {
                progress.on_message(&format!("Fetching content from {}", url));

                // Fetch HTML
                let html = self.fetch_url(&url).await?;

                // Extract title
                let title = self.extract_title(&html);

                // Extract text content
                let text = self.extract_text(&html, config.selector.as_deref())?;

                // Create content item
                let mut item =
                    ContentItem::new(source.id, ContentCategory::Document, url.to_string(), title)
                        .with_content_type("text/html".to_string())
                        .with_content(text);

                // Add metadata
                let metadata = ContentMetadata {
                    size_bytes: Some(html.len()),
                    url: Some(url.to_string()),
                    ..Default::default()
                };
                item = item.with_metadata(metadata);

                progress.on_item(&item);
                result.add_item(item);
                progress.on_progress(1, Some(1));
            }
            FetchStrategy::MetadataOnly => {
                progress.on_message(&format!("Fetching metadata from {}", url));

                // For metadata only, we still need to fetch to get the title
                // But we don't extract the full text
                let html = self.fetch_url(&url).await?;
                let title = self.extract_title(&html);

                let mut item =
                    ContentItem::new(source.id, ContentCategory::Document, url.to_string(), title)
                        .with_content_type("text/html".to_string());

                let metadata = ContentMetadata {
                    size_bytes: Some(html.len()),
                    url: Some(url.to_string()),
                    ..Default::default()
                };
                item = item.with_metadata(metadata);

                // Mark as metadata only (no content)
                item.metadata_only = true;
                item.token_count = 0;

                progress.on_item(&item);
                result.add_item(item);
                progress.on_progress(1, Some(1));
            }
        }

        Ok(result)
    }

    fn supports_incremental(&self) -> bool {
        false // Web pages don't have a good incremental sync mechanism
    }
}

fn fetch_error(error: abnegate_http::Error) -> ContextError {
    if redirected_too_often(&error) {
        return ContextError::TooManyRedirects;
    }
    ContextError::adapter("web", format!("Failed to fetch URL: {error}"))
}

fn redirected_too_often(error: &abnegate_http::Error) -> bool {
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if matches!(
            cause.downcast_ref::<abnegate_http::Error>(),
            Some(abnegate_http::Error::TooManyRedirects)
        ) {
            return true;
        }
        source = cause.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::NoOpProgress;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_source(config: serde_json::Value) -> Source {
        Source {
            id: Uuid::new_v4(),
            name: "Test Web Source".to_string(),
            source_type: zone_core::SourceType::Web,
            category: zone_core::SourceCategory::Document,
            config,
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_web_adapter_source_type() {
        let adapter = WebAdapter::new();
        assert_eq!(adapter.source_type(), "web");
    }

    #[test]
    fn test_validate_url_valid() {
        let adapter = WebAdapter::new();
        assert!(adapter.validate_url("https://example.com").is_ok());
        assert!(adapter.validate_url("http://example.com").is_ok());
        assert!(
            adapter
                .validate_url("https://example.com/path?query=value")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_url_invalid_scheme() {
        let adapter = WebAdapter::new();
        assert!(adapter.validate_url("ftp://example.com").is_err());
        assert!(adapter.validate_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_validate_url_with_credentials() {
        let adapter = WebAdapter::new();
        assert!(
            adapter
                .validate_url("https://user:pass@example.com")
                .is_err()
        );
    }

    #[test]
    fn test_validate_url_private_ips() {
        let adapter = WebAdapter::new();
        assert!(adapter.validate_url("http://127.0.0.1").is_err());
        assert!(adapter.validate_url("http://10.0.0.1").is_err());
        assert!(adapter.validate_url("http://172.16.0.1").is_err());
        assert!(adapter.validate_url("http://192.168.1.1").is_err());
        assert!(adapter.validate_url("http://169.254.169.254").is_err());
    }

    #[test]
    fn test_validate_url_localhost() {
        let adapter = WebAdapter::new();
        assert!(adapter.validate_url("http://localhost").is_err());
        assert!(adapter.validate_url("http://server.local").is_err());
        assert!(
            adapter
                .validate_url("http://metadata.google.internal")
                .is_err()
        );
    }

    #[test]
    fn targets_outside_the_private_ranges_but_inside_the_deployment_are_refused() {
        let adapter = WebAdapter::new();
        for url in [
            "http://100.64.0.1/",
            "http://198.18.0.1/",
            "http://[64:ff9b::a00:1]/",
            "http://[fec0::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://localhost./",
        ] {
            assert!(adapter.validate_url(url).is_err(), "{url} must be refused");
        }
    }

    #[tokio::test]
    async fn a_production_fetch_never_connects_to_loopback() {
        let _vpn = zone_core::vpn::Hold::off();
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
            .expect(0)
            .mount(&mock_server)
            .await;
        let adapter = WebAdapter::new();
        let url = Url::parse(&mock_server.uri()).expect("a URL");

        let error = adapter
            .fetch_url(&url)
            .await
            .expect_err("loopback must not be fetched");

        assert!(
            matches!(error, ContextError::InvalidSourceConfig(_)),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn test_web_adapter_verify() {
        let adapter = WebAdapter::new();
        let source = create_test_source(json!({
            "url": "https://example.com"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_web_adapter_verify_invalid_url() {
        let adapter = WebAdapter::new();
        let source = create_test_source(json!({
            "url": "http://localhost"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_web_adapter_verify_invalid_selector() {
        let adapter = WebAdapter::new();
        let source = create_test_source(json!({
            "url": "https://example.com",
            "selector": "invalid[[[selector"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn a_production_fetch_stays_offline_until_the_vpn_is_on() {
        let _vpn = zone_core::vpn::Hold::required_off();
        let adapter = WebAdapter::new();
        let source = create_test_source(json!({
            "url": "https://example.com"
        }));
        let error = adapter
            .fetch(
                &source,
                &FetchConfig::default(),
                FetchStrategy::Full,
                &NoOpProgress,
            )
            .await
            .expect_err("must refuse");
        assert!(
            error.to_string().contains(zone_core::vpn::OFFLINE),
            "{error}"
        );
    }

    #[tokio::test]
    async fn test_web_adapter_fetch_with_mock() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(
                        r#"
                        <!DOCTYPE html>
                        <html>
                        <head><title>Test Page</title></head>
                        <body>
                            <h1>Hello World</h1>
                            <p>This is a test page.</p>
                        </body>
                        </html>
                        "#,
                    )
                    .insert_header("content-type", "text/html"),
            )
            .mount(&mock_server)
            .await;

        let adapter = WebAdapter::new_for_testing();
        let source = create_test_source(json!({
            "url": mock_server.uri()
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        if let Err(ref e) = result {
            eprintln!("Test failed with error: {:?}", e);
        }
        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 1);

        let item = &fetch_result.items[0];
        assert_eq!(item.title, "Test Page");
        assert!(item.content.is_some());
        assert!(item.content.as_ref().unwrap().contains("Hello World"));
    }

    #[tokio::test]
    async fn test_web_adapter_fetch_with_selector() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(
                        r#"
                        <!DOCTYPE html>
                        <html>
                        <head><title>Test Page</title></head>
                        <body>
                            <nav>Navigation</nav>
                            <article>
                                <h1>Article Title</h1>
                                <p>Article content.</p>
                            </article>
                            <footer>Footer</footer>
                        </body>
                        </html>
                        "#,
                    )
                    .insert_header("content-type", "text/html"),
            )
            .mount(&mock_server)
            .await;

        let adapter = WebAdapter::new_for_testing();
        let source = create_test_source(json!({
            "url": mock_server.uri(),
            "selector": "article"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 1);

        let item = &fetch_result.items[0];
        assert!(item.content.is_some());
        let content = item.content.as_ref().unwrap();
        assert!(content.contains("Article Title"));
        assert!(!content.contains("Navigation"));
        assert!(!content.contains("Footer"));
    }

    #[tokio::test]
    async fn test_web_adapter_fetch_metadata_only() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(
                        r#"
                        <!DOCTYPE html>
                        <html>
                        <head><title>Test Page</title></head>
                        <body><p>Content</p></body>
                        </html>
                        "#,
                    )
                    .insert_header("content-type", "text/html"),
            )
            .mount(&mock_server)
            .await;

        let adapter = WebAdapter::new_for_testing();
        let source = create_test_source(json!({
            "url": mock_server.uri()
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 1);

        let item = &fetch_result.items[0];
        assert_eq!(item.title, "Test Page");
        assert!(item.metadata_only);
        assert_eq!(item.token_count, 0);
    }

    #[tokio::test]
    async fn test_web_adapter_estimate_tokens() {
        let adapter = WebAdapter::new();
        let source = create_test_source(json!({
            "url": "https://example.com"
        }));

        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);
    }

    #[test]
    fn test_web_adapter_supports_incremental() {
        let adapter = WebAdapter::new();
        assert!(!adapter.supports_incremental());
    }

    #[test]
    fn test_extract_title() {
        let adapter = WebAdapter::new();
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head><title>Test Title</title></head>
            <body></body>
            </html>
        "#;

        let title = adapter.extract_title(html);
        assert_eq!(title, "Test Title");
    }

    #[test]
    fn test_extract_title_no_title() {
        let adapter = WebAdapter::new();
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body></body>
            </html>
        "#;

        let title = adapter.extract_title(html);
        assert_eq!(title, "Untitled");
    }

    #[test]
    fn test_extract_text() {
        let adapter = WebAdapter::new();
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1>Heading</h1>
                <p>Paragraph 1</p>
                <p>Paragraph 2</p>
            </body>
            </html>
        "#;

        let text = adapter.extract_text(html, None).unwrap();
        assert!(text.contains("Heading"));
        assert!(text.contains("Paragraph 1"));
        assert!(text.contains("Paragraph 2"));
    }

    #[test]
    fn test_extract_text_with_selector() {
        let adapter = WebAdapter::new();
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <div class="content">Selected content</div>
                <div class="other">Other content</div>
            </body>
            </html>
        "#;

        let text = adapter.extract_text(html, Some(".content")).unwrap();
        assert!(text.contains("Selected content"));
        assert!(!text.contains("Other content"));
    }

    #[tokio::test]
    async fn test_web_adapter_response_size_limit() {
        let mock_server = MockServer::start().await;

        // Mock a response that claims to be 15MB (over the 10MB limit)
        // Set an accurate content-length header
        let large_content = "x".repeat(15 * 1024 * 1024); // 15MB
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(large_content.clone())
                    .insert_header("content-type", "text/html")
                    .insert_header("content-length", large_content.len().to_string()),
            )
            .mount(&mock_server)
            .await;

        let adapter = WebAdapter::new_for_testing();
        let source = create_test_source(json!({
            "url": mock_server.uri()
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(
                matches!(e, ContextError::ContentTooLarge { .. }),
                "Expected ContentTooLarge error, got: {:?}",
                e
            );
        }
    }

    #[tokio::test]
    async fn test_web_adapter_redirect_validation() {
        let mock_server = MockServer::start().await;

        // Mock a redirect response (status 302)
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/final"))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(
                        "<html><head><title>Final</title></head><body>Content</body></html>",
                    )
                    .insert_header("content-type", "text/html"),
            )
            .mount(&mock_server)
            .await;

        let adapter = WebAdapter::new_for_testing();
        let source = create_test_source(json!({
            "url": format!("{}/redirect", mock_server.uri())
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        // Should succeed since redirect is to same origin
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_web_adapter_too_many_redirects() {
        let mock_server = MockServer::start().await;

        // Create a redirect loop
        for i in 0..10 {
            let next = format!("/redirect{}", (i + 1) % 10);
            Mock::given(method("GET"))
                .and(path(format!("/redirect{}", i)))
                .respond_with(ResponseTemplate::new(302).insert_header("location", next))
                .mount(&mock_server)
                .await;
        }

        let adapter = WebAdapter::new_for_testing();
        let source = create_test_source(json!({
            "url": format!("{}/redirect0", mock_server.uri())
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, ContextError::TooManyRedirects));
        }
    }

    #[tokio::test]
    async fn a_cross_origin_redirect_is_refused() {
        let origin = MockServer::start().await;
        let elsewhere = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/final", elsewhere.uri())),
            )
            .mount(&origin)
            .await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("<html><body>Elsewhere</body></html>")
                    .insert_header("content-type", "text/html"),
            )
            .mount(&elsewhere)
            .await;
        let adapter = WebAdapter::new_for_testing();
        let url = Url::parse(&origin.uri()).expect("a URL");

        let error = adapter
            .fetch_url(&url)
            .await
            .expect_err("the redirect leaves the origin");

        assert!(
            matches!(error, ContextError::InvalidSourceConfig(ref message) if message.contains("Cross-origin")),
            "{error:?}"
        );
    }
}
