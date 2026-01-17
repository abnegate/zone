//! Web adapter for fetching and parsing HTML content from URLs
//!
//! Provides URL validation, SSRF protection, and HTML content extraction.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use url::Url;

use crate::adapters::{ProgressCallback, RateLimitConfig, SourceAdapter};
use crate::content::{
    ContentCategory, ContentItem, ContentMetadata, FetchConfig, FetchResult, FetchStrategy,
};
use crate::error::{ContextError, Result};
use zone_core::Source;

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
    /// Set of allowed schemes
    allowed_schemes: HashSet<String>,
    /// Allow private IPs (for testing only)
    allow_private_ips: bool,
}

impl Default for WebAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WebAdapter {
    /// Create a new Web adapter
    pub fn new() -> Self {
        let mut allowed_schemes = HashSet::new();
        allowed_schemes.insert("http".to_string());
        allowed_schemes.insert("https".to_string());

        Self {
            allowed_schemes,
            allow_private_ips: false,
        }
    }

    /// Create a new Web adapter that allows private IPs (for testing)
    #[cfg(test)]
    pub fn new_for_testing() -> Self {
        let mut allowed_schemes = HashSet::new();
        allowed_schemes.insert("http".to_string());
        allowed_schemes.insert("https".to_string());

        Self {
            allowed_schemes,
            allow_private_ips: true,
        }
    }

    /// Parse web config from source
    fn parse_config(&self, source: &Source) -> Result<WebConfig> {
        serde_json::from_value(source.config.clone())
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid Web config: {}", e)))
    }

