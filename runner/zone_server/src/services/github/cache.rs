mod decision;
mod entry;
mod expiry;
mod slot;
#[cfg(test)]
mod tests;

use std::any::Any;
use std::hash::Hash;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use dashmap::DashMap;
use futures::FutureExt;
use tokio::time::Instant;

use super::client::REQUEST_FAILED;
use super::fault::Fault;
use super::freshness::Freshness;
use super::snapshot::Snapshot;
use decision::Decision;
use expiry::Expiry;
use slot::Slot;

pub const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);
pub const IDLE_EXPIRY: Duration = Duration::from_secs(60 * 60);
pub const REVALIDATE_WINDOW: Duration = Duration::from_secs(10 * 60);
pub const REFRESH_DEADLINE: Duration = Duration::from_secs(2 * 60);
const UNKNOWN_PANIC: &str = "unknown panic";
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

pub struct Cache<K, T> {
    ttl: Duration,
    floor: Duration,
    slots: Arc<DashMap<K, Arc<Slot<T>>>>,
    swept_at: Mutex<Option<DateTime<Utc>>>,
}

impl<K: Eq + Hash + Clone, T> Cache<K, T> {
    pub fn new(ttl: Duration, floor: Duration) -> Self {
        Self {
            ttl,
            floor,
            slots: Arc::new(DashMap::new()),
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
        let decide = || slot.decide(refresh, now, self.ttl, self.floor, Expiry::Wait);
        if let Some(answer) = decide().answer() {
            return answer;
        }
        let _flight = slot.flight().await;
        if let Some(answer) = decide().answer() {
            return answer;
        }
        self.fetched(&key, &slot, now, fetch).await
    }

    pub fn peek(&self, key: &K) -> Option<Snapshot<T>> {
        self.slots.get(key)?.peek()
    }

    pub fn invalidate(&self, key: &K) {
        if let Some(slot) = self.slots.get(key) {
            slot.expire();
        }
    }

    pub fn forget(&self, key: &K) {
        if let Some(slot) = self.slots.get(key) {
            slot.forget();
        }
    }

    #[cfg(test)]
    pub(crate) fn holds(&self, key: &K) -> bool {
        self.slots.contains_key(key)
    }

    async fn fetched<Fetch, Fetched>(
        &self,
        key: &K,
        slot: &Arc<Slot<T>>,
        now: DateTime<Utc>,
        fetch: Fetch,
    ) -> Result<(Snapshot<T>, Freshness), Fault>
    where
        Fetch: FnOnce(Option<Arc<T>>) -> Fetched,
        Fetched: Future<Output = Result<T, Fault>>,
    {
        let (previous, epoch) = slot.previous();
        settle(&self.slots, key, slot, fetch(previous), epoch, now).await
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

impl<K, T> Cache<K, T>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    pub async fn serve<Fetch, Fetched>(
        &self,
        key: K,
        refresh: bool,
        now: DateTime<Utc>,
        fetch: Fetch,
    ) -> Result<(Snapshot<T>, Freshness), Fault>
    where
        Fetch: FnOnce(Option<Arc<T>>) -> Fetched,
        Fetched: Future<Output = Result<T, Fault>> + Send + 'static,
    {
        self.sweep(now);
        let slot = self.slot(key.clone(), now);
        let decide = || slot.decide(refresh, now, self.ttl, self.floor, Expiry::Revalidate);
        let flight = match decide() {
            Decision::Revalidate(snapshot, freshness) => match slot.try_flight() {
                Some(flight) => flight,
                None => return Ok((snapshot, freshness)),
            },
            Decision::Fetch => slot.flight().await,
            Decision::Serve(snapshot, freshness) => return Ok((snapshot, freshness)),
            Decision::Refuse(fault) => return Err(fault),
        };
        match decide() {
            Decision::Revalidate(snapshot, freshness) => {
                let (previous, epoch) = slot.previous();
                let fetched = guarded(fetch(previous));
                let slots = Arc::clone(&self.slots);
                tokio::spawn(async move {
                    let _flight = flight;
                    settle(&slots, &key, &slot, fetched, epoch, now).await
                });
                Ok((snapshot, freshness))
            }
            Decision::Fetch => self.fetched(&key, &slot, now, fetch).await,
            Decision::Serve(snapshot, freshness) => Ok((snapshot, freshness)),
            Decision::Refuse(fault) => Err(fault),
        }
    }
}

async fn guarded<T>(fetched: impl Future<Output = Result<T, Fault>>) -> Result<T, Fault> {
    let bounded = tokio::time::timeout(REFRESH_DEADLINE, AssertUnwindSafe(fetched).catch_unwind());
    match bounded.await {
        Ok(Ok(fetched)) => fetched,
        Ok(Err(panic)) => {
            tracing::error!(
                panic = panicked(panic.as_ref()),
                "a background GitHub refresh panicked"
            );
            Err(Fault::upstream(REQUEST_FAILED))
        }
        Err(_) => {
            tracing::warn!(
                deadline = ?REFRESH_DEADLINE,
                "a background GitHub refresh ran past its deadline"
            );
            Err(Fault::upstream(REQUEST_FAILED))
        }
    }
}

fn panicked(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or(UNKNOWN_PANIC)
}

async fn settle<K, T, Fetched>(
    slots: &DashMap<K, Arc<Slot<T>>>,
    key: &K,
    slot: &Arc<Slot<T>>,
    fetched: Fetched,
    epoch: u64,
    now: DateTime<Utc>,
) -> Result<(Snapshot<T>, Freshness), Fault>
where
    K: Eq + Hash,
    Fetched: Future<Output = Result<T, Fault>>,
{
    let started = Instant::now();
    let fetched = fetched.await;
    let settled_at = now + delta(started.elapsed());
    let answer = slot.settle(fetched, settled_at, epoch);
    if slot.vacant(settled_at) {
        // Two holders are the map and this fetch; any more is a reader queued
        // on the slot that would otherwise settle into a slot the map dropped.
        slots.remove_if(key, |_, held| {
            Arc::ptr_eq(held, slot) && Arc::strong_count(held) == 2
        });
    }
    answer
}

pub(super) fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}
