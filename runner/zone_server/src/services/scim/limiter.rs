use std::fmt::Display;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use abnegate_http::{Decision, RateLimitConfig, RateLimiter};
use dashmap::DashMap;
use uuid::Uuid;

use super::roster::Roster;
use crate::cluster::Bus;
use crate::services::sso::throttle::{self, Throttle};

pub struct Limiter {
    organizations: RateLimiter<Uuid>,
    failures: Throttle,
    refused: DashMap<IpAddr, Instant>,
    roster: Roster,
    recorded: DashMap<Uuid, Option<Instant>>,
}

impl Limiter {
    pub fn new(
        organization_requests: u32,
        window: Duration,
        address_failures: u32,
        reload: Duration,
        bus: Bus,
    ) -> Self {
        Self {
            organizations: RateLimiter::new(RateLimitConfig::new(organization_requests, window)),
            failures: Throttle::new(address_failures, bus),
            refused: DashMap::new(),
            roster: Roster::new(reload),
            recorded: DashMap::new(),
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
        let decision = self.failures.check(address).await;
        if decision.allowed {
            return None;
        }
        let reset_at = decision
            .reset_at
            .unwrap_or_else(|| Instant::now() + throttle::WINDOW);
        self.refused.insert(throttle::bucket(address), reset_at);
        Some(reset_at)
    }

    pub fn check(&self, organization_id: Uuid) -> Decision {
        self.organizations.check_rate_limit(organization_id)
    }

    pub fn window(&self) -> Duration {
        self.organizations.config().window
    }

    pub fn records(&self, organization_id: Uuid, decision: &Decision) -> bool {
        !decision.allowed && self.first_refusal(organization_id, decision.reset_at)
    }

    fn first_refusal(&self, organization_id: Uuid, reset_at: Option<Instant>) -> bool {
        self.recorded
            .insert(organization_id, reset_at)
            .is_none_or(|recorded| recorded != reset_at)
    }

    pub fn cleanup(&self) {
        self.organizations.cleanup();
        self.failures.cleanup();
        let now = Instant::now();
        self.refused.retain(|_, reset_at| *reset_at > now);
        self.recorded
            .retain(|_, reset_at| reset_at.is_none_or(|reset_at| reset_at > now));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::secret;

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

    #[test]
    fn only_the_first_refusal_in_a_window_is_recorded() {
        let limiter = limiter(1, 10);
        let organization = Uuid::new_v4();
        let neighbour = Uuid::new_v4();

        let allowed = limiter.check(organization);
        assert!(allowed.allowed);
        assert!(!limiter.records(organization, &allowed));
        let first = limiter.check(organization);
        assert!(limiter.records(organization, &first));
        for _ in 0..3 {
            let repeated = limiter.check(organization);
            assert!(!limiter.records(organization, &repeated));
        }

        limiter.check(neighbour);
        let refused = limiter.check(neighbour);
        assert!(limiter.records(neighbour, &refused));
    }

    #[test]
    fn a_new_window_records_again() {
        let limiter = limiter(1, 10);
        let organization = Uuid::new_v4();
        let now = Instant::now();

        assert!(limiter.first_refusal(organization, Some(now + WINDOW)));
        assert!(!limiter.first_refusal(organization, Some(now + WINDOW)));
        assert!(limiter.first_refusal(organization, Some(now + WINDOW + WINDOW)));
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

        assert!(limiter.check(organization).allowed);
        let refused = limiter.check(organization);
        assert!(limiter.records(organization, &refused));
        limiter.reject(address, "guess").await;
        let reset_at = limiter.reject(address, "guess").await;
        limiter.cleanup();

        let repeated = limiter.check(organization);
        assert!(!repeated.allowed);
        assert!(!limiter.records(organization, &repeated));
        assert_eq!(limiter.refusal(address), reset_at);
    }
}
