use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;

use super::super::client::RATE_LIMIT_EXHAUSTED;
use super::super::freshness::Freshness;
use super::super::snapshot::Snapshot;
use super::RATE_LIMIT_BACKOFF;
use super::decision::Decision;

pub(super) struct Entry<T> {
    snapshot: Option<Snapshot<T>>,
    failure: Option<String>,
    retry_after: Option<DateTime<Utc>>,
    expired: bool,
    epoch: u64,
}

impl<T> Default for Entry<T> {
    fn default() -> Self {
        Self {
            snapshot: None,
            failure: None,
            retry_after: None,
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
        if self
            .retry_after
            .is_some_and(|retry_after| now < retry_after)
        {
            return match &self.snapshot {
                Some(snapshot) => {
                    Decision::Serve(snapshot.warned(self.failure.clone()), Freshness::Stale)
                }
                None => Decision::Refuse(
                    self.failure
                        .clone()
                        .unwrap_or_else(|| RATE_LIMIT_EXHAUSTED.to_string()),
                ),
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
        fetched: Result<T, String>,
        now: DateTime<Utc>,
        epoch: u64,
    ) -> Result<(Snapshot<T>, Freshness), String> {
        match fetched {
            Ok(value) => {
                let snapshot = Snapshot::new(value, now);
                self.snapshot = Some(snapshot.clone());
                self.failure = None;
                self.retry_after = None;
                self.expired = self.epoch != epoch;
                Ok((snapshot, Freshness::Refreshed))
            }
            Err(error) => {
                if error == RATE_LIMIT_EXHAUSTED {
                    self.retry_after = Some(now + delta(RATE_LIMIT_BACKOFF));
                }
                self.failure = Some(error.clone());
                match &self.snapshot {
                    Some(snapshot) => Ok((snapshot.warned(Some(error)), Freshness::Stale)),
                    None => Err(error),
                }
            }
        }
    }

    pub(super) fn expire(&mut self) {
        self.expired = true;
        self.epoch += 1;
    }

    pub(super) fn peek(&self) -> Option<Snapshot<T>> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.warned(self.failure.clone()))
    }
}

fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}
