//! Notion adapter for fetching pages and databases (STUB)
//!
//! This is a stub implementation that will be completed in a future phase.
//! Currently returns "not implemented" errors for all operations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::adapters::{ProgressCallback, RateLimitConfig, SourceAdapter};
use crate::content::{FetchConfig, FetchResult, FetchStrategy};
use crate::error::{ContextError, Result};
use zone_core::Source;

/// Configuration for Notion sources
///
/// This configuration will be used when the adapter is fully implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionConfig {
    /// Notion page ID to fetch (optional)
    #[serde(default)]
    pub page_id: Option<String>,
    /// Notion database ID to fetch (optional)
    #[serde(default)]
    pub database_id: Option<String>,
    /// Notion integration token (required)
    pub token: String,
}

/// Notion source adapter (stub implementation)
///
/// # Future Implementation
///
/// When implemented, this adapter will:
/// - Connect to the Notion API using an integration token
/// - Fetch pages and their content in markdown or plain text format
/// - Fetch database entries and their properties
/// - Support incremental sync using Notion's last_edited_time
/// - Handle pagination for large databases
/// - Extract rich content including nested blocks
///
/// # API Requirements
///
/// - Notion API version: 2022-06-28 or later
/// - Required OAuth scopes: `read` for pages and databases
/// - Base URL: `https://api.notion.com/v1`
/// - Authentication: Bearer token in Authorization header
///
/// # Configuration
///
/// Either `page_id` or `database_id` must be provided, but not both:
/// - `page_id`: Fetches a single page and its child blocks
/// - `database_id`: Fetches all pages in a database
#[derive(Debug, Clone)]
pub struct NotionAdapter;

impl Default for NotionAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl NotionAdapter {
    /// Create a new Notion adapter
    pub fn new() -> Self {
        Self
    }

    /// Parse Notion config from source
    fn parse_config(&self, source: &Source) -> Result<NotionConfig> {
        serde_json::from_value(source.config.clone())
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid Notion config: {}", e)))
    }
}

#[async_trait]
impl SourceAdapter for NotionAdapter {
    fn source_type(&self) -> &str {
        "notion"
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        // Notion API rate limits: 3 requests per second
        RateLimitConfig {
            requests_per_second: 3.0,
            burst_size: 10,
            retry_after_429: true,
            max_retries: 3,
            backoff_base_ms: 1000,
        }
    }

    async fn verify(&self, source: &Source) -> Result<()> {
        let config = self.parse_config(source)?;

        // Validate configuration
        if config.token.is_empty() {
            return Err(ContextError::InvalidSourceConfig(
                "token is required".to_string(),
            ));
        }

        if config.page_id.is_none() && config.database_id.is_none() {
            return Err(ContextError::InvalidSourceConfig(
                "Either page_id or database_id must be provided".to_string(),
            ));
        }

        if config.page_id.is_some() && config.database_id.is_some() {
            return Err(ContextError::InvalidSourceConfig(
                "Cannot provide both page_id and database_id".to_string(),
            ));
        }

        // Stub: Return not implemented error
        Err(ContextError::adapter(
            "notion",
            "Notion adapter is not yet implemented. This is a stub for future development.",
        ))
    }

    async fn estimate_tokens(&self, _source: &Source) -> Result<usize> {
        Err(ContextError::adapter(
            "notion",
            "Notion adapter is not yet implemented. This is a stub for future development.",
        ))
    }

    async fn fetch(
        &self,
        _source: &Source,
        _config: &FetchConfig,
        _strategy: FetchStrategy,
        _progress: &dyn ProgressCallback,
    ) -> Result<FetchResult> {
        Err(ContextError::adapter(
            "notion",
            "Notion adapter is not yet implemented. This is a stub for future development.",
        ))
    }

    fn supports_incremental(&self) -> bool {
        // Will be true when implemented (using last_edited_time)
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::NoOpProgress;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn create_test_source(config: serde_json::Value) -> Source {
        Source {
            id: Uuid::new_v4(),
            name: "Test Notion Source".to_string(),
            source_type: zone_core::SourceType::Notion,
            category: zone_core::SourceCategory::Document,
            config,
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_notion_adapter_source_type() {
        let adapter = NotionAdapter::new();
        assert_eq!(adapter.source_type(), "notion");
    }

    #[test]
    fn test_notion_adapter_rate_limit_config() {
        let adapter = NotionAdapter::new();
        let config = adapter.rate_limit_config();
        assert_eq!(config.requests_per_second, 3.0);
        assert!(config.retry_after_429);
    }

    #[tokio::test]
    async fn test_notion_adapter_verify_missing_token() {
        let adapter = NotionAdapter::new();
        let source = create_test_source(json!({
            "page_id": "test-page-id"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        // Should fail with config error (missing token)
    }

    #[tokio::test]
    async fn test_notion_adapter_verify_missing_ids() {
        let adapter = NotionAdapter::new();
        let source = create_test_source(json!({
            "token": "test-token"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("page_id or database_id"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_notion_adapter_verify_both_ids() {
        let adapter = NotionAdapter::new();
        let source = create_test_source(json!({
            "token": "test-token",
            "page_id": "page-id",
            "database_id": "db-id"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("Cannot provide both"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_notion_adapter_verify_returns_not_implemented() {
        let adapter = NotionAdapter::new();
        let source = create_test_source(json!({
            "token": "test-token",
            "page_id": "test-page-id"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::Adapter { message, .. }) = result {
            assert!(message.contains("not yet implemented"));
        } else {
            panic!("Expected Adapter error");
        }
    }

    #[tokio::test]
    async fn test_notion_adapter_estimate_tokens_not_implemented() {
        let adapter = NotionAdapter::new();
        let source = create_test_source(json!({
            "token": "test-token",
            "page_id": "test-page-id"
        }));

        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::Adapter { .. }) = result {
            // Expected
        } else {
            panic!("Expected Adapter error");
        }
    }

    #[tokio::test]
    async fn test_notion_adapter_fetch_not_implemented() {
        let adapter = NotionAdapter::new();
        let source = create_test_source(json!({
            "token": "test-token",
            "page_id": "test-page-id"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_err());
        if let Err(ContextError::Adapter { .. }) = result {
            // Expected
        } else {
            panic!("Expected Adapter error");
        }
    }

    #[test]
    fn test_notion_adapter_supports_incremental() {
        let adapter = NotionAdapter::new();
        assert!(!adapter.supports_incremental());
    }

    #[test]
    fn test_notion_config_deserialization() {
        // Test with page_id
        let config: NotionConfig = serde_json::from_value(json!({
            "token": "test-token",
            "page_id": "page-123"
        }))
        .unwrap();
        assert_eq!(config.token, "test-token");
        assert_eq!(config.page_id, Some("page-123".to_string()));
        assert!(config.database_id.is_none());

        // Test with database_id
        let config: NotionConfig = serde_json::from_value(json!({
            "token": "test-token",
            "database_id": "db-456"
        }))
        .unwrap();
        assert_eq!(config.token, "test-token");
        assert!(config.page_id.is_none());
        assert_eq!(config.database_id, Some("db-456".to_string()));
    }
}
