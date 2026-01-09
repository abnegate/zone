//! Source adapters for content gathering
//!
//! This module provides the `SourceAdapter` trait and implementations for
//! fetching content from various source types.

mod filesystem;
mod github;
mod gitlab;
mod notion;
mod registry;
mod text;
mod web;

pub use filesystem::FilesystemAdapter;
pub use github::GitHubAdapter;
pub use gitlab::GitLabAdapter;
pub use notion::NotionAdapter;
pub use registry::AdapterRegistry;
pub use text::{TEXT_INLINE_URI, TextAdapter};
pub use web::WebAdapter;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::content::{ContentItem, FetchConfig, FetchResult, FetchStrategy};
use crate::error::Result;
use zone_core::Source;

/// Progress callback for streaming updates during fetch
pub trait ProgressCallback: Send + Sync {
    /// Called when an item is fetched
    fn on_item(&self, item: &ContentItem);

    /// Called with progress update (current, total)
    fn on_progress(&self, current: usize, total: Option<usize>);

    /// Called with a status message
    fn on_message(&self, message: &str);
}

/// No-op progress callback
pub struct NoOpProgress;

impl ProgressCallback for NoOpProgress {
    fn on_item(&self, _item: &ContentItem) {}
    fn on_progress(&self, _current: usize, _total: Option<usize>) {}
    fn on_message(&self, _message: &str) {}
}

/// Rate limit configuration for an adapter
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per second limit
    pub requests_per_second: f64,
    /// Maximum burst size
    pub burst_size: u32,
    /// Whether to automatically retry on 429
    pub retry_after_429: bool,
    /// Maximum number of retries
    pub max_retries: u32,
    /// Base delay for exponential backoff (ms)
    pub backoff_base_ms: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10.0,
            burst_size: 20,
            retry_after_429: true,
            max_retries: 3,
            backoff_base_ms: 1000,
        }
    }
}

/// Sync state for incremental fetching
#[derive(Debug, Clone, Default)]
pub struct SyncState {
    /// Source ID
    pub source_id: uuid::Uuid,
    /// Last sync timestamp
    pub last_sync_at: Option<DateTime<Utc>>,
    /// Continuation cursor (source-specific)
    pub cursor: Option<String>,
    /// ETag for HTTP-based sources
    pub etag: Option<String>,
    /// Version string (source-specific)
    pub version: Option<String>,
    /// Additional source-specific state
    pub extra: serde_json::Value,
}

/// The core trait for source adapters
///
/// Implementations should handle:
/// - Connection verification
/// - Token estimation for sizing decisions
/// - Content fetching with progress callbacks
/// - Incremental sync support (optional)
#[async_trait]
pub trait SourceAdapter: Send + Sync {
    /// Get the source type identifier (e.g., "github", "filesystem")
    fn source_type(&self) -> &str;

    /// Get rate limit configuration
    fn rate_limit_config(&self) -> RateLimitConfig {
        RateLimitConfig::default()
    }

    /// Verify the source connection and credentials
    async fn verify(&self, source: &Source) -> Result<()>;

    /// Estimate total tokens for the source
    ///
    /// Used to decide fetch strategy before full fetch.
    async fn estimate_tokens(&self, source: &Source) -> Result<usize>;

    /// Fetch content from the source
    ///
    /// The `strategy` determines how much content to fetch:
    /// - `Full`: Fetch everything
    /// - `MetadataOnly`: Only metadata, no content
    /// - `Partial`: Fetch up to max_tokens
    /// - `Progressive`: Fetch in priority order
    async fn fetch(
        &self,
        source: &Source,
        config: &FetchConfig,
        strategy: FetchStrategy,
        progress: &dyn ProgressCallback,
    ) -> Result<FetchResult>;

    /// Check if incremental sync is supported
    fn supports_incremental(&self) -> bool {
        false
    }

    /// Get current sync state
    async fn get_sync_state(&self, _source: &Source) -> Result<SyncState> {
        Ok(SyncState::default())
    }
}

/// Type alias for adapter references
pub type AdapterRef = Arc<dyn SourceAdapter>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_second, 10.0);
        assert_eq!(config.burst_size, 20);
        assert!(config.retry_after_429);
    }

    #[test]
    fn test_sync_state_default() {
        let state = SyncState::default();
        assert!(state.last_sync_at.is_none());
        assert!(state.cursor.is_none());
    }

    #[test]
    fn test_no_op_progress() {
        let progress = NoOpProgress;
        // These should not panic
        progress.on_message("test");
        progress.on_progress(0, Some(100));
    }
}
