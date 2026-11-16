use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::license::License;

pub const GRACE_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Community,
    Active,
    Grace,
    Lapsed,
}

pub(crate) const fn grace() -> TimeDelta {
    TimeDelta::days(GRACE_DAYS)
}

pub fn status(license: Option<&License>, now: DateTime<Utc>) -> Status {
    match license {
        None => Status::Community,
        Some(license) if now < license.expires_at() => Status::Active,
        Some(license) if now < license.grace_ends_at() => Status::Grace,
        Some(_) => Status::Lapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn second() -> TimeDelta {
        TimeDelta::seconds(1)
    }

    #[test]
    fn no_license_is_community() {
        assert_eq!(
            status(None, fixtures::payload().issued_at),
            Status::Community
        );
    }

    #[test]
    fn active_until_the_last_second_before_expiry() {
        let license = fixtures::license(&fixtures::payload());
        assert_eq!(status(Some(&license), license.issued_at()), Status::Active);
        assert_eq!(
            status(Some(&license), license.expires_at() - second()),
            Status::Active
        );
    }

    #[test]
    fn grace_from_the_instant_of_expiry() {
        let license = fixtures::license(&fixtures::payload());
        assert_eq!(status(Some(&license), license.expires_at()), Status::Grace);
    }

    #[test]
    fn grace_until_the_last_second_of_thirty_days() {
        let license = fixtures::license(&fixtures::payload());
        let last = license.expires_at() + TimeDelta::days(30) - second();
        assert_eq!(status(Some(&license), last), Status::Grace);
    }

    #[test]
    fn lapsed_thirty_days_after_expiry() {
        let license = fixtures::license(&fixtures::payload());
        let end = license.expires_at() + TimeDelta::days(30);
        assert_eq!(status(Some(&license), end), Status::Lapsed);
        assert_eq!(
            status(Some(&license), end + TimeDelta::days(365)),
            Status::Lapsed
        );
    }

    #[test]
    fn active_before_the_issue_date() {
        let license = fixtures::license(&fixtures::payload());
        assert_eq!(
            status(Some(&license), license.issued_at() - TimeDelta::days(1)),
            Status::Active
        );
    }

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = [
            Status::Community,
            Status::Active,
            Status::Grace,
            Status::Lapsed,
        ]
        .iter()
        .map(|status| serde_json::to_string(status).unwrap())
        .collect();
        assert_eq!(
            values,
            [r#""community""#, r#""active""#, r#""grace""#, r#""lapsed""#]
        );
    }
}
