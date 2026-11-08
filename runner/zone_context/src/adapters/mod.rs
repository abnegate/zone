//! Source adapters for content gathering
//!
//! This module provides the `SourceAdapter` trait and implementations for
//! fetching content from various source types.

mod credential;
mod filesystem;
mod github;
mod gitlab;
mod notion;
mod registry;
mod status;
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
use std::time::Duration;

use crate::content::{ContentItem, FetchConfig, FetchResult, FetchStrategy};
use crate::error::ContextError;
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

const RETRY_AFTER_SECONDS: u64 = 60;

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

impl RateLimitConfig {
    pub(crate) async fn retried<T, E, Call, Answer>(
        &self,
        limited: fn(&E) -> bool,
        mut call: Call,
    ) -> Result<std::result::Result<T, E>>
    where
        Call: FnMut() -> Answer,
        Answer: Future<Output = std::result::Result<T, E>>,
    {
        let mut retries = 0;
        loop {
            let answer = call().await;
            if !answer.as_ref().is_err_and(limited) {
                return Ok(answer);
            }
            if retries >= self.max_retries {
                return Err(ContextError::RateLimited {
                    retry_after_secs: RETRY_AFTER_SECONDS,
                });
            }
            retries += 1;
            tokio::time::sleep(Duration::from_millis(
                self.backoff_base_ms * 2_u64.pow(retries - 1),
            ))
            .await;
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

    #[tokio::test(start_paused = true)]
    async fn a_rate_limit_is_retried_with_doubling_waits_until_the_retries_run_out() {
        let calls = std::cell::Cell::new(0);
        let started = tokio::time::Instant::now();

        let answer = RateLimitConfig::default()
            .retried(
                |_: &()| true,
                || {
                    calls.set(calls.get() + 1);
                    std::future::ready(Err::<(), ()>(()))
                },
            )
            .await;

        assert!(
            matches!(
                answer,
                Err(ContextError::RateLimited {
                    retry_after_secs: 60
                })
            ),
            "{answer:?}"
        );
        assert_eq!(calls.get(), 4);
        assert_eq!(started.elapsed(), Duration::from_millis(7000));
    }

    #[tokio::test(start_paused = true)]
    async fn an_answer_after_a_rate_limit_is_returned() {
        let calls = std::cell::Cell::new(0);
        let started = tokio::time::Instant::now();

        let answer = RateLimitConfig::default()
            .retried(
                |_: &()| true,
                || {
                    calls.set(calls.get() + 1);
                    std::future::ready(match calls.get() {
                        1 => Err(()),
                        _ => Ok(5),
                    })
                },
            )
            .await;

        assert!(matches!(answer, Ok(Ok(5))), "{answer:?}");
        assert_eq!(calls.get(), 2);
        assert_eq!(started.elapsed(), Duration::from_millis(1000));
    }

    #[tokio::test(start_paused = true)]
    async fn an_error_that_is_not_a_rate_limit_is_returned_unretried() {
        let calls = std::cell::Cell::new(0);
        let started = tokio::time::Instant::now();

        let answer = RateLimitConfig::default()
            .retried(
                |_: &()| false,
                || {
                    calls.set(calls.get() + 1);
                    std::future::ready(Err::<(), ()>(()))
                },
            )
            .await;

        assert!(matches!(answer, Ok(Err(()))), "{answer:?}");
        assert_eq!(calls.get(), 1);
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}
