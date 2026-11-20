use chrono::{DateTime, Utc};

use crate::license::License;
use crate::status::{Status, status};

pub const LICENSE_REQUIRED: &str = "license_required";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Sso,
    Policy,
    AuditExport,
    AuditStreaming,
    AuditRetention,
}

impl Feature {
    pub const ALL: [Self; 5] = [
        Self::Sso,
        Self::Policy,
        Self::AuditExport,
        Self::AuditStreaming,
        Self::AuditRetention,
    ];

    pub fn enforced(license: Option<&License>) -> bool {
        license.is_some()
    }

    pub fn entitled(self, license: Option<&License>, now: DateTime<Utc>) -> bool {
        Self::enforced(license) && matches!(status(license, now), Status::Active | Status::Grace)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures;

    fn license() -> License {
        fixtures::license(&fixtures::payload())
    }

    fn moments(license: &License) -> [(DateTime<Utc>, Status); 3] {
        [
            (license.issued_at(), Status::Active),
            (license.expires_at(), Status::Grace),
            (license.grace_ends_at() + TimeDelta::days(1), Status::Lapsed),
        ]
    }

    #[test]
    fn community_is_never_entitled() {
        for feature in Feature::ALL {
            for (now, _) in moments(&license()) {
                assert!(!feature.entitled(None, now), "{feature:?} at {now}");
            }
        }
    }

    #[test]
    fn community_enforces_nothing() {
        assert!(!Feature::enforced(None));
    }

    #[test]
    fn a_license_is_entitled_to_every_feature_while_active_or_in_grace() {
        let license = license();
        for feature in Feature::ALL {
            for (now, status) in moments(&license) {
                assert_eq!(license.status(now), status);
                assert_eq!(
                    feature.entitled(Some(&license), now),
                    matches!(status, Status::Active | Status::Grace),
                    "{feature:?} while {status:?}"
                );
            }
        }
    }

    #[test]
    fn a_license_enforces_every_feature_whatever_its_status() {
        assert!(Feature::enforced(Some(&license())));
    }

    #[test]
    fn code_is_the_wire_contract() {
        assert_eq!(LICENSE_REQUIRED, "license_required");
    }
}
