use std::time::{Duration, Instant};

use abnegate_http::{RateLimitConfig, RateLimiter};
use uuid::Uuid;

pub const REQUESTS: u32 = 600;
pub const WINDOW: Duration = Duration::from_secs(60);

pub fn limiter() -> RateLimiter<Uuid> {
    RateLimiter::new(RateLimitConfig::new(REQUESTS, WINDOW))
}

pub fn retry_after(reset_at: Option<Instant>, window: Duration) -> u64 {
    let remaining = reset_at.map_or(window, |reset_at| {
        reset_at.saturating_duration_since(Instant::now())
    });
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    seconds.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_allows_six_hundred_requests_a_minute() {
        let limiter = limiter();
        assert_eq!(limiter.config().limit, 600);
        assert_eq!(limiter.config().window, Duration::from_secs(60));
    }

    #[test]
    fn retry_after_rounds_up_and_is_never_zero() {
        let soon = Instant::now() + Duration::from_millis(1500);
        assert_eq!(retry_after(Some(soon), WINDOW), 2);
        assert_eq!(retry_after(Some(Instant::now()), WINDOW), 1);
        assert_eq!(retry_after(None, WINDOW), 60);
    }
}
