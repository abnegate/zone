use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use abnegate_http::{Decision, RateLimitConfig, RateLimiter};
use dashmap::DashMap;

use crate::auth::client_address::key;

pub const WINDOW: Duration = Duration::from_secs(60);

const UNKNOWN: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Counts single sign-on starts and SAML posts per client address. Requests whose peer is unknown
/// share one bucket, so they are limited together rather than not at all. The count lives in this
/// process, so each server replica limits on its own.
#[derive(Debug)]
pub struct Throttle {
    limiter: RateLimiter<IpAddr>,
    announced: DashMap<IpAddr, Option<Instant>>,
}

impl Throttle {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            limiter: RateLimiter::new(RateLimitConfig::new(requests_per_minute, WINDOW)),
            announced: DashMap::new(),
        }
    }

    pub fn check(&self, address: Option<IpAddr>) -> Decision {
        self.limiter.check_rate_limit(bucket(address))
    }

    /// Whether a refusal is the address's first in its window, so it is worth a warning while
    /// the rest of a flood is not.
    pub fn announces(&self, address: Option<IpAddr>, decision: &Decision) -> bool {
        !decision.allowed && self.first_refusal(address, decision.reset_at)
    }

    fn first_refusal(&self, address: Option<IpAddr>, reset_at: Option<Instant>) -> bool {
        self.announced
            .insert(bucket(address), reset_at)
            .is_none_or(|announced| announced != reset_at)
    }

    pub fn cleanup(&self) {
        self.limiter.cleanup();
        self.sweep(Instant::now());
    }

    fn sweep(&self, now: Instant) {
        self.announced
            .retain(|_, reset_at| reset_at.is_none_or(|reset_at| reset_at > now));
    }
}

fn bucket(address: Option<IpAddr>) -> IpAddr {
    address.map_or(UNKNOWN, key)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

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
    fn only_the_first_refusal_in_a_window_is_announced() {
        let throttle = Throttle::new(1);
        let client = address("198.51.100.7");

        let allowed = throttle.check(client);
        assert!(!throttle.announces(client, &allowed));
        let first = throttle.check(client);
        assert!(throttle.announces(client, &first));
        for _ in 0..3 {
            let repeated = throttle.check(client);
            assert!(!throttle.announces(client, &repeated));
        }
    }

    #[test]
    fn each_address_announces_its_own_first_refusal() {
        let throttle = Throttle::new(1);

        for client in [
            address("198.51.100.7"),
            address("198.51.100.8"),
            address("2001:db8:1:2::1"),
            None,
        ] {
            throttle.check(client);
            let refused = throttle.check(client);
            assert!(throttle.announces(client, &refused), "{client:?}");
        }
        let network = address("2001:db8:1:2::9");
        let refused = throttle.check(network);
        assert!(!throttle.announces(network, &refused));
    }

    #[test]
    fn a_new_window_announces_again() {
        let throttle = Throttle::new(1);
        let client = address("198.51.100.7");
        let now = Instant::now();

        assert!(throttle.first_refusal(client, Some(now + WINDOW)));
        assert!(!throttle.first_refusal(client, Some(now + WINDOW)));
        assert!(throttle.first_refusal(client, Some(now + WINDOW + WINDOW)));
    }

    #[test]
    fn a_sweep_forgets_announcements_of_past_windows_only() {
        let throttle = Throttle::new(1);
        let current = address("198.51.100.7");
        let past = address("198.51.100.8");
        let now = Instant::now();

        assert!(throttle.first_refusal(current, Some(now + WINDOW)));
        assert!(throttle.first_refusal(past, Some(now)));
        throttle.sweep(now);

        assert!(!throttle.first_refusal(current, Some(now + WINDOW)));
        assert_eq!(throttle.announced.len(), 1);
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
