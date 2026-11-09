mod claim;

pub(crate) use claim::Claim;

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct InFlight<Key: Eq + Hash> {
    keys: Arc<Mutex<HashSet<Key>>>,
}

impl<Key: Eq + Hash> Clone for InFlight<Key> {
    fn clone(&self) -> Self {
        Self {
            keys: Arc::clone(&self.keys),
        }
    }
}

impl<Key: Clone + Eq + Hash> InFlight<Key> {
    pub(crate) fn new() -> Self {
        Self {
            keys: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Claims `key` for the caller's read until the returned claim is dropped,
    /// however that read ends; `None` while another read holds it
    pub(crate) fn claim(&self, key: &Key) -> Option<Claim<Key>> {
        let claimed = self
            .keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone());
        claimed.then(|| Claim::new(Arc::clone(&self.keys), key.clone()))
    }

    /// Claims `key` as soon as the read holding it lets go, trying once every
    /// `interval` up to `attempts` times; `None` while it is still held
    pub(crate) async fn claim_when_released(
        &self,
        key: &Key,
        attempts: u64,
        interval: Duration,
    ) -> Option<Claim<Key>> {
        for _ in 0..attempts {
            tokio::time::sleep(interval).await;
            if let Some(claim) = self.claim(key) {
                return Some(claim);
            }
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &Key) -> bool {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_claimed_by_one_read_at_a_time() {
        let in_flight = InFlight::new();

        let claim = in_flight.claim(&"tree");

        assert!(claim.is_some());
        assert!(in_flight.contains(&"tree"));
        assert!(in_flight.claim(&"tree").is_none());
        assert!(in_flight.claim(&"other").is_some());
    }

    #[test]
    fn dropping_a_claim_releases_its_key() {
        let in_flight = InFlight::new();

        drop(in_flight.claim(&"tree"));

        assert!(!in_flight.contains(&"tree"));
        assert!(in_flight.claim(&"tree").is_some());
    }

    #[test]
    fn a_refused_claim_never_releases_the_key_another_read_holds() {
        let in_flight = InFlight::new();
        let held = in_flight.claim(&"tree");

        drop(in_flight.claim(&"tree"));

        assert!(in_flight.contains(&"tree"));
        drop(held);
        assert!(!in_flight.contains(&"tree"));
    }

    const ATTEMPTS: u64 = 10;

    const INTERVAL: Duration = Duration::from_millis(100);

    #[tokio::test(start_paused = true)]
    async fn a_waiter_claims_a_key_at_its_first_try_after_the_holder_lets_go() {
        let in_flight = InFlight::new();
        let held = in_flight.claim(&"tree");
        let started = tokio::time::Instant::now();

        let (claimed, ()) = tokio::join!(
            in_flight.claim_when_released(&"tree", ATTEMPTS, INTERVAL),
            async {
                tokio::time::sleep(INTERVAL * 3 / 2).await;
                drop(held);
            }
        );

        assert!(claimed.is_some());
        assert_eq!(started.elapsed(), INTERVAL * 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiter_gives_up_while_the_key_stays_held() {
        let in_flight = InFlight::new();
        let _held = in_flight.claim(&"tree");
        let started = tokio::time::Instant::now();

        let claimed = in_flight
            .claim_when_released(&"tree", ATTEMPTS, INTERVAL)
            .await;

        assert!(claimed.is_none());
        assert_eq!(started.elapsed(), INTERVAL * ATTEMPTS as u32);
    }

    #[tokio::test(start_paused = true)]
    async fn only_one_waiter_claims_a_released_key() {
        let in_flight = InFlight::new();
        let held = in_flight.claim(&"tree");

        let (first, second, ()) = tokio::join!(
            in_flight.claim_when_released(&"tree", ATTEMPTS, INTERVAL),
            in_flight.claim_when_released(&"tree", ATTEMPTS, INTERVAL),
            async {
                tokio::time::sleep(INTERVAL / 2).await;
                drop(held);
            }
        );

        assert!(first.is_some() != second.is_some());
    }

    #[tokio::test]
    async fn a_cancelled_read_releases_its_key() {
        let in_flight = InFlight::new();
        let reading = {
            let in_flight = in_flight.clone();
            async move {
                let _claim = in_flight.claim(&"tree");
                std::future::pending::<()>().await;
            }
        };

        let cancelled = tokio::time::timeout(Duration::from_millis(10), reading).await;

        assert!(cancelled.is_err());
        assert!(!in_flight.contains(&"tree"));
    }
}
