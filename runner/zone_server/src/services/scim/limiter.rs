use std::fmt::Display;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use abnegate_http::{RateLimitConfig, RateLimiter};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use uuid::Uuid;

use super::roster::Roster;
use crate::cluster::Bus;
use crate::services::limit::{self, Decision, Scope, open};
use crate::services::sso::throttle;

/// Every replica counts an organization's requests and an address's rejected tokens into one
/// limit through the Valkey the bus is connected to; without it, each replica limits on its own
/// count. Which addresses are refused, the active token hashes and which refusals were recorded
/// stay with each replica: a replica that has not yet seen an address refused looks up one more
/// token, whose rejection the shared count refuses, before it refuses the address too.
pub struct Limiter {
    organizations: limit::Limiter<Uuid>,
    failures: limit::Limiter<IpAddr>,
    refused: Arc<DashMap<IpAddr, Instant>>,
    roster: Arc<Roster>,
    recorded: Arc<DashMap<Uuid, Option<Instant>>>,
}

impl Limiter {
    pub fn new(
        organization_requests: u32,
        window: Duration,
        address_failures: u32,
        reload: Duration,
        bus: Bus,
    ) -> Self {
        let organizations = RateLimiter::new(RateLimitConfig::new(organization_requests, window));
        let failures = RateLimiter::new(RateLimitConfig::new(address_failures, throttle::WINDOW));
        Self {
            organizations: limit::Limiter::new(organizations, bus.clone(), Scope::Scim),
            failures: limit::Limiter::new(failures, bus, Scope::ScimFailures),
            refused: Arc::default(),
            roster: Arc::new(Roster::new(reload)),
            recorded: Arc::default(),
        }
    }

    /// The same limiter, sharing its local counts, refusals, roster and records, on another bus.
    pub fn with_bus(&self, bus: Bus) -> Self {
        Self {
            organizations: self.organizations.with_bus(bus.clone()),
            failures: self.failures.with_bus(bus),
            refused: Arc::clone(&self.refused),
            roster: Arc::clone(&self.roster),
            recorded: Arc::clone(&self.recorded),
        }
    }

    pub async fn screen<Load, Loading, Error>(
        &self,
        address: Option<IpAddr>,
        hash: &str,
        load: Load,
    ) -> Option<Instant>
    where
        Load: FnOnce() -> Loading,
        Loading: Future<Output = Result<Vec<String>, Error>>,
        Error: Display,
    {
        let reset_at = self.refusal(address)?;
        (!self.roster.recognizes(hash, load).await).then_some(reset_at)
    }

    fn refusal(&self, address: Option<IpAddr>) -> Option<Instant> {
        let reset_at = *self.refused.get(&throttle::bucket(address))?;
        (reset_at > Instant::now()).then_some(reset_at)
    }

    pub fn accept(&self, hash: &str) {
        self.roster.admit(hash);
    }

    pub async fn reject(&self, address: Option<IpAddr>, hash: &str) -> Option<Instant> {
        self.roster.forget(hash);
        let bucket = throttle::bucket(address);
        let decision = self.failures.check(bucket).await;
        if decision.allowed {
            return None;
        }
        let reset_at = decision
            .reset_at
            .unwrap_or_else(|| Instant::now() + throttle::WINDOW);
        self.refused.insert(bucket, reset_at);
        Some(reset_at)
    }

    pub async fn check(&self, organization_id: Uuid) -> Decision {
        self.organizations.check(organization_id).await
    }

    pub fn window(&self) -> Duration {
        self.organizations.config().window
    }

    pub fn records(&self, organization_id: Uuid, decision: &Decision) -> bool {
        !decision.allowed && self.first_refusal(organization_id, decision.reset_at, Instant::now())
    }

    /// A shared count measures its reset afresh for every request, so a refusal belongs to the
    /// recorded window while that window has not reset, not only when the two resets are equal.
    fn first_refusal(
        &self,
        organization_id: Uuid,
        reset_at: Option<Instant>,
        now: Instant,
    ) -> bool {
        match self.recorded.entry(organization_id) {
            Entry::Occupied(recorded) if open(*recorded.get(), now) => false,
            Entry::Occupied(mut recorded) => {
                recorded.insert(reset_at);
                true
            }
            Entry::Vacant(recorded) => {
                recorded.insert(reset_at);
                true
            }
        }
    }

