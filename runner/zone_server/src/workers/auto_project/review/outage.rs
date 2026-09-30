//! A reviewer endpoint that stops answering, followed across ticks.
//!
//! A restarting model server fails a review without judging anything, so such
//! a tick records no round and asks the same reviewer again. The streak is
//! bounded: once it has lasted [`WINDOW`] over at least [`ATTEMPTS`] ticks the
//! task waits for a person instead of retrying unseen.

use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use uuid::Uuid;

/// Unanswered attempts a streak needs before it can pause a task.
pub const ATTEMPTS: u32 = 5;
/// How long a streak has to last before it can pause a task. A gap this long
/// between attempts starts a new streak, since nobody saw what the endpoint did
/// in between.
pub const WINDOW: TimeDelta = TimeDelta::minutes(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outage {
    pub attempts: u32,
    pub since: DateTime<Utc>,
    pub last: DateTime<Utc>,
}

impl Outage {
    /// The streak `previous` becomes with one more unanswered attempt at `now`.
    pub fn extend(previous: Option<Self>, now: DateTime<Utc>) -> Self {
        match previous {
            Some(previous) if now - previous.last < WINDOW => Self {
                attempts: previous.attempts + 1,
                since: previous.since,
                last: now,
            },
            _ => Self {
                attempts: 1,
                since: now,
                last: now,
            },
        }
    }

    /// Whether the endpoint has been silent long enough, and often enough,
    /// that another tick is unlikely to reach it.
    pub fn outlasted(&self) -> bool {
        self.attempts >= ATTEMPTS && self.last - self.since >= WINDOW
    }

    /// Why the task waits for a person, in the words of the last failure.
    pub fn reason(&self, reviewer: &str, failure: &str) -> String {
        format!(
            "the reviewer model {reviewer} could not be reached on {} attempts over {} minutes; \
             the last: {failure}. Check that the model server is running",
            self.attempts,
            (self.last - self.since).num_minutes()
        )
    }
}

/// Each task's current streak, kept for the life of the driver.
#[derive(Debug, Default)]
pub struct Outages(DashMap<Uuid, Outage>);

impl Outages {
    /// Count an unanswered attempt at reviewing `task` and return the streak.
    pub fn record(&self, task: Uuid, now: DateTime<Utc>) -> Outage {
        match self.0.entry(task) {
            Entry::Occupied(mut current) => {
                let streak = Outage::extend(Some(*current.get()), now);
                current.insert(streak);
                streak
            }
            Entry::Vacant(vacant) => *vacant.insert(Outage::extend(None, now)),
        }
    }

    /// End `task`'s streak: the endpoint answered, or a person was told.
    pub fn clear(&self, task: Uuid) {
        self.0.remove(&task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::UNIX_EPOCH + TimeDelta::minutes(minutes)
    }

    #[test]
    fn a_streak_pauses_only_after_enough_attempts_over_the_whole_window() {
        let outages = Outages::default();
        let task = Uuid::new_v4();

        let quick: Vec<bool> = (0..=i64::from(ATTEMPTS))
            .map(|minute| outages.record(task, at(minute)).outlasted())
            .collect();
        assert!(
            quick.iter().all(|outlasted| !outlasted),
            "a server restarting for a few ticks is retried: {quick:?}"
        );

        let mut streak = outages.record(task, at(9));
        assert!(!streak.outlasted(), "{streak:?}");
        streak = outages.record(task, at(10));
        assert!(streak.outlasted(), "{streak:?}");
        assert_eq!(streak.attempts, ATTEMPTS + 3);
        assert_eq!(streak.since, at(0));
    }

    #[test]
    fn two_sparse_attempts_across_the_window_do_not_pause() {
        let outages = Outages::default();
        let task = Uuid::new_v4();

        outages.record(task, at(0));
        let streak = outages.record(task, at(9));

        assert_eq!(streak.attempts, 2);
        assert!(
            !Outage {
                last: at(10),
                ..streak
            }
            .outlasted()
        );
    }

    #[test]
    fn a_gap_as_long_as_the_window_or_an_answer_starts_the_streak_over() {
        let outages = Outages::default();
        let task = Uuid::new_v4();
        for minute in 0..4 {
            outages.record(task, at(minute));
        }

        let after_gap = outages.record(task, at(3) + WINDOW);
        assert_eq!(after_gap.attempts, 1);
        assert_eq!(after_gap.since, at(3) + WINDOW);

        outages.clear(task);
        assert_eq!(outages.record(task, at(20)).attempts, 1);
    }

    #[test]
    fn each_task_keeps_its_own_streak() {
        let outages = Outages::default();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());

        outages.record(first, at(0));
        outages.record(first, at(1));

        assert_eq!(outages.record(second, at(1)).attempts, 1);
    }

    #[test]
    fn the_pause_names_the_reviewer_the_streak_and_the_last_failure() {
        let streak = Outage {
            attempts: 7,
            since: at(0),
            last: at(12),
        };

        let reason = streak.reason("qwen3:32b", "connection refused");

        assert_eq!(
            reason,
            "the reviewer model qwen3:32b could not be reached on 7 attempts over 12 minutes; the \
             last: connection refused. Check that the model server is running"
        );
    }
}
