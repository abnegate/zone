use std::net::IpAddr;
use std::time::Instant;

use abnegate_http::{Decision, RateLimitConfig, RateLimiter};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::gate::limited::Limited;
use crate::gate::limits::{Limits, WINDOW};
use crate::gate::throttle_key::ThrottleKey;

/// Each route owns one, so a client busy on one route keeps its allowance on
/// the other. Clones share their counts.
#[derive(Debug, Clone)]
pub struct Throttle {
    addresses: RateLimiter<ThrottleKey>,
    licenses: RateLimiter<Uuid>,
}

impl Throttle {
    pub fn new(limits: Limits) -> Self {
        Self {
            addresses: RateLimiter::new(RateLimitConfig::new(limits.address.get(), WINDOW)),
            licenses: RateLimiter::new(RateLimitConfig::new(limits.license.get(), WINDOW)),
        }
    }

    pub fn check_address(&self, address: IpAddr) -> Result<(), Limited> {
        admitted(self.addresses.check_rate_limit(ThrottleKey::from(address)))
    }

    pub fn check_license(&self, license: Uuid) -> Result<(), Limited> {
        admitted(self.licenses.check_rate_limit(license))
    }

    /// Drops spent windows once a `WINDOW`, until the task is aborted.
    pub fn spawn_cleanup(&self) -> JoinHandle<()> {
        let throttle = self.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(WINDOW);
            loop {
                ticks.tick().await;
                throttle.addresses.cleanup();
                throttle.licenses.cleanup();
            }
        })
    }
}

fn admitted(decision: Decision) -> Result<(), Limited> {
    if decision.allowed {
        return Ok(());
    }
    let wait = decision.reset_at.map_or(WINDOW, |reset| {
        reset.saturating_duration_since(Instant::now())
    });
    Err(Limited::after(wait))
}
