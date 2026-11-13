mod decision;
mod entry;
mod slot;
#[cfg(test)]
mod tests;

use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use dashmap::DashMap;

use super::freshness::Freshness;
use super::snapshot::Snapshot;
use slot::Slot;

pub const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);

pub struct Cache<K, T> {
    ttl: Duration,
    floor: Duration,
    slots: DashMap<K, Arc<Slot<T>>>,
}

impl<K: Eq + Hash + Clone, T> Cache<K, T> {
    pub fn new(ttl: Duration, floor: Duration) -> Self {
        Self {
            ttl,
            floor,
            slots: DashMap::new(),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn floor(&self) -> Duration {
        self.floor
    }

    pub async fn read<Fetch, Fetched>(
        &self,
        key: K,
        refresh: bool,
        now: DateTime<Utc>,
        fetch: Fetch,
    ) -> Result<(Snapshot<T>, Freshness), String>
    where
        Fetch: FnOnce(Option<Arc<T>>) -> Fetched,
        Fetched: Future<Output = Result<T, String>>,
    {
        let slot = self.slot(key);
        let decide = || slot.decide(refresh, now, self.ttl, self.floor);
        if let Some(answer) = decide().answer() {
            return answer;
        }
        let _flight = slot.flight().await;
        if let Some(answer) = decide().answer() {
            return answer;
        }
        let (previous, epoch) = slot.previous();
        let fetched = fetch(previous).await;
        slot.settle(fetched, now, epoch)
    }

    pub fn peek(&self, key: &K) -> Option<Snapshot<T>> {
        self.slots.get(key)?.peek()
    }

    pub fn invalidate(&self, key: &K) {
        if let Some(slot) = self.slots.get(key) {
            slot.expire();
        }
    }

    pub fn invalidate_where(&self, matches: impl Fn(&K) -> bool) {
        for slot in self.slots.iter().filter(|slot| matches(slot.key())) {
            slot.expire();
        }
    }

    fn slot(&self, key: K) -> Arc<Slot<T>> {
        Arc::clone(&self.slots.entry(key).or_default())
    }
}
