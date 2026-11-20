use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::audit::delivery::{S3_BATCH, S3_FLUSH_AGE, SYSLOG_BATCH, WEBHOOK_BATCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Webhook,
    S3,
    Syslog,
}

impl Kind {
    pub const ALL: [Self; 3] = [Self::Webhook, Self::S3, Self::Syslog];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
            Self::S3 => "s3",
            Self::Syslog => "syslog",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    pub fn batch_size(self) -> usize {
        match self {
            Self::Webhook => WEBHOOK_BATCH,
            Self::S3 => S3_BATCH,
            Self::Syslog => SYSLOG_BATCH,
        }
    }

    pub fn ready(self, pending: usize, oldest: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        if pending == 0 {
            return false;
        }
        match self {
            Self::Webhook | Self::Syslog => true,
            Self::S3 => {
                pending >= S3_BATCH || oldest.is_some_and(|oldest| now - oldest >= S3_FLUSH_AGE)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-17T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn ago(seconds: i64) -> Option<DateTime<Utc>> {
        Some(now() - TimeDelta::seconds(seconds))
    }

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = Kind::ALL
            .iter()
            .map(|kind| serde_json::to_string(kind).unwrap())
            .collect();
        assert_eq!(values, [r#""webhook""#, r#""s3""#, r#""syslog""#]);
        for kind in Kind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
            assert_eq!(
                serde_json::from_value::<Kind>(kind.as_str().into()).unwrap(),
                kind
            );
        }
    }

    #[test]
    fn parses_only_its_own_values() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        for value in ["", "Webhook", "S3", "http", "syslog "] {
            assert_eq!(Kind::parse(value), None, "{value:?}");
            assert!(
                serde_json::from_value::<Kind>(value.into()).is_err(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn batch_sizes_come_from_the_delivery_policy() {
        assert_eq!(Kind::Webhook.batch_size(), 100);
        assert_eq!(Kind::S3.batch_size(), 1000);
        assert_eq!(Kind::Syslog.batch_size(), 500);
    }

    #[test]
    fn nothing_pending_is_never_ready() {
        for kind in Kind::ALL {
            assert!(!kind.ready(0, None, now()), "{kind:?}");
            assert!(!kind.ready(0, ago(3600), now()), "{kind:?}");
        }
    }

    #[test]
    fn webhook_and_syslog_send_whatever_is_pending() {
        for kind in [Kind::Webhook, Kind::Syslog] {
            assert!(kind.ready(1, ago(0), now()), "{kind:?}");
            assert!(kind.ready(1, None, now()), "{kind:?}");
        }
    }

    #[test]
    fn s3_waits_for_a_full_batch_or_the_flush_age() {
        assert!(!Kind::S3.ready(1, ago(0), now()));
        assert!(!Kind::S3.ready(999, ago(59), now()));
        assert!(!Kind::S3.ready(999, None, now()));
        assert!(Kind::S3.ready(1000, ago(0), now()));
        assert!(Kind::S3.ready(1001, None, now()));
        assert!(Kind::S3.ready(1, ago(60), now()));
        assert!(Kind::S3.ready(1, ago(3600), now()));
    }

    #[test]
    fn s3_does_not_flush_an_event_from_the_future() {
        assert!(!Kind::S3.ready(1, ago(-3600), now()));
    }
}
