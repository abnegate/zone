mod decision;
mod entry;
mod slot;
#[cfg(test)]
mod tests;

use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use dashmap::DashMap;
use tokio::time::Instant;

use super::fault::Fault;
use super::freshness::Freshness;
use super::snapshot::Snapshot;
use slot::Slot;

pub const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);
pub const IDLE_EXPIRY: Duration = Duration::from_secs(60 * 60);
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

pub struct Cache<K, T> {
    ttl: Duration,
    floor: Duration,
    slots: DashMap<K, Arc<Slot<T>>>,
    swept_at: Mutex<Option<DateTime<Utc>>>,
}

impl<K: Eq + Hash + Clone, T> Cache<K, T> {
    pub fn new(ttl: Duration, floor: Duration) -> Self {
        Self {
            ttl,
            floor,
            slots: DashMap::new(),
            swept_at: Mutex::new(None),
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
    ) -> Result<(Snapshot<T>, Freshness), Fault>
    where
        Fetch: FnOnce(Option<Arc<T>>) -> Fetched,
        Fetched: Future<Output = Result<T, Fault>>,
    {
        self.sweep(now);
        let slot = self.slot(key.clone(), now);
        let decide = || slot.decide(refresh, now, self.ttl, self.floor);
        if let Some(answer) = decide().answer() {
            return answer;
        }
        let _flight = slot.flight().await;
        if let Some(answer) = decide().answer() {
            return answer;
        }
        let (previous, epoch) = slot.previous();
        let started = Instant::now();
        let fetched = fetch(previous).await;
        let settled_at = now + delta(started.elapsed());
        let answer = slot.settle(fetched, settled_at, epoch);
        if slot.vacant(settled_at) {
            self.slots
                .remove_if(&key, |_, held| Arc::ptr_eq(held, &slot));
        }
        answer
    }

    pub fn peek(&self, key: &K) -> Option<Snapshot<T>> {
        self.slots.get(key)?.peek()
    }

    pub fn invalidate(&self, key: &K) {
        if let Some(slot) = self.slots.get(key) {
            slot.expire();
        }
    }

    #[cfg(test)]
    pub(crate) fn holds(&self, key: &K) -> bool {
        self.slots.contains_key(key)
    }

    fn slot(&self, key: K, now: DateTime<Utc>) -> Arc<Slot<T>> {
        let slot = Arc::clone(&self.slots.entry(key).or_default());
        slot.touch(now);
        slot
    }

    fn sweep(&self, now: DateTime<Utc>) {
        {
            let mut swept_at = self.swept_at.lock().unwrap_or_else(PoisonError::into_inner);
            if swept_at.is_some_and(|at| now.signed_duration_since(at) < delta(SWEEP_INTERVAL)) {
                return;
            }
            *swept_at = Some(now);
        }
        self.slots
            .retain(|_, slot| Arc::strong_count(slot) > 1 || !slot.idle(now, IDLE_EXPIRY));
    }
}

pub(super) fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}
