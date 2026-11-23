use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use abnegate_http::{RateLimitConfig, RateLimiter};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;

use crate::auth::client_address::key;
use crate::cluster::Bus;
use crate::services::limit::{Decision, Limiter, Scope};

pub const WINDOW: Duration = Duration::from_secs(60);

const UNKNOWN: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Counts single sign-on starts and SAML posts per client address. Requests whose peer is unknown
/// share one bucket, so they are limited together rather than not at all. Every replica counts
/// into one limit through the Valkey the bus is connected to; without it, each replica limits on
/// its own count. Each replica announces refusals for itself.
pub struct Throttle {
    limiter: Limiter<IpAddr>,
    announced: Arc<DashMap<IpAddr, Option<Instant>>>,
}

impl Throttle {
    pub fn new(requests_per_minute: u32, bus: Bus) -> Self {
        let local = RateLimiter::new(RateLimitConfig::new(requests_per_minute, WINDOW));
        Self {
            limiter: Limiter::new(local, bus, Scope::Sso),
            announced: Arc::default(),
        }
    }

    /// The same throttle, sharing its local counts and announcements, on another bus.
    pub fn with_bus(&self, bus: Bus) -> Self {
        Self {
            limiter: self.limiter.with_bus(bus),
            announced: Arc::clone(&self.announced),
        }
    }

    pub async fn check(&self, address: Option<IpAddr>) -> Decision {
        self.limiter.check(bucket(address)).await
    }

    /// Whether a refusal is the address's first in its window, so it is worth a warning while
    /// the rest of a flood is not.
    pub fn announces(&self, address: Option<IpAddr>, decision: &Decision) -> bool {
        !decision.allowed && self.first_refusal(address, decision.reset_at, Instant::now())
    }

    /// A shared count measures its reset afresh for every request, so a refusal belongs to the
    /// announced window while that window has not reset, not only when the two resets are equal.
    fn first_refusal(
        &self,
        address: Option<IpAddr>,
        reset_at: Option<Instant>,
        now: Instant,
    ) -> bool {
        match self.announced.entry(bucket(address)) {
            Entry::Occupied(announced) if open(*announced.get(), now) => false,
            Entry::Occupied(mut announced) => {
                announced.insert(reset_at);
                true
            }
            Entry::Vacant(announced) => {
                announced.insert(reset_at);
                true
            }
        }
    }

    pub fn cleanup(&self) {
        self.limiter.cleanup();
        self.sweep(Instant::now());
    }

    fn sweep(&self, now: Instant) {
        self.announced.retain(|_, reset_at| open(*reset_at, now));
    }
}

pub fn bucket(address: Option<IpAddr>) -> IpAddr {
    address.map_or(UNKNOWN, key)
}

