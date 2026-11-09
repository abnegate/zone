mod claim;

pub(crate) use claim::Claim;

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

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

        let cancelled = tokio::time::timeout(std::time::Duration::from_millis(10), reading).await;

        assert!(cancelled.is_err());
        assert!(!in_flight.contains(&"tree"));
    }
}
