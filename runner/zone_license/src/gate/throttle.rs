use std::net::IpAddr;
use std::time::Instant;

use abnegate_http::{Decision, RateLimitConfig, RateLimiter};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::gate::limited::Limited;
use crate::gate::limits::{Limits, WINDOW};
use crate::gate::throttle_key::ThrottleKey;

#[derive(Debug, Clone)]
pub struct Throttle {
    addresses: RateLimiter<ThrottleKey>,
    keys: RateLimiter<Uuid>,
}

impl Throttle {
    pub fn new(limits: Limits) -> Self {
        Self {
            addresses: RateLimiter::new(RateLimitConfig::new(limits.address.get(), WINDOW)),
            keys: RateLimiter::new(RateLimitConfig::new(limits.key.get(), WINDOW)),
        }
    }

    pub fn check_address(&self, address: IpAddr) -> Result<(), Limited> {
        admitted(self.addresses.check_rate_limit(ThrottleKey::from(address)))
    }

    pub fn check_key(&self, license: Uuid) -> Result<(), Limited> {
        admitted(self.keys.check_rate_limit(license))
    }

    pub fn spawn_cleanup(&self) -> JoinHandle<()> {
        let throttle = self.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(WINDOW);
            loop {
                ticks.tick().await;
                throttle.addresses.cleanup();
                throttle.keys.cleanup();
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
