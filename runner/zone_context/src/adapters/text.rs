//! Text adapter for inline text content
//!
//! This adapter allows users to provide text content directly without needing
//! an external source. Useful for quick testing and ad-hoc content injection.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::adapters::{ProgressCallback, RateLimitConfig, SourceAdapter, SyncState};
use crate::content::{
    ContentCategory, ContentItem, FetchConfig, FetchResult, FetchStrategy, estimate_tokens,
};
use crate::error::{ContextError, Result};
use zone_core::Source;

/// URI for inline text content
pub const TEXT_INLINE_URI: &str = "text://inline";

/// Maximum allowed text content size (10 MB)
const MAX_TEXT_CONTENT_BYTES: usize = 10 * 1024 * 1024;

/// Configuration for text sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextConfig {
    /// The text content
    pub content: String,
    /// Optional label/title for the content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Text source adapter
///
/// Provides inline text content without needing external sources.
#[derive(Debug, Default)]
pub struct TextAdapter;

impl TextAdapter {
    /// Create a new text adapter
    pub fn new() -> Self {
        Self
    }

    /// Parse text config from source
    fn parse_config(&self, source: &Source) -> Result<TextConfig> {
        serde_json::from_value(source.config.clone())
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid text config: {}", e)))
    }
}

#[async_trait]
impl SourceAdapter for TextAdapter {
    fn source_type(&self) -> &str {
        "text"
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        RateLimitConfig {
            requests_per_second: f64::INFINITY,
            burst_size: u32::MAX,
            retry_after_429: false,
            max_retries: 0,
            backoff_base_ms: 0,
        }
    }

    async fn verify(&self, source: &Source) -> Result<()> {
        let config = self.parse_config(source)?;

        if config.content.is_empty() {
            return Err(ContextError::InvalidSourceConfig(
                "Text content cannot be empty".to_string(),
            ));
        }

        if config.content.len() > MAX_TEXT_CONTENT_BYTES {
            return Err(ContextError::ContentTooLarge {
                size_bytes: config.content.len(),
                max_bytes: MAX_TEXT_CONTENT_BYTES,
            });
        }

        Ok(())
    }

    async fn estimate_tokens(&self, source: &Source) -> Result<usize> {
        let config = self.parse_config(source)?;
        Ok(estimate_tokens(&config.content))
    }

    async fn fetch(
        &self,
        source: &Source,
        _config: &FetchConfig,
        strategy: FetchStrategy,
        progress: &dyn ProgressCallback,
    ) -> Result<FetchResult> {
        let text_config = self.parse_config(source)?;

        let title = text_config
            .label
            .unwrap_or_else(|| "Text Content".to_string());

        let mut result = FetchResult::new(source.id, false);

        let item = match strategy {
            FetchStrategy::MetadataOnly => {
                // Metadata only: don't include content
                ContentItem::new(source.id, ContentCategory::Text, TEXT_INLINE_URI, &title)
            }
            FetchStrategy::Partial { max_tokens } => {
                let estimated = estimate_tokens(&text_config.content);
                if estimated > max_tokens {
                    // Truncate content to fit within budget
                    // Use character-based truncation (approx 4 chars per token)
                    let char_limit = max_tokens * 4;
                    let truncated: String = text_config.content.chars().take(char_limit).collect();
                    ContentItem::new(source.id, ContentCategory::Text, TEXT_INLINE_URI, &title)
                        .with_content(truncated)
                } else {
                    ContentItem::new(source.id, ContentCategory::Text, TEXT_INLINE_URI, &title)
                        .with_content(text_config.content)
                }
            }
            FetchStrategy::Full | FetchStrategy::Progressive { .. } => {
                // Full and Progressive strategies: include full content
                ContentItem::new(source.id, ContentCategory::Text, TEXT_INLINE_URI, &title)
                    .with_content(text_config.content)
            }
        };

        progress.on_item(&item);
        result.add_item(item);
        progress.on_progress(1, Some(1));

        Ok(result)
    }

    fn supports_incremental(&self) -> bool {
        false
    }

