use std::net::IpAddr;
use std::time::{Duration, Instant};

use abnegate_http::{Decision, RateLimitConfig, RateLimiter};
use dashmap::DashMap;
use uuid::Uuid;

use crate::services::sso::throttle::Throttle;

#[derive(Debug)]
pub struct Limiter {
    organizations: RateLimiter<Uuid>,
    addresses: Throttle,
    recorded: DashMap<Uuid, Option<Instant>>,
}

impl Limiter {
    pub fn new(organization_requests: u32, window: Duration, address_requests: u32) -> Self {
        Self {
            organizations: RateLimiter::new(RateLimitConfig::new(organization_requests, window)),
            addresses: Throttle::new(address_requests),
            recorded: DashMap::new(),
        }
    }

    pub fn screen(&self, address: Option<IpAddr>) -> Decision {
        self.addresses.check(address)
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
        self.addresses.cleanup();
        let now = Instant::now();
        self.recorded
            .retain(|_, reset_at| reset_at.is_none_or(|reset_at| reset_at > now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(60);

    #[test]
    fn only_the_first_refusal_in_a_window_is_recorded() {
        let limiter = Limiter::new(1, WINDOW, 10);
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
        let limiter = Limiter::new(1, WINDOW, 10);
        let organization = Uuid::new_v4();
        let now = Instant::now();

        assert!(limiter.first_refusal(organization, Some(now + WINDOW)));
        assert!(!limiter.first_refusal(organization, Some(now + WINDOW)));
        assert!(limiter.first_refusal(organization, Some(now + WINDOW + WINDOW)));
    }

    #[test]
    fn each_address_is_screened_on_its_own_budget() {
        let limiter = Limiter::new(100, WINDOW, 2);
        let flooding = Some(IpAddr::from([198, 51, 100, 7]));
        let quiet = Some(IpAddr::from([198, 51, 100, 8]));

        for _ in 0..2 {
            assert!(limiter.screen(flooding).allowed);
        }
        let refused = limiter.screen(flooding);
        assert!(!refused.allowed);
        assert!(refused.reset_at.is_some());
        assert!(limiter.screen(quiet).allowed);
    }

    #[test]
    fn a_cleanup_keeps_the_current_window() {
        let limiter = Limiter::new(1, WINDOW, 10);
        let organization = Uuid::new_v4();

        assert!(limiter.check(organization).allowed);
        let refused = limiter.check(organization);
        assert!(limiter.records(organization, &refused));
        limiter.cleanup();

        let repeated = limiter.check(organization);
        assert!(!repeated.allowed);
        assert!(!limiter.records(organization, &repeated));
    }
}
