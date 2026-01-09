//! Integration tests for TextAdapter

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use zone_context::adapters::{NoOpProgress, SourceAdapter, TEXT_INLINE_URI, TextAdapter};
use zone_context::{ContentCategory, FetchConfig, FetchStrategy};
use zone_core::{Source, SourceCategory, SourceType};

fn create_text_source(content: &str, label: Option<&str>) -> Source {
    let mut config = json!({
        "content": content
    });

    if let Some(label) = label {
        config["label"] = json!(label);
    }

    Source {
        id: Uuid::new_v4(),
        name: "Test Text Source".to_string(),
        source_type: SourceType::Text,
        category: SourceCategory::Text,
        config,
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_text_adapter_full_workflow() {
    let adapter = TextAdapter::new();
    let content = "This is a comprehensive test of the TextAdapter implementation.";
    let label = "Integration Test";
    let source = create_text_source(content, Some(label));

    // Step 1: Verify the source
    let verify_result = adapter.verify(&source).await;
    assert!(verify_result.is_ok(), "Verify should succeed");

    // Step 2: Estimate tokens
    let token_estimate = adapter.estimate_tokens(&source).await.unwrap();
    assert!(token_estimate > 0, "Token estimate should be positive");
    assert!(
        token_estimate < 100,
        "Small text should have reasonable token count"
    );

    // Step 3: Fetch full content
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 1, "Should fetch exactly one item");

    let item = &result.items[0];
    assert_eq!(item.source_id, source.id);
    assert_eq!(item.category, ContentCategory::Text);
    assert_eq!(item.uri, TEXT_INLINE_URI);
    assert_eq!(item.title, label);
    assert_eq!(item.content.as_deref(), Some(content));
    assert!(!item.metadata_only);
    assert_eq!(item.token_count, token_estimate);

    // Verify fetch stats
    assert_eq!(result.stats.items_fetched, 1);
    assert_eq!(result.stats.total_tokens, token_estimate);
    assert_eq!(result.stats.metadata_only_count, 0);
}

#[tokio::test]
async fn test_text_adapter_metadata_only_workflow() {
    let adapter = TextAdapter::new();
    let content = "This content should not be fetched in metadata-only mode.";
    let source = create_text_source(content, None);

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 1);

    let item = &result.items[0];
    assert_eq!(item.title, "Text Content"); // Default title
    assert!(item.content.is_none(), "Content should be None");
    assert!(item.metadata_only, "Should be metadata only");
    assert_eq!(
        item.token_count, 0,
        "Token count should be 0 for metadata only"
    );

    // Verify fetch stats
    assert_eq!(result.stats.items_fetched, 1);
    assert_eq!(result.stats.total_tokens, 0);
    assert_eq!(result.stats.metadata_only_count, 1);
}

#[tokio::test]
async fn test_text_adapter_large_content() {
    let adapter = TextAdapter::new();
    // Create a larger text content
    let large_content = "Lorem ipsum dolor sit amet. ".repeat(100); // ~2800 chars
    let source = create_text_source(&large_content, Some("Large Text"));

    // Verify
    assert!(adapter.verify(&source).await.is_ok());

    // Estimate tokens - should be around 700 tokens (2800 / 4)
    let token_estimate = adapter.estimate_tokens(&source).await.unwrap();
    assert!(
        token_estimate > 600 && token_estimate < 800,
        "Token estimate should be around 700, got {}",
        token_estimate
    );

    // Fetch with partial strategy
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(
            &source,
            &config,
            FetchStrategy::Partial { max_tokens: 1000 },
            &progress,
        )
        .await
        .unwrap();

    let item = &result.items[0];
    assert!(item.content.is_some());
    assert_eq!(item.content.as_ref().unwrap().len(), large_content.len());
}

#[tokio::test]
async fn test_text_adapter_validation_errors() {
    let adapter = TextAdapter::new();

    // Test empty content
    let empty_source = create_text_source("", None);
    let verify_result = adapter.verify(&empty_source).await;
    assert!(
        verify_result.is_err(),
        "Empty content should fail verification"
    );

    // Test invalid config (missing content field)
    let mut invalid_source = create_text_source("test", None);
    invalid_source.config = json!({
        "label": "No Content Field"
    });
    let verify_result = adapter.verify(&invalid_source).await;
    assert!(
        verify_result.is_err(),
        "Missing content field should fail verification"
    );
}

#[tokio::test]
async fn test_text_adapter_special_characters() {
    let adapter = TextAdapter::new();
    let content = "Special chars: 你好世界 🚀 \n\t\r \"quotes\" 'single' <html> & symbols!";
    let source = create_text_source(content, Some("Special Characters"));

    assert!(adapter.verify(&source).await.is_ok());

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    let item = &result.items[0];
    assert_eq!(
        item.content.as_deref(),
        Some(content),
        "Special characters should be preserved"
    );
}

#[tokio::test]
async fn test_text_adapter_rate_limiting() {
    let adapter = TextAdapter::new();
    let rate_config = adapter.rate_limit_config();

    // Text adapter should have unlimited rate limiting
    assert!(rate_config.requests_per_second.is_infinite());
    assert_eq!(rate_config.burst_size, u32::MAX);
    assert!(!rate_config.retry_after_429);
}

#[tokio::test]
async fn test_text_adapter_does_not_support_incremental() {
    let adapter = TextAdapter::new();
    assert!(!adapter.supports_incremental());
}

#[tokio::test]
async fn test_text_adapter_fetch_partial_truncates() {
    let adapter = TextAdapter::new();
    // Create content that's definitely larger than 10 tokens
    let large_content = "a".repeat(1000); // ~250 tokens
    let source = create_text_source(&large_content, None);
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
    use zone_context::error::ContextError;

    let adapter = TextAdapter::new();
    // Create content larger than 10MB limit
    let huge_content = "x".repeat(11 * 1024 * 1024);
    let source = create_text_source(&huge_content, None);

    let result = adapter.verify(&source).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(ContextError::ContentTooLarge { .. })));
}
