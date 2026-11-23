use std::time::{Duration, Instant};

use super::limiter::Limiter;

pub const REQUESTS: u32 = 600;
pub const ADDRESS_FAILURES: u32 = 1200;
pub const WINDOW: Duration = Duration::from_secs(60);
pub const RELOAD: Duration = Duration::from_secs(5);

pub fn limiter() -> Limiter {
    Limiter::new(REQUESTS, WINDOW, ADDRESS_FAILURES, RELOAD)
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
    fn an_organization_gets_six_hundred_requests_a_minute_and_an_address_twice_that_in_failures() {
        let limiter = limiter();
        let organization = uuid::Uuid::new_v4();
        assert_eq!(limiter.window(), Duration::from_secs(60));
        for _ in 0..REQUESTS {
            assert!(limiter.check(organization).allowed);
        }
        assert!(!limiter.check(organization).allowed);
        for _ in 0..ADDRESS_FAILURES {
            assert_eq!(limiter.reject(None, "guess"), None);
        }
        assert!(limiter.reject(None, "guess").is_some());
        assert_eq!((REQUESTS, ADDRESS_FAILURES), (600, 1200));
        assert_eq!(RELOAD, Duration::from_secs(5));
    }

    #[test]
    fn retry_after_rounds_up_and_is_never_zero() {
        let soon = Instant::now() + Duration::from_millis(1500);
        assert_eq!(retry_after(Some(soon), WINDOW), 2);
        assert_eq!(retry_after(Some(Instant::now()), WINDOW), 1);
        assert_eq!(retry_after(None, WINDOW), 60);
    }
}
