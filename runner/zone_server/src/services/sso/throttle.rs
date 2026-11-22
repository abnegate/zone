use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use abnegate_http::{Decision, RateLimitConfig, RateLimiter};

use crate::auth::client_address::key;

pub const WINDOW: Duration = Duration::from_secs(60);

const UNKNOWN: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Counts single sign-on starts and SAML posts per client address. Requests whose peer is unknown
/// share one bucket, so they are limited together rather than not at all.
#[derive(Debug)]
pub struct Throttle {
    limiter: RateLimiter<IpAddr>,
}

impl Throttle {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            limiter: RateLimiter::new(RateLimitConfig::new(requests_per_minute, WINDOW)),
        }
    }

    pub fn check(&self, address: Option<IpAddr>) -> Decision {
        self.limiter.check_rate_limit(address.map_or(UNKNOWN, key))
    }

    pub fn cleanup(&self) {
        self.limiter.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    fn exhausted(throttle: &Throttle, client: Option<IpAddr>) {
        for _ in 0..2 {
            assert!(throttle.check(client).allowed, "{client:?}");
        }
        assert!(!throttle.check(client).allowed, "{client:?}");
    }

    #[test]
    fn each_address_has_its_own_bucket() {
        let throttle = Throttle::new(2);

        exhausted(&throttle, address("198.51.100.7"));
        assert!(throttle.check(address("198.51.100.8")).allowed);
    }

    #[test]
    fn requests_without_an_address_share_one_bucket() {
        let throttle = Throttle::new(2);

        exhausted(&throttle, None);
        assert!(!throttle.check(None).allowed);
        assert!(throttle.check(address("198.51.100.7")).allowed);
    }

    #[test]
    fn an_ipv6_network_shares_one_bucket() {
        let throttle = Throttle::new(2);

        exhausted(&throttle, address("2001:db8:1:2::1"));
        assert!(!throttle.check(address("2001:db8:1:2:ffff::9")).allowed);
        assert!(throttle.check(address("2001:db8:1:3::1")).allowed);
    }

    #[test]
    fn an_ipv4_mapped_address_shares_the_ipv4_bucket() {
        let throttle = Throttle::new(2);

        exhausted(&throttle, address("198.51.100.7"));
        assert!(!throttle.check(address("::ffff:198.51.100.7")).allowed);
    }

    #[test]
    fn a_refusal_says_when_the_window_resets() {
        let throttle = Throttle::new(1);
        let client = address("198.51.100.7");

        assert!(throttle.check(client).allowed);
        let refused = throttle.check(client);

        assert!(!refused.allowed);
        assert_eq!(refused.remaining, 0);
        assert!(refused.reset_at.is_some());
    }

    #[test]
    fn a_sweep_keeps_the_current_window() {
        let throttle = Throttle::new(1);
        let client = address("198.51.100.7");

        assert!(throttle.check(client).allowed);
        throttle.cleanup();

        assert!(!throttle.check(client).allowed);
    }

    #[test]
    fn the_window_is_a_minute() {
        assert_eq!(WINDOW, std::time::Duration::from_secs(60));
    }
}
