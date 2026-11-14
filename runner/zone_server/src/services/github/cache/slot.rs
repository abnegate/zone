use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;

use super::super::fault::Fault;
use super::super::freshness::Freshness;
use super::super::snapshot::Snapshot;
use super::decision::Decision;
use super::entry::Entry;

pub(super) struct Slot<T> {
    flight: tokio::sync::Mutex<()>,
    entry: Mutex<Entry<T>>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            flight: tokio::sync::Mutex::new(()),
            entry: Mutex::new(Entry::default()),
        }
    }
}

impl<T> Slot<T> {
    pub(super) async fn flight(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.flight.lock().await
    }

    pub(super) fn decide(
        &self,
        refresh: bool,
        now: DateTime<Utc>,
        ttl: Duration,
        floor: Duration,
    ) -> Decision<T> {
        self.entry().decide(refresh, now, ttl, floor)
    }

    pub(super) fn previous(&self) -> (Option<Arc<T>>, u64) {
        self.entry().previous()
    }

    pub(super) fn settle(
        &self,
        fetched: Result<T, Fault>,
        now: DateTime<Utc>,
        epoch: u64,
    ) -> Result<(Snapshot<T>, Freshness), Fault> {
        self.entry().settle(fetched, now, epoch)
    }

    pub(super) fn expire(&self) {
        self.entry().expire();
    }

    pub(super) fn touch(&self, now: DateTime<Utc>) {
        self.entry().touch(now);
    }

    pub(super) fn idle(&self, now: DateTime<Utc>, expiry: Duration) -> bool {
        self.entry().idle(now, expiry)
    }

    pub(super) fn vacant(&self, now: DateTime<Utc>) -> bool {
        self.entry().vacant(now)
    }

    pub(super) fn peek(&self) -> Option<Snapshot<T>> {
        self.entry().peek()
    }

    fn entry(&self) -> MutexGuard<'_, Entry<T>> {
        self.entry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