    async fn get_sync_state(&self, source: &Source) -> Result<SyncState> {
        Ok(SyncState {
            source_id: source.id,
            ..Default::default()
        })
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
            name: "Test Text Source".to_string(),
            source_type: zone_core::SourceType::Text,
            category: zone_core::SourceCategory::Text,
            config,
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_text_adapter_source_type() {
        let adapter = TextAdapter::new();
        assert_eq!(adapter.source_type(), "text");
    }

    #[tokio::test]
    async fn test_text_adapter_verify_valid() {
        let adapter = TextAdapter::new();
        let source = create_test_source(json!({
            "content": "Hello, world!"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_text_adapter_verify_missing_content() {
        let adapter = TextAdapter::new();
        let source = create_test_source(json!({
            "label": "My Label"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());

        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("Invalid text config"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_text_adapter_verify_empty_content() {
        let adapter = TextAdapter::new();
        let source = create_test_source(json!({
            "content": ""
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());

        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("cannot be empty"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_text_adapter_estimate_tokens() {
        let adapter = TextAdapter::new();
        let content = "This is a test with some content that should be tokenized.";
        let source = create_test_source(json!({
            "content": content
        }));

        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_ok());

        let token_count = result.unwrap();
        // Should use the estimate_tokens function which is ~4 chars per token
        let expected = estimate_tokens(content);
        assert_eq!(token_count, expected);
        assert!(token_count > 0);
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_full() {
        let adapter = TextAdapter::new();
        let content = "This is the full content.";
        let source = create_test_source(json!({
            "content": content
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
        assert_eq!(item.source_id, source.id);
        assert_eq!(item.category, ContentCategory::Text);
        assert_eq!(item.uri, TEXT_INLINE_URI);
        assert_eq!(item.content, Some(content.to_string()));
        assert!(!item.metadata_only);
        assert!(item.token_count > 0);
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_metadata_only() {
        let adapter = TextAdapter::new();
        let content = "This content should not be included.";
        let source = create_test_source(json!({
            "content": content
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
        assert_eq!(item.source_id, source.id);
        assert_eq!(item.category, ContentCategory::Text);
        assert_eq!(item.uri, TEXT_INLINE_URI);
        assert!(item.content.is_none());
        assert!(item.metadata_only);
        assert_eq!(item.token_count, 0);
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_with_label() {
        let adapter = TextAdapter::new();
        let content = "Content with a custom label.";
        let label = "My Custom Label";
        let source = create_test_source(json!({
            "content": content,
            "label": label
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
        assert_eq!(item.title, label);
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_default_title() {
        let adapter = TextAdapter::new();
        let content = "Content without a label.";
        let source = create_test_source(json!({
            "content": content
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
        assert_eq!(item.title, "Text Content");
    }

    #[test]
    fn test_text_adapter_rate_limit_config() {
        let adapter = TextAdapter::new();
        let config = adapter.rate_limit_config();

        // Text adapter should have unlimited rate limiting
        assert!(config.requests_per_second.is_infinite());
        assert_eq!(config.burst_size, u32::MAX);
        assert!(!config.retry_after_429);
    }

    #[test]
    fn test_text_adapter_supports_incremental() {
        let adapter = TextAdapter::new();
        assert!(!adapter.supports_incremental());
    }

    #[tokio::test]
    async fn test_text_adapter_get_sync_state() {
        let adapter = TextAdapter::new();
        let source = create_test_source(json!({
            "content": "test"
        }));

        let result = adapter.get_sync_state(&source).await;
        assert!(result.is_ok());

        let state = result.unwrap();
        assert_eq!(state.source_id, source.id);
        assert!(state.last_sync_at.is_none());
        assert!(state.cursor.is_none());
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_partial_strategy() {
        let adapter = TextAdapter::new();
        let content = "This is content for partial fetch strategy.";
        let source = create_test_source(json!({
            "content": content
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Partial { max_tokens: 100 },
                &progress,
            )
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 1);

        // For Partial strategy, we include content
        let item = &fetch_result.items[0];
        assert!(item.content.is_some());
        assert_eq!(item.content.as_ref().unwrap(), content);
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_progressive_strategy() {
        let adapter = TextAdapter::new();
        let content = "This is content for progressive fetch strategy.";
        let source = create_test_source(json!({
            "content": content
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Progressive {
                    priority_order: vec!["*.rs".to_string()],
                },
                &progress,
            )
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 1);

        // For Progressive strategy, we include content
        let item = &fetch_result.items[0];
        assert!(item.content.is_some());
    }

    #[test]
    fn test_text_config_serialization() {
        let config = TextConfig {
            content: "Hello, world!".to_string(),
            label: Some("Test Label".to_string()),
        };

        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["content"], "Hello, world!");
        assert_eq!(json["label"], "Test Label");

        let deserialized: TextConfig = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.content, config.content);
        assert_eq!(deserialized.label, config.label);
    }

    #[test]
    fn test_text_config_without_label() {
        let config = TextConfig {
            content: "Content only".to_string(),
            label: None,
        };

        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["content"], "Content only");
        // Label should be omitted when None
        assert!(json.get("label").is_none() || json["label"].is_null());
    }

    #[tokio::test]
    async fn test_text_adapter_fetch_partial_truncates() {
        let adapter = TextAdapter::new();
        // Create content that's definitely larger than 10 tokens
        let large_content = "a".repeat(1000); // ~250 tokens
        let source = create_test_source(json!({
            "content": large_content
        }));
        let config = FetchConfig::default();

        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Partial { max_tokens: 10 },
                &NoOpProgress,
            )
            .await
            .unwrap();

        // Content should be truncated to ~40 chars (10 tokens * 4 chars)
        let item = &result.items[0];
        assert!(item.content.is_some());
        let content = item.content.as_ref().unwrap();
        assert!(
            content.len() <= 50,
            "Content should be truncated, got {} chars",
            content.len()
        );
    }

    #[tokio::test]
    async fn test_text_adapter_verify_content_too_large() {
        let adapter = TextAdapter::new();
        // Create content larger than 10MB limit
        let huge_content = "x".repeat(11 * 1024 * 1024);
        let source = create_test_source(json!({
            "content": huge_content
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(ContextError::ContentTooLarge { .. })));
    }
}
