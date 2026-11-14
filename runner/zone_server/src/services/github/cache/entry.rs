use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;

use super::super::fault::Fault;
use super::super::freshness::Freshness;
use super::super::snapshot::Snapshot;
use super::RATE_LIMIT_BACKOFF;
use super::decision::Decision;
use super::delta;

pub(super) struct Entry<T> {
    snapshot: Option<Snapshot<T>>,
    failure: Option<Fault>,
    retry_after: Option<DateTime<Utc>>,
    used_at: Option<DateTime<Utc>>,
    expired: bool,
    epoch: u64,
}

impl<T> Default for Entry<T> {
    fn default() -> Self {
        Self {
            snapshot: None,
            failure: None,
            retry_after: None,
            used_at: None,
            expired: false,
            epoch: 0,
        }
    }
}

impl<T> Entry<T> {
    pub(super) fn decide(
        &self,
        refresh: bool,
        now: DateTime<Utc>,
        ttl: Duration,
        floor: Duration,
    ) -> Decision<T> {
        if self.backing_off(now) {
            let failure = self.failure.clone().unwrap_or(Fault::RateLimited {
                retry_after: None,
                exhausted: true,
            });
            return match &self.snapshot {
                Some(snapshot) => {
                    Decision::Serve(snapshot.warned(Some(failure.to_string())), Freshness::Stale)
                }
                None => Decision::Refuse(failure),
            };
        }
        let Some(snapshot) = self.snapshot.as_ref().filter(|_| !self.expired) else {
            return Decision::Fetch;
        };
        let age = now.signed_duration_since(snapshot.generated_at);
        match refresh {
            true if age < delta(floor) => Decision::Serve(snapshot.clone(), Freshness::Floored),
            false if age < delta(ttl) => Decision::Serve(snapshot.clone(), Freshness::Cached),
            _ => Decision::Fetch,
        }
    }

    pub(super) fn previous(&self) -> (Option<Arc<T>>, u64) {
        let previous = self
            .snapshot
            .as_ref()
            .map(|snapshot| Arc::clone(&snapshot.value));
        (previous, self.epoch)
    }

    pub(super) fn settle(
        &mut self,
        fetched: Result<T, Fault>,
        now: DateTime<Utc>,
        epoch: u64,
    ) -> Result<(Snapshot<T>, Freshness), Fault> {
        match fetched {
            Ok(value) => {
                let snapshot = Snapshot::new(value, now);
                self.snapshot = Some(snapshot.clone());
                self.failure = None;
                self.retry_after = None;
                self.expired = self.epoch != epoch;
                Ok((snapshot, Freshness::Refreshed))
            }
            Err(fault) => {
                self.retry_after = fault
                    .retry_after()
                    .map(|wait| now + delta(wait.unwrap_or(RATE_LIMIT_BACKOFF)));
                self.failure = Some(fault.clone());
                match &self.snapshot {
                    Some(snapshot) => {
                        Ok((snapshot.warned(Some(fault.to_string())), Freshness::Stale))
                    }
                    None => Err(fault),
                }
            }
        }
    }

    pub(super) fn expire(&mut self) {
        self.expired = true;
        self.epoch += 1;
    }

    pub(super) fn touch(&mut self, now: DateTime<Utc>) {
        self.used_at = Some(self.used_at.map_or(now, |used_at| used_at.max(now)));
    }

    pub(super) fn idle(&self, now: DateTime<Utc>, expiry: Duration) -> bool {
        self.used_at
            .is_none_or(|used_at| now.signed_duration_since(used_at) >= delta(expiry))
    }

    pub(super) fn vacant(&self, now: DateTime<Utc>) -> bool {
        self.snapshot.is_none() && !self.backing_off(now)
    }

    pub(super) fn peek(&self) -> Option<Snapshot<T>> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.warned(self.failure.as_ref().map(ToString::to_string)))
    }

    fn backing_off(&self, now: DateTime<Utc>) -> bool {
        self.retry_after
            .is_some_and(|retry_after| now < retry_after)
    }
}
