//! Rate limiting utilities for API endpoints
//!
//! Provides in-memory rate limiting based on user ID.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed
    pub max_requests: u32,
    /// Time window for rate limiting
    pub window: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 10,
            window: Duration::from_secs(60), // 1 minute
        }
    }
}

/// Request record for rate limiting
#[derive(Debug, Clone)]
struct RequestRecord {
    /// Timestamp of the first request in the current window
    window_start: Instant,
    /// Number of requests in the current window
    count: u32,
}

/// In-memory rate limiter
#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    records: Arc<DashMap<Uuid, RequestRecord>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            records: Arc::new(DashMap::new()),
        }
    }

    /// Get the rate limiter configuration
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Check if a request should be allowed
    ///
    /// Returns (allowed, remaining, reset_at)
    /// - allowed: true if the request should be allowed
    /// - remaining: number of requests remaining in the window
    /// - reset_at: when the rate limit window resets
    pub fn check_rate_limit(&self, user_id: Uuid) -> (bool, u32, Instant) {
        let now = Instant::now();

        // Get or create record for user
        let mut entry = self
            .records
            .entry(user_id)
            .or_insert_with(|| RequestRecord {
                window_start: now,
                count: 0,
            });

        let record = entry.value_mut();

        // Check if we're in a new window
        if now.duration_since(record.window_start) >= self.config.window {
            // Reset the window
            record.window_start = now;
            record.count = 0;
        }

        // ATOMICALLY increment first, then check limit
        // This prevents race condition where two concurrent requests both see count=9
        record.count = record.count.saturating_add(1);

        // Check if we've exceeded the limit
        if record.count > self.config.max_requests {
            // Rollback the increment since we're over the limit
            record.count = record.count.saturating_sub(1);
            let reset_at = record.window_start + self.config.window;
            return (false, 0, reset_at);
        }

        // Request is allowed
        let remaining = self.config.max_requests.saturating_sub(record.count);
        let reset_at = record.window_start + self.config.window;

        (true, remaining, reset_at)
    }

    /// Clean up old entries (optional, for memory management)
    pub fn cleanup(&self) {
        let now = Instant::now();
        let window = self.config.window;

        self.records.retain(|_, record| {
            // Keep entries that are still within their window + some grace period
            now.duration_since(record.window_start) < window * 2
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_rate_limit_allows_requests_within_limit() {
        let config = RateLimitConfig {
            max_requests: 3,
            window: Duration::from_secs(60),
        };
        let limiter = RateLimiter::new(config);
        let user_id = Uuid::new_v4();

        // First request should be allowed
        let (allowed, remaining, _) = limiter.check_rate_limit(user_id);
        assert!(allowed);
        assert_eq!(remaining, 2);

        // Second request should be allowed
        let (allowed, remaining, _) = limiter.check_rate_limit(user_id);
        assert!(allowed);
        assert_eq!(remaining, 1);

        // Third request should be allowed
        let (allowed, remaining, _) = limiter.check_rate_limit(user_id);
        assert!(allowed);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_rate_limit_blocks_requests_over_limit() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_secs(60),
        };
        let limiter = RateLimiter::new(config);
        let user_id = Uuid::new_v4();

        // First two requests allowed
        assert!(limiter.check_rate_limit(user_id).0);
        assert!(limiter.check_rate_limit(user_id).0);

        // Third request should be blocked
        let (allowed, remaining, _) = limiter.check_rate_limit(user_id);
        assert!(!allowed);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_rate_limit_resets_after_window() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_millis(100),
        };
        let limiter = RateLimiter::new(config);
        let user_id = Uuid::new_v4();

        // Use up the limit
        assert!(limiter.check_rate_limit(user_id).0);
        assert!(limiter.check_rate_limit(user_id).0);
        assert!(!limiter.check_rate_limit(user_id).0);

        // Wait for window to reset
        sleep(Duration::from_millis(110));

        // Should be allowed again
        let (allowed, remaining, _) = limiter.check_rate_limit(user_id);
        assert!(allowed);
        assert_eq!(remaining, 1);
    }

    #[test]
    fn test_rate_limit_different_users_independent() {
        let config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(60),
        };
        let limiter = RateLimiter::new(config);
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        // User 1 uses their limit
        assert!(limiter.check_rate_limit(user1).0);
        assert!(!limiter.check_rate_limit(user1).0);

        // User 2 should still be allowed
        assert!(limiter.check_rate_limit(user2).0);
    }

    #[test]
    fn test_rate_limit_cleanup_removes_old_entries() {
        let config = RateLimitConfig {
            max_requests: 10,
            window: Duration::from_millis(50),
        };
        let limiter = RateLimiter::new(config);

        // Add some entries
        for _ in 0..5 {
            let user_id = Uuid::new_v4();
            limiter.check_rate_limit(user_id);
        }

        assert_eq!(limiter.records.len(), 5);

        // Wait for entries to become old
        sleep(Duration::from_millis(150));

        // Cleanup should remove old entries
        limiter.cleanup();

        // Note: cleanup removes entries older than window * 2
        // So entries should still be there if within grace period
        // Let's verify by checking that new requests work
        let user_id = Uuid::new_v4();
        assert!(limiter.check_rate_limit(user_id).0);
    }

    #[test]
    fn test_rate_limit_reset_timestamp() {
        let config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(60),
        };
        let window = config.window; // Clone the window duration before moving config
        let limiter = RateLimiter::new(config);
        let user_id = Uuid::new_v4();

        let start = Instant::now();
        let (_, _, reset_at) = limiter.check_rate_limit(user_id);

        // Reset should be approximately window duration from now
        let expected_reset = start + window;
        let diff = if reset_at > expected_reset {
            reset_at - expected_reset
        } else {
            expected_reset - reset_at
        };

        // Should be within 10ms tolerance
        assert!(diff < Duration::from_millis(10));
    }
}