    pub fn cleanup(&self) {
        self.organizations.cleanup();
        self.failures.cleanup();
        let now = Instant::now();
        self.refused.retain(|_, reset_at| *reset_at > now);
        self.recorded.retain(|_, reset_at| open(*reset_at, now));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::secret;
    use crate::services::limit::Window;

    const WINDOW: Duration = Duration::from_secs(60);
    const PASSPHRASE: &str = "a-scim-limiter-passphrase-of-at-least-32-bytes";

    fn limiter(organization_requests: u32, address_failures: u32) -> Limiter {
        let bus = Bus::loopback(&secret::derive(PASSPHRASE).unwrap());
        Limiter::new(organization_requests, WINDOW, address_failures, WINDOW, bus)
    }

    async fn screened(limiter: &Limiter, address: Option<IpAddr>, hash: &str) -> bool {
        let unloaded = || async { Ok::<_, &str>(Vec::new()) };
        limiter.screen(address, hash, unloaded).await.is_some()
    }

    #[tokio::test]
    async fn only_the_first_refusal_in_a_window_is_recorded() {
        let limiter = limiter(1, 10);
        let organization = Uuid::new_v4();
        let neighbour = Uuid::new_v4();

        let allowed = limiter.check(organization).await;
        assert!(allowed.allowed);
        assert!(!limiter.records(organization, &allowed));
        let first = limiter.check(organization).await;
        assert!(limiter.records(organization, &first));
        for _ in 0..3 {
            let repeated = limiter.check(organization).await;
            assert!(!limiter.records(organization, &repeated));
        }

        limiter.check(neighbour).await;
        let refused = limiter.check(neighbour).await;
        assert!(limiter.records(neighbour, &refused));
    }

    #[tokio::test]
    async fn a_new_window_records_again() {
        let limiter = limiter(1, 10);
        let organization = Uuid::new_v4();
        let now = Instant::now();

        assert!(limiter.first_refusal(organization, Some(now + WINDOW), now));
        assert!(!limiter.first_refusal(organization, Some(now + WINDOW), now));
        assert!(limiter.first_refusal(organization, Some(now + WINDOW + WINDOW), now + WINDOW));
    }

    #[tokio::test]
    async fn a_shared_count_records_once_although_each_refusal_measures_its_own_reset() {
        let limiter = limiter(1, 10);
        let organization = Uuid::new_v4();
        let window = Window::current(WINDOW);

        let first = window.decide(2, 1);
        std::thread::sleep(Duration::from_millis(1));
        let second = window.decide(3, 1);

        assert_ne!(first.reset_at, second.reset_at);
        assert!(limiter.records(organization, &first));
        assert!(!limiter.records(organization, &second));
    }

    #[tokio::test]
    async fn a_limiter_moved_to_another_bus_keeps_its_counts_refusals_and_records() {
        let limiter = limiter(1, 1);
        let organization = Uuid::new_v4();
        let address = Some(IpAddr::from([198, 51, 100, 7]));
        assert!(limiter.check(organization).await.allowed);
        let refused = limiter.check(organization).await;
        assert!(limiter.records(organization, &refused));
        limiter.accept("accepted");
        limiter.reject(address, "guess").await;
        let reset_at = limiter.reject(address, "guess").await;

        let moved = limiter.with_bus(Bus::loopback(&secret::derive(PASSPHRASE).unwrap()));

        let again = moved.check(organization).await;
        assert!(!again.allowed);
        assert!(!moved.records(organization, &again));
        assert_eq!(moved.refusal(address), reset_at);
        assert!(!screened(&moved, address, "accepted").await);
        assert_eq!(moved.window(), WINDOW);
    }

    #[tokio::test]
    async fn only_rejected_tokens_draw_on_an_address_budget() {
        let limiter = limiter(100, 2);
        let flooding = Some(IpAddr::from([198, 51, 100, 7]));
        let quiet = Some(IpAddr::from([198, 51, 100, 8]));

        for _ in 0..10 {
            assert!(!screened(&limiter, flooding, "accepted").await);
            limiter.accept("accepted");
        }
        for _ in 0..2 {
            assert!(!screened(&limiter, flooding, "guess").await);
            assert_eq!(limiter.reject(flooding, "guess").await, None);
        }
        let reset_at = limiter.reject(flooding, "guess").await;
        assert!(reset_at.is_some_and(|reset_at| reset_at > Instant::now()));

        assert!(screened(&limiter, flooding, "guess").await);
        assert!(!screened(&limiter, quiet, "guess").await);
    }

    #[tokio::test]
    async fn an_accepted_token_passes_an_address_refused_for_failures() {
        let limiter = limiter(100, 1);
        let address = Some(IpAddr::from([198, 51, 100, 7]));
        limiter.accept("accepted");
        limiter.reject(address, "guess").await;
        limiter.reject(address, "guess").await;

        assert!(!screened(&limiter, address, "accepted").await);
        assert!(screened(&limiter, address, "guess").await);

        limiter.reject(address, "accepted").await;
        assert!(screened(&limiter, address, "accepted").await);
    }

    #[tokio::test]
    async fn a_refused_address_loads_the_active_tokens_once_a_reload_interval() {
        let limiter = limiter(100, 1);
        let address = Some(IpAddr::from([198, 51, 100, 7]));
        let loads = Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            async { Ok::<_, &str>(vec!["active".to_owned()]) }
        };

        assert_eq!(limiter.screen(address, "guess", load).await, None);
        assert_eq!(loads.get(), 0, "an address under its budget loads nothing");
        limiter.reject(address, "guess").await;
        limiter.reject(address, "guess").await;

        for _ in 0..50 {
            assert!(limiter.screen(address, "guess", load).await.is_some());
        }
        assert_eq!(limiter.screen(address, "active", load).await, None);
        assert_eq!(loads.get(), 1);
    }

    #[tokio::test]
    async fn a_cleanup_keeps_the_current_window() {
        let limiter = limiter(1, 1);
        let organization = Uuid::new_v4();
        let address = Some(IpAddr::from([198, 51, 100, 7]));

        assert!(limiter.check(organization).await.allowed);
        let refused = limiter.check(organization).await;
        assert!(limiter.records(organization, &refused));
        limiter.reject(address, "guess").await;
        let reset_at = limiter.reject(address, "guess").await;
        limiter.cleanup();

        let repeated = limiter.check(organization).await;
        assert!(!repeated.allowed);
        assert!(!limiter.records(organization, &repeated));
        assert_eq!(limiter.refusal(address), reset_at);
    }
}
