use std::fmt;

use serde::{Deserialize, Serialize};

use crate::entitlement::Feature;
use crate::license::License;
use crate::plan::Plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Community,
    Team,
    Enterprise,
}

impl Edition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Community => "community",
            Self::Team => "team",
            Self::Enterprise => "enterprise",
        }
    }

    pub fn includes(self, feature: Feature) -> bool {
        match self {
            Self::Community => false,
            Self::Team => Plan::Team.includes(feature),
            Self::Enterprise => Plan::Enterprise.includes(feature),
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<Plan> for Edition {
    fn from(plan: Plan) -> Self {
        match plan {
            Plan::Team => Self::Team,
            Plan::Enterprise => Self::Enterprise,
        }
    }
}

impl From<Option<&License>> for Edition {
    fn from(license: Option<&License>) -> Self {
        license.map_or(Self::Community, |license| license.plan().into())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};

    use super::*;
    use crate::fixtures;
    use crate::payload::Payload;
    use crate::status::Status;

    const EDITIONS: [Edition; 3] = [Edition::Community, Edition::Team, Edition::Enterprise];

    fn license(plan: Option<Plan>) -> License {
        fixtures::license(&Payload {
            plan,
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

    #[test]
    fn no_license_is_community() {
        assert_eq!(Edition::from(None), Edition::Community);
    }

    #[test]
    fn each_plan_is_its_edition() {
        assert_eq!(Edition::from(Plan::Team), Edition::Team);
        assert_eq!(Edition::from(Plan::Enterprise), Edition::Enterprise);
    }

    #[test]
    fn a_license_is_its_plan_whatever_its_status() {
        for (plan, edition) in [
            (Some(Plan::Team), Edition::Team),
            (Some(Plan::Enterprise), Edition::Enterprise),
            (None, Edition::Enterprise),
        ] {
            let license = license(plan);
            for (now, status) in moments(&license) {
                assert_eq!(license.status(now), status);
                assert_eq!(
                    Edition::from(Some(&license)),
                    edition,
                    "{plan:?} while {status:?}"
                );
            }
        }
    }

    #[test]
    fn community_includes_nothing() {
        for feature in Feature::ALL {
            assert!(!Edition::Community.includes(feature), "{feature:?}");
        }
    }

    #[test]
    fn team_includes_spend_and_members_only() {
        for feature in Feature::ALL {
            assert_eq!(
                Edition::Team.includes(feature),
                matches!(feature, Feature::Spend | Feature::Members),
                "{feature:?}"
            );
        }
    }

    #[test]
    fn enterprise_includes_every_feature() {
        for feature in Feature::ALL {
            assert!(Edition::Enterprise.includes(feature), "{feature:?}");
        }
    }

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = EDITIONS
            .iter()
            .map(|edition| serde_json::to_string(edition).unwrap())
            .collect();
        assert_eq!(values, [r#""community""#, r#""team""#, r#""enterprise""#]);
    }

    #[test]
    fn displays_its_wire_value() {
        for edition in EDITIONS {
            assert_eq!(
                serde_json::to_value(edition).unwrap(),
                edition.to_string(),
                "{edition:?}"
            );
        }
    }

    #[test]
    fn decodes_the_team_edition() {
        assert_eq!(
            serde_json::from_str::<Edition>(r#""team""#).unwrap(),
            Edition::Team
        );
    }

    #[test]
    fn refuses_an_unknown_edition() {
        for value in [r#""Enterprise""#, r#""Team""#, r#""business""#, r#""""#] {
            assert!(
                serde_json::from_str::<Edition>(value).is_err(),
                "accepted {value}"
            );
        }
    }
}
