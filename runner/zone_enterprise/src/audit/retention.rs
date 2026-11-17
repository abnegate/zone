use std::fmt;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use super::standing::Pause;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wait {
    Backlog,
    Failing,
    Paused,
}

impl Wait {
    pub const ALL: [Self; 3] = [Self::Backlog, Self::Failing, Self::Paused];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Failing => "failing",
            Self::Paused => "paused",
        }
    }
}

impl fmt::Display for Wait {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub fn wait(pause: Option<Pause>, attempts: u32) -> Wait {
    if pause.is_some() {
        return Wait::Paused;
    }
    if attempts > 0 {
        return Wait::Failing;
    }
    Wait::Backlog
}

pub fn cutoff(now: DateTime<Utc>, days: u32) -> DateTime<Utc> {
    TimeDelta::try_days(i64::from(days))
        .and_then(|period| now.checked_sub_signed(period))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 17, 12, 30, 0).unwrap()
    }

    #[test]
    fn a_paused_holder_is_paused_whatever_its_attempts() {
        for pause in Pause::ALL {
            assert_eq!(wait(Some(pause), 0), Wait::Paused);
            assert_eq!(wait(Some(pause), 2), Wait::Paused);
        }
    }

    #[test]
    fn a_retrying_holder_is_failing() {
        assert_eq!(wait(None, 1), Wait::Failing);
    }

    #[test]
    fn a_holder_without_attempts_is_working_through_its_backlog() {
        assert_eq!(wait(None, 0), Wait::Backlog);
    }

    #[test]
    fn the_cutoff_is_the_period_before_now() {
        assert_eq!(
            cutoff(now(), 30),
            Utc.with_ymd_and_hms(2026, 10, 18, 12, 30, 0).unwrap()
        );
        assert_eq!(cutoff(now(), 0), now());
    }

    #[test]
    fn a_period_past_the_earliest_date_saturates() {
        assert_eq!(cutoff(now(), u32::MAX), DateTime::<Utc>::MIN_UTC);
        assert_eq!(
            cutoff(DateTime::<Utc>::MIN_UTC, 1),
            DateTime::<Utc>::MIN_UTC
        );
    }

    #[test]
    fn waits_are_the_wire_contract() {
        let values: Vec<&str> = Wait::ALL.iter().map(|wait| wait.as_str()).collect();
        assert_eq!(values, ["backlog", "failing", "paused"]);
        for wait in Wait::ALL {
            assert_eq!(serde_json::to_value(wait).unwrap(), wait.as_str());
            assert_eq!(wait.to_string(), wait.as_str());
        }
    }
}
