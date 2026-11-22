mod decision;
mod window;

pub use decision::Decision;
pub use window::Window;

use std::time::Duration;

use abnegate_http::{RateLimitConfig, RateLimiter};
use redis::aio::ConnectionManager;
use uuid::Uuid;

use crate::cluster::{Bus, Namespace};

const PREFIX: &str = "zone";
const SCOPE: &str = "limit";
const TIMEOUT: Duration = Duration::from_millis(500);

/// A fixed-window rate limit every replica counts into together, through the
/// Valkey the bus is connected to. While the bus has no Valkey, or Valkey
/// does not answer, each replica limits on its own count instead.
#[derive(Clone)]
pub struct Limiter {
    local: RateLimiter<Uuid>,
    bus: Bus,
}

impl Limiter {
    pub fn new(local: RateLimiter<Uuid>, bus: Bus) -> Self {
        Self { local, bus }
    }

    /// The same limiter, sharing its local counts, on another bus.
    pub fn with_bus(&self, bus: Bus) -> Self {
        Self::new(self.local.clone(), bus)
    }

    pub fn config(&self) -> &RateLimitConfig {
        self.local.config()
    }

    /// Counts a request for `user` and decides whether it may proceed.
    pub async fn check(&self, user: Uuid) -> Decision {
        let Some(connection) = self.bus.valkey().filter(|_| self.bus.connected()) else {
            return self.local.check_rate_limit(user).into();
        };
        match self.count(connection, user).await {
            Ok(decision) => decision,
            Err(reason) => {
                tracing::warn!(
                    %reason,
                    "Valkey could not count a rate-limited request, so this replica limits it on its own count"
                );
                self.local.check_rate_limit(user).into()
            }
        }
    }

    async fn count(
        &self,
        mut connection: ConnectionManager,
        user: Uuid,
    ) -> Result<Decision, String> {
        let window = Window::current(self.config().window);
        let key = key(self.bus.namespace(), user, window.index);
        let mut pipeline = redis::pipe();
        pipeline
            .atomic()
            .incr(&key, 1)
            .expire(&key, window.seconds)
            .ignore();
        let request = pipeline.query_async::<(u64,)>(&mut connection);
        let (count,) = tokio::time::timeout(TIMEOUT, request)
            .await
            .map_err(|_| "Valkey did not answer in time".to_string())?
            .map_err(|error| error.to_string())?;
        Ok(window.decide(count, self.config().limit))
    }
}

pub fn key(namespace: &Namespace, user: Uuid, window: u128) -> String {
    format!("{PREFIX}:{namespace}:{SCOPE}:{user}:{window}")
}

#[cfg(test)]
mod tests {
    use metrics::Gauge;

    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-limiter-passphrase-of-at-least-32-bytes";
    const LIMIT: u32 = 3;
    const WINDOW: Duration = Duration::from_secs(60);

    fn local() -> RateLimiter<Uuid> {
        RateLimiter::new(RateLimitConfig::new(LIMIT, WINDOW))
    }

    fn closed_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    async fn limited_locally(limiter: &Limiter) {
        let user = Uuid::new_v4();
        for remaining in (0..LIMIT).rev() {
            let decision = limiter.check(user).await;
            assert!(decision.allowed);
            assert_eq!(decision.remaining, remaining);
            assert!(decision.reset_at.is_some());
        }
        let refused = limiter.check(user).await;
        assert!(!refused.allowed);
        assert_eq!(refused.remaining, 0);
        assert!(limiter.check(Uuid::new_v4()).await.allowed);
    }

    #[test]
    fn a_key_names_the_namespace_the_user_and_the_window() {
        let namespace = Namespace::derive(&secret::derive(PASSPHRASE).unwrap());
        let user = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        assert_eq!(
            key(&namespace, user, 29_000_000),
            format!("zone:{namespace}:limit:550e8400-e29b-41d4-a716-446655440000:29000000")
        );
    }

    #[tokio::test]
    async fn without_valkey_a_lone_replica_limits_on_its_own_count() {
        let master = secret::derive(PASSPHRASE).unwrap();
        let limiter = Limiter::new(local(), Bus::loopback(&master));

        limited_locally(&limiter).await;
    }

    #[tokio::test]
    async fn with_valkey_unreachable_each_replica_limits_on_its_own_count() {
        let master = secret::derive(PASSPHRASE).unwrap();
        let url = format!("redis://127.0.0.1:{}", closed_port());
        let limiter = Limiter::new(local(), Bus::connect(&url, &master, Gauge::noop()));

        limited_locally(&limiter).await;
    }

    #[tokio::test]
    async fn a_limiter_moved_to_another_bus_keeps_its_local_counts() {
        let master = secret::derive(PASSPHRASE).unwrap();
        let limiter = Limiter::new(local(), Bus::loopback(&master));
        let user = Uuid::new_v4();
        for _ in 0..LIMIT {
            assert!(limiter.check(user).await.allowed);
        }

        let moved = limiter.with_bus(Bus::loopback(&master));

        assert!(!moved.check(user).await.allowed);
        assert_eq!(moved.config().limit, LIMIT);
    }
}
