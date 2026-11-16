use chrono::{DateTime, Utc};

use crate::edition::Edition;
use crate::license::License;
use crate::status::{Status, status};

pub const LICENSE_REQUIRED: &str = "license_required";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Sso,
    Policy,
}

impl Feature {
    pub fn editions(self) -> &'static [Edition] {
        match self {
            Self::Sso | Self::Policy => &[Edition::Team, Edition::Enterprise],
        }
    }

    pub fn enforced(self, license: Option<&License>) -> bool {
        self.editions().contains(&Edition::from(license))
    }

    pub fn entitled(self, license: Option<&License>, now: DateTime<Utc>) -> bool {
        self.enforced(license) && matches!(status(license, now), Status::Active | Status::Grace)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures;
    use crate::payload::Payload;
    use crate::tier::Tier;

    fn license(tier: Tier) -> License {
        fixtures::license(&Payload {
            tier,
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
    fn community_is_never_entitled() {
        let reference = license(Tier::Team);
        for (now, _) in moments(&reference) {
            assert!(!Feature::Sso.entitled(None, now), "community at {now}");
        }
    }

    #[test]
    fn team_and_enterprise_are_entitled_while_active_or_in_grace() {
        for tier in [Tier::Team, Tier::Enterprise] {
            let license = license(tier);
            for (now, expected) in moments(&license) {
                assert_eq!(license.status(now), expected);
                assert_eq!(
                    Feature::Sso.entitled(Some(&license), now),
                    expected != Status::Lapsed,
                    "{tier:?} while {expected:?}"
                );
            }
        }
    }

    #[test]
    fn single_sign_on_needs_team_or_enterprise() {
        assert_eq!(
            Feature::Sso.editions(),
            [Edition::Team, Edition::Enterprise]
        );
    }

    #[test]
    fn agent_policy_needs_team_or_enterprise() {
        assert_eq!(
            Feature::Policy.editions(),
            [Edition::Team, Edition::Enterprise]
        );
    }

    #[test]
    fn agent_policy_is_editable_only_while_active_or_in_grace() {
        for tier in [Tier::Team, Tier::Enterprise] {
            let license = license(tier);
            for (now, expected) in moments(&license) {
                assert_eq!(
                    Feature::Policy.entitled(Some(&license), now),
                    matches!(expected, Status::Active | Status::Grace),
                    "{tier:?} while {expected:?}"
                );
            }
        }
        for (now, _) in moments(&license(Tier::Team)) {
            assert!(!Feature::Policy.entitled(None, now), "community at {now}");
        }
    }

    #[test]
    fn agent_policy_is_enforced_whatever_the_status() {
        for tier in [Tier::Team, Tier::Enterprise] {
            let license = license(tier);
            for (now, status) in moments(&license) {
                assert_eq!(license.status(now), status);
                assert!(
                    Feature::Policy.enforced(Some(&license)),
                    "{tier:?} while {status:?}"
                );
            }
        }
    }

    #[test]
    fn community_enforces_nothing() {
        assert!(!Feature::Policy.enforced(None));
        assert!(!Feature::Sso.enforced(None));
    }

    #[test]
    fn code_is_the_wire_contract() {
        assert_eq!(LICENSE_REQUIRED, "license_required");
    }
}
