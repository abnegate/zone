use chrono::{DateTime, Utc};

use crate::license::License;
use crate::plan::Plan;
use crate::status::{Status, status};

pub const LICENSE_REQUIRED: &str = "license_required";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Sso,
    Policy,
    AuditExport,
    AuditStreaming,
    AuditRetention,
    CustomRoles,
    Spend,
    /// Uncapped membership: without it a server admits at most the Community member cap.
    Members,
}

impl Feature {
    pub const ALL: [Self; 8] = [
        Self::Sso,
        Self::Policy,
        Self::AuditExport,
        Self::AuditStreaming,
        Self::AuditRetention,
        Self::CustomRoles,
        Self::Spend,
        Self::Members,
    ];

    pub fn plan(self) -> Plan {
        match self {
            Self::Spend | Self::Members => Plan::Team,
            Self::Sso
            | Self::Policy
            | Self::AuditExport
            | Self::AuditStreaming
            | Self::AuditRetention
            | Self::CustomRoles => Plan::Enterprise,
        }
    }

    pub fn enforced(self, license: Option<&License>) -> bool {
        license.is_some_and(|license| license.plan().includes(self))
    }

    pub fn entitled(self, license: Option<&License>, now: DateTime<Utc>) -> bool {
        self.enforced(license) && matches!(status(license, now), Status::Active | Status::Grace)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures;
    use crate::payload::Payload;

    fn license(plan: Plan) -> License {
        fixtures::license(&Payload {
            plan: Some(plan),
            ..fixtures::payload()
        })
    }

    fn moments(license: &License) -> [(DateTime<Utc>, Status); 3] {
        [
            (license.issued_at(), Status::Active),
            (license.expires_at(), Status::Grace),
            (license.grace_ends_at() + TimeDelta::days(1), Status::Lapsed),
        ]
    }

    fn team_feature(feature: Feature) -> bool {
        matches!(feature, Feature::Spend | Feature::Members)
    }

    #[test]
    fn every_feature_is_listed_once() {
        let listed: HashSet<Feature> = Feature::ALL.into_iter().collect();
        assert_eq!(listed.len(), Feature::ALL.len());
        assert!(listed.contains(&Feature::CustomRoles));
    }

    #[test]
    fn community_is_never_entitled() {
        for feature in Feature::ALL {
            for (now, _) in moments(&license(Plan::Enterprise)) {
                assert!(!feature.entitled(None, now), "{feature:?} at {now}");
            }
        }
    }

    #[test]
    fn community_enforces_nothing() {
        for feature in Feature::ALL {
            assert!(!feature.enforced(None), "{feature:?}");
        }
    }

    #[test]
    fn spend_and_members_need_team_and_the_rest_need_enterprise() {
        for feature in Feature::ALL {
            let expected = if team_feature(feature) {
                Plan::Team
            } else {
                Plan::Enterprise
            };
            assert_eq!(feature.plan(), expected, "{feature:?}");
        }
    }

    #[test]
    fn enterprise_is_entitled_to_every_feature_while_active_or_in_grace() {
        let license = license(Plan::Enterprise);
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
    fn team_is_entitled_to_spend_and_members_only_while_active_or_in_grace() {
        let license = license(Plan::Team);
        for feature in Feature::ALL {
            for (now, status) in moments(&license) {
                assert_eq!(license.status(now), status);
                assert_eq!(
                    feature.entitled(Some(&license), now),
                    team_feature(feature) && matches!(status, Status::Active | Status::Grace),
                    "{feature:?} while {status:?}"
                );
            }
        }
    }

    #[test]
    fn a_license_without_a_plan_is_entitled_as_enterprise() {
        let license = fixtures::license(&fixtures::payload());
        for feature in Feature::ALL {
            assert!(
                feature.entitled(Some(&license), license.issued_at()),
                "{feature:?}"
            );
        }
    }

    #[test]
    fn nothing_is_entitled_after_a_lapse() {
        for plan in Plan::ALL {
            let license = license(plan);
            let lapsed = license.grace_ends_at() + TimeDelta::days(1);
            for feature in Feature::ALL {
                assert!(
                    !feature.entitled(Some(&license), lapsed),
                    "{plan:?} {feature:?}"
                );
            }
        }
    }

    #[test]
    fn enterprise_enforces_every_feature_whatever_its_status() {
        for feature in Feature::ALL {
            assert!(
                feature.enforced(Some(&license(Plan::Enterprise))),
                "{feature:?}"
            );
        }
    }

    #[test]
    fn team_enforces_only_its_own_features() {
        for feature in Feature::ALL {
            assert_eq!(
                feature.enforced(Some(&license(Plan::Team))),
                team_feature(feature),
                "{feature:?}"
            );
        }
    }

    #[test]
    fn code_is_the_wire_contract() {
        assert_eq!(LICENSE_REQUIRED, "license_required");
    }
}
