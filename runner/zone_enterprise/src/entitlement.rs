use chrono::{DateTime, Utc};

use crate::edition::Edition;
use crate::license::License;
use crate::status::{Status, status};

pub const LICENSE_REQUIRED: &str = "license_required";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Sso,
}

impl Feature {
    pub fn editions(self) -> &'static [Edition] {
        match self {
            Self::Sso => &[Edition::Team, Edition::Enterprise],
        }
    }

    pub fn entitled(self, license: Option<&License>, now: DateTime<Utc>) -> bool {
        self.editions().contains(&Edition::from(license))
            && matches!(status(license, now), Status::Active | Status::Grace)
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
    fn code_is_the_wire_contract() {
        assert_eq!(LICENSE_REQUIRED, "license_required");
    }
}