fn open(reset_at: Option<Instant>, now: Instant) -> bool {
    reset_at.is_none_or(|reset_at| reset_at > now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;
    use crate::services::limit::Window;

    const PASSPHRASE: &str = "a-throttle-passphrase-of-at-least-32-bytes";

    fn loopback() -> Bus {
        Bus::loopback(&secret::derive(PASSPHRASE).unwrap())
    }

    fn throttle(requests_per_minute: u32) -> Throttle {
        Throttle::new(requests_per_minute, loopback())
    }

    fn address(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    async fn exhausted(throttle: &Throttle, client: Option<IpAddr>) {
        for _ in 0..2 {
            assert!(throttle.check(client).await.allowed, "{client:?}");
        }
        assert!(!throttle.check(client).await.allowed, "{client:?}");
    }

    #[tokio::test]
    async fn each_address_has_its_own_bucket() {
        let throttle = throttle(2);

        exhausted(&throttle, address("198.51.100.7")).await;
        assert!(throttle.check(address("198.51.100.8")).await.allowed);
    }

    #[tokio::test]
    async fn requests_without_an_address_share_one_bucket() {
        let throttle = throttle(2);

        exhausted(&throttle, None).await;
        assert!(!throttle.check(None).await.allowed);
        assert!(throttle.check(address("198.51.100.7")).await.allowed);
    }

    #[tokio::test]
    async fn an_ipv6_network_shares_one_bucket() {
        let throttle = throttle(2);

        exhausted(&throttle, address("2001:db8:1:2::1")).await;
        assert!(
            !throttle
                .check(address("2001:db8:1:2:ffff::9"))
                .await
                .allowed
        );
        assert!(throttle.check(address("2001:db8:1:3::1")).await.allowed);
    }

    #[tokio::test]
    async fn an_ipv4_mapped_address_shares_the_ipv4_bucket() {
        let throttle = throttle(2);

        exhausted(&throttle, address("198.51.100.7")).await;
        assert!(!throttle.check(address("::ffff:198.51.100.7")).await.allowed);
    }

    #[tokio::test]
    async fn a_refusal_says_when_the_window_resets() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");

        assert!(throttle.check(client).await.allowed);
        let refused = throttle.check(client).await;

        assert!(!refused.allowed);
        assert_eq!(refused.remaining, 0);
        assert!(refused.reset_at.is_some());
    }

    #[tokio::test]
    async fn only_the_first_refusal_in_a_window_is_announced() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");

        let allowed = throttle.check(client).await;
        assert!(!throttle.announces(client, &allowed));
        let first = throttle.check(client).await;
        assert!(throttle.announces(client, &first));
        for _ in 0..3 {
            let repeated = throttle.check(client).await;
            assert!(!throttle.announces(client, &repeated));
        }
    }

    #[tokio::test]
    async fn a_shared_count_announces_once_although_each_refusal_measures_its_own_reset() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");
        let window = Window::current(WINDOW);

        let first = window.decide(2, 1);
        std::thread::sleep(Duration::from_millis(1));
        let second = window.decide(3, 1);

        assert_ne!(first.reset_at, second.reset_at);
        assert!(throttle.announces(client, &first));
        assert!(!throttle.announces(client, &second));
    }

    #[tokio::test]
    async fn each_address_announces_its_own_first_refusal() {
        let throttle = throttle(1);

        for client in [
            address("198.51.100.7"),
            address("198.51.100.8"),
            address("2001:db8:1:2::1"),
            None,
        ] {
            throttle.check(client).await;
            let refused = throttle.check(client).await;
            assert!(throttle.announces(client, &refused), "{client:?}");
        }
        let network = address("2001:db8:1:2::9");
        let refused = throttle.check(network).await;
        assert!(!throttle.announces(network, &refused));
    }

    #[tokio::test]
    async fn a_new_window_announces_again() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");
        let now = Instant::now();

        assert!(throttle.first_refusal(client, Some(now + WINDOW), now));
        assert!(!throttle.first_refusal(client, Some(now + WINDOW), now));
        assert!(throttle.first_refusal(client, Some(now + WINDOW + WINDOW), now + WINDOW));
    }

    #[tokio::test]
    async fn a_window_too_long_to_reset_announces_once() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");
        let now = Instant::now();

        assert!(throttle.first_refusal(client, None, now));
        assert!(!throttle.first_refusal(client, None, now + WINDOW));
    }

    #[tokio::test]
    async fn a_sweep_forgets_announcements_of_past_windows_only() {
        let throttle = throttle(1);
        let current = address("198.51.100.7");
        let past = address("198.51.100.8");
        let now = Instant::now();

        assert!(throttle.first_refusal(current, Some(now + WINDOW), now));
        assert!(throttle.first_refusal(past, Some(now), now));
        throttle.sweep(now);

        assert!(!throttle.first_refusal(current, Some(now + WINDOW), now));
        assert_eq!(throttle.announced.len(), 1);
    }

    #[tokio::test]
    async fn a_sweep_keeps_the_current_window() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");

        assert!(throttle.check(client).await.allowed);
        throttle.cleanup();

        assert!(!throttle.check(client).await.allowed);
    }

    #[tokio::test]
    async fn a_throttle_moved_to_another_bus_keeps_its_local_counts_and_announcements() {
        let throttle = throttle(1);
        let client = address("198.51.100.7");
        assert!(throttle.check(client).await.allowed);
        let refused = throttle.check(client).await;
        assert!(throttle.announces(client, &refused));

        let moved = throttle.with_bus(loopback());

        let again = moved.check(client).await;
        assert!(!again.allowed);
        assert!(!moved.announces(client, &again));
        throttle.sweep(Instant::now() + WINDOW + WINDOW);
        assert!(moved.announced.is_empty());
    }

    #[test]
    fn the_window_is_a_minute() {
        assert_eq!(WINDOW, Duration::from_secs(60));
    }
}