    /// Validate URL for SSRF protection
    fn validate_url(&self, url_str: &str) -> Result<Url> {
        // Parse URL
        let url = Url::parse(url_str)
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid URL: {}", e)))?;

        // Check scheme
        if !self.allowed_schemes.contains(url.scheme()) {
            return Err(ContextError::InvalidSourceConfig(format!(
                "Unsupported URL scheme: {}. Only HTTP(S) is allowed",
                url.scheme()
            )));
        }

        // Check for credentials in URL (security risk)
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ContextError::InvalidSourceConfig(
                "URLs with embedded credentials are not allowed".to_string(),
            ));
        }

        // Get hostname
        let host = url.host_str().ok_or_else(|| {
            ContextError::InvalidSourceConfig("URL must have a valid host".to_string())
        })?;

        // Block private IP ranges (SSRF protection) unless explicitly allowed
        if !self.allow_private_ips {
            if let Ok(ip) = host.parse::<IpAddr>()
                && Self::is_private_ip(&ip)
            {
                return Err(ContextError::InvalidSourceConfig(
                    "Private IP addresses are not allowed (SSRF protection)".to_string(),
                ));
            }

            // Block localhost and common internal hostnames
            let lowercase_host = host.to_lowercase();
            if lowercase_host == "localhost"
                || lowercase_host.ends_with(".local")
                || lowercase_host.ends_with(".internal")
                || lowercase_host == "metadata.google.internal" // Cloud metadata service
                || lowercase_host == "169.254.169.254"
            // AWS metadata service
            {
                return Err(ContextError::InvalidSourceConfig(
                    "Internal/localhost hostnames are not allowed (SSRF protection)".to_string(),
                ));
            }
        }

        Ok(url)
    }

    /// Check if an IP address is in a private range
    fn is_private_ip(ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(ipv4) => {
                // 10.0.0.0/8
                ipv4.octets()[0] == 10
                    // 172.16.0.0/12
                    || (ipv4.octets()[0] == 172 && (ipv4.octets()[1] & 0xF0) == 16)
                    // 192.168.0.0/16
                    || (ipv4.octets()[0] == 192 && ipv4.octets()[1] == 168)
                    // 127.0.0.0/8 (loopback)
                    || ipv4.octets()[0] == 127
                    // 169.254.0.0/16 (link-local)
                    || (ipv4.octets()[0] == 169 && ipv4.octets()[1] == 254)
                    // 0.0.0.0/8
                    || ipv4.octets()[0] == 0
            }
            IpAddr::V6(ipv6) => {
                // Check for IPv4-mapped IPv6 addresses (::ffff:x.x.x.x)
                if let Some(mapped_v4) = ipv6.to_ipv4_mapped() {
                    return Self::is_private_ip(&IpAddr::V4(mapped_v4));
                }
                // ::1 (loopback)
                ipv6.is_loopback()
                    // :: (unspecified)
                    || ipv6.is_unspecified()
                    // fe80::/10 (link-local)
                    || (ipv6.segments()[0] & 0xffc0) == 0xfe80
                    // fc00::/7 (unique local)
                    || (ipv6.segments()[0] & 0xfe00) == 0xfc00
            }
        }
    }

    /// Validate that a redirect URL is safe (same origin only)
    fn validate_redirect(&self, original_url: &Url, redirect_url: &Url) -> Result<()> {
        // Validate scheme is still allowed
        if !self.allowed_schemes.contains(redirect_url.scheme()) {
            return Err(ContextError::InvalidSourceConfig(format!(
                "Redirect to disallowed scheme: {}",
                redirect_url.scheme()
            )));
        }

        // Only allow redirects to the same origin
        if original_url.origin() != redirect_url.origin() {
            return Err(ContextError::InvalidSourceConfig(
                "Cross-origin redirects are not allowed for security".to_string(),
            ));
        }

        // Re-validate the redirect URL for SSRF
        self.validate_url(redirect_url.as_str())?;

        Ok(())
    }

    /// Fetch HTML content from a URL with manual redirect handling
    async fn fetch_url(&self, url: &Url) -> Result<String> {
        const MAX_REDIRECTS: u32 = 5;
        const MAX_RESPONSE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

        // Build HTTP client with NO auto-redirect (manual handling for security)
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none()) // Disable auto-redirect
            .user_agent("zone-context/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ContextError::Config(format!("Failed to build HTTP client: {}", e)))?;

        let mut current_url = url.clone();
        let mut redirects = 0;

        loop {
            // Validate URL before each request
            self.validate_url(current_url.as_str())?;

            let response =
                client.get(current_url.as_str()).send().await.map_err(|e| {
                    ContextError::adapter("web", format!("Failed to fetch URL: {}", e))
                })?;

            // Handle redirects manually
            if response.status().is_redirection() {
                if redirects >= MAX_REDIRECTS {
                    return Err(ContextError::TooManyRedirects);
                }

                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(ContextError::InvalidRedirect)?;

                let redirect_url = current_url.join(location).map_err(|e| {
                    ContextError::InvalidSourceConfig(format!("Invalid redirect URL: {}", e))
                })?;

                self.validate_redirect(&current_url, &redirect_url)?;
                current_url = redirect_url;
                redirects += 1;
                continue;
            }

            // Handle error status codes
            if !response.status().is_success() {
                return Err(ContextError::adapter(
                    "web",
                    format!("HTTP error: {}", response.status()),
                ));
            }

            // Check content length before downloading
            let content_length = response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());

            if let Some(len) = content_length
                && len > MAX_RESPONSE_SIZE
            {
                return Err(ContextError::ContentTooLarge {
                    size_bytes: len as usize,
                    max_bytes: MAX_RESPONSE_SIZE as usize,
                });
            }

            // Get content type
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            // Verify it's HTML (or text/plain which might be HTML without proper content-type)
            // In tests, we'll be more lenient
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

            return response.text().await.map_err(|e| {
                ContextError::adapter("web", format!("Failed to read response: {}", e))
            });
        }
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
    fn test_is_private_ip() {
        // Private IPv4 ranges
        assert!(WebAdapter::is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"192.168.1.1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"169.254.1.1".parse().unwrap()));

        // Public IPv4
        assert!(!WebAdapter::is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!WebAdapter::is_private_ip(&"1.1.1.1".parse().unwrap()));

        // Private IPv6
        assert!(WebAdapter::is_private_ip(&"::1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"fe80::1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"fc00::1".parse().unwrap()));
        assert!(WebAdapter::is_private_ip(&"::".parse().unwrap())); // unspecified

        // IPv4-mapped IPv6 addresses
        assert!(WebAdapter::is_private_ip(
            &"::ffff:127.0.0.1".parse().unwrap()
        )); // loopback
        assert!(WebAdapter::is_private_ip(
            &"::ffff:10.0.0.1".parse().unwrap()
        )); // private
        assert!(WebAdapter::is_private_ip(
            &"::ffff:192.168.1.1".parse().unwrap()
        )); // private
        assert!(WebAdapter::is_private_ip(
            &"::ffff:172.16.0.1".parse().unwrap()
        )); // private
        assert!(WebAdapter::is_private_ip(
            &"::ffff:169.254.169.254".parse().unwrap()
        )); // link-local
        assert!(!WebAdapter::is_private_ip(
            &"::ffff:8.8.8.8".parse().unwrap()
        )); // public

        // Public IPv6
        assert!(!WebAdapter::is_private_ip(
            &"2001:4860:4860::8888".parse().unwrap()
        ));
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

    #[test]
    fn test_validate_redirect_scheme() {
        let adapter = WebAdapter::new();
        let original = Url::parse("https://example.com/path").unwrap();

        // Same scheme should work
        let redirect_https = Url::parse("https://example.com/other").unwrap();
        assert!(
            adapter
                .validate_redirect(&original, &redirect_https)
                .is_ok()
        );

        // Different allowed scheme to same origin should fail (cross-origin check)
        let redirect_http = Url::parse("http://example.com/other").unwrap();
        assert!(
            adapter
                .validate_redirect(&original, &redirect_http)
                .is_err()
        );

        // Disallowed scheme should fail
        let redirect_ftp = Url::parse("ftp://example.com/other").unwrap();
        assert!(adapter.validate_redirect(&original, &redirect_ftp).is_err());
    }
}
