//! One login as a status reads it, with what ranks it against the agent's other logins.

use std::cmp::Ordering;

use chrono::{DateTime, Utc};

use super::login::LoginStatus;
use super::state::State;

pub(super) struct Reading {
    pub(super) login: LoginStatus,
    /// Until when the login cannot take a turn: the later of when it was marked exhausted until
    /// and when its last snapshot says its spent windows reset.
    pub(super) spent_until: Option<DateTime<Utc>>,
}

impl Reading {
    /// How it ranks against `other` at `now`, greater being the login a turn would rather start on:
    /// signed in before expired, then not at its usage limit, then the most headroom, a known
    /// headroom before an unknown one, then the one used last, then by label.
    pub(super) fn compare(&self, other: &Self, now: DateTime<Utc>) -> Ordering {
        self.signed_in()
            .cmp(&other.signed_in())
            .then_with(|| self.available(now).cmp(&other.available(now)))
            .then_with(|| match (self.headroom(), other.headroom()) {
                (Some(mine), Some(theirs)) => mine.total_cmp(&theirs),
                (mine, theirs) => mine.is_some().cmp(&theirs.is_some()),
            })
            .then_with(|| self.login.last_used_at.cmp(&other.login.last_used_at))
            .then_with(|| other.login.label.cmp(&self.login.label))
    }

    fn signed_in(&self) -> bool {
        self.login.state == State::SignedIn
    }

    fn available(&self, now: DateTime<Utc>) -> bool {
        self.spent_until.is_none_or(|until| until <= now)
    }

    fn headroom(&self) -> Option<f64> {
        self.login.usage.as_ref().and_then(|usage| usage.headroom)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use uuid::Uuid;

    use super::super::usage::UsageStatus;
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("a valid timestamp")
    }

    fn reading(label: &str, state: State, headroom: Option<f64>) -> Reading {
        Reading {
            login: LoginStatus {
                id: Uuid::new_v4(),
                label: Some(label.to_string()),
                plan: None,
                state,
                expires_at: None,
                usage: headroom.map(|headroom| UsageStatus {
                    windows: vec![],
                    headroom: Some(headroom),
                    fetched_at: at(1_790_000_000),
                    exhausted_until: None,
                }),
                last_used_at: None,
            },
            spent_until: None,
        }
    }

    fn best(readings: &[Reading], now: DateTime<Utc>) -> Option<&str> {
        readings
            .iter()
            .max_by(|reading, other| reading.compare(other, now))
            .and_then(|reading| reading.login.label.as_deref())
    }

    #[test]
    fn the_signed_in_login_with_the_most_headroom_ranks_first() {
        let now = at(1_790_000_000);
        let readings = [
            reading("expired", State::Expired, Some(90.0)),
            reading("little", State::SignedIn, Some(10.0)),
            reading("unknown", State::SignedIn, None),
            reading("most", State::SignedIn, Some(60.0)),
        ];

        assert_eq!(best(&readings, now), Some("most"));
        assert_eq!(best(&readings[..3], now), Some("little"));
        assert_eq!(
            best(&readings[2..3], now),
            Some("unknown"),
            "a login with no snapshot yet is still one a turn can start on"
        );
    }

    #[test]
    fn a_login_at_its_usage_limit_ranks_after_one_that_is_not_until_it_resets() {
        let now = at(1_790_000_000);
        let mut spent = reading("spent", State::SignedIn, Some(95.0));
        spent.spent_until = Some(now + TimeDelta::hours(2));
        let readings = [spent, reading("fresh", State::SignedIn, Some(5.0))];

        assert_eq!(best(&readings, now), Some("fresh"));
        assert_eq!(
            best(&readings, now + TimeDelta::hours(3)),
            Some("spent"),
            "a login whose limit has reset still ranked as spent"
        );
    }

    #[test]
    fn of_equal_logins_the_one_used_last_then_the_first_label_ranks_first() {
        let now = at(1_790_000_000);
        let mut used = reading("used", State::SignedIn, Some(50.0));
        used.login.last_used_at = Some(now - TimeDelta::minutes(5));
        let readings = [
            reading("bea", State::SignedIn, Some(50.0)),
            used,
            reading("ada", State::SignedIn, Some(50.0)),
        ];

        assert_eq!(best(&readings, now), Some("used"));
        let unused = [
            reading("bea", State::SignedIn, Some(50.0)),
            reading("ada", State::SignedIn, Some(50.0)),
        ];
        assert_eq!(best(&unused, now), Some("ada"));
    }
}
