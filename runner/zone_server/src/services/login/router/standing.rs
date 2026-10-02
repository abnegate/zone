use std::cmp::Ordering;

use chrono::{DateTime, Utc};

use crate::db::agent_logins::AgentLoginRow;
use crate::services::login::usage::{Availability, Snapshot};

/// How usable a login is for a new session, judged by its usage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Standing {
    /// Its most used window has `headroom` percent left.
    Available { headroom: f64 },
    /// Its usage has not been read, or what was read has since reset.
    Unknown,
    /// It has reached a limit, until `until` when that is known.
    Exhausted { until: Option<DateTime<Utc>> },
}

impl Standing {
    pub(super) fn of(
        login: &AgentLoginRow,
        snapshot: Option<&Snapshot>,
        now: DateTime<Utc>,
    ) -> Self {
        if let Some(until) = marked(login, now) {
            return Self::Exhausted { until: Some(until) };
        }
        let Some(snapshot) = snapshot else {
            return Self::Unknown;
        };
        match snapshot.headroom {
            Some(headroom) if headroom > 0.0 => Self::Available { headroom },
            Some(_) => match snapshot.availability() {
                Availability::At(until) if until > now => Self::Exhausted { until: Some(until) },
                Availability::Unknown => Self::Exhausted { until: None },
                Availability::At(_) | Availability::Now => Self::Unknown,
            },
            None => Self::Unknown,
        }
    }

    pub(super) fn until(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Exhausted { until } => until,
            Self::Available { .. } | Self::Unknown => None,
        }
    }

    pub(super) fn usable(self) -> bool {
        !matches!(self, Self::Exhausted { .. })
    }

    /// Most headroom first, then unknown usage, then exhausted.
    pub(super) fn rank(self, other: Self) -> Ordering {
        match (self, other) {
            (Self::Available { headroom }, Self::Available { headroom: other }) => {
                other.total_cmp(&headroom)
            }
            (left, right) => left.class().cmp(&right.class()),
        }
    }

    fn class(self) -> u8 {
        match self {
            Self::Available { .. } => 0,
            Self::Unknown => 1,
            Self::Exhausted { .. } => 2,
        }
    }
}

/// When a limit some turn on the login hit resets, while it has not.
pub(super) fn marked(login: &AgentLoginRow, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    login.exhausted_until.filter(|until| *until > now)
}
