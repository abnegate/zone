use serde::{Deserialize, Serialize};

use crate::license::License;
use crate::tier::Tier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Community,
    Team,
    Enterprise,
}

impl From<Tier> for Edition {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Team => Self::Team,
            Tier::Enterprise => Self::Enterprise,
        }
    }
}

impl From<Option<&License>> for Edition {
    fn from(license: Option<&License>) -> Self {
        license.map_or(Self::Community, |license| license.tier().into())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures;
    use crate::payload::Payload;
    use crate::status::Status;

    #[test]
    fn no_license_is_community() {
        assert_eq!(Edition::from(None), Edition::Community);
    }

    #[test]
    fn a_license_gives_its_tier_whatever_its_status() {
        for (tier, edition) in [
            (Tier::Team, Edition::Team),
            (Tier::Enterprise, Edition::Enterprise),
        ] {
            let license = fixtures::license(&Payload {
                tier,
                ..fixtures::payload()
            });
            let moments = [
                (license.issued_at(), Status::Active),
                (license.expires_at(), Status::Grace),
                (license.grace_ends_at() + TimeDelta::days(1), Status::Lapsed),
            ];
            for (now, status) in moments {
                assert_eq!(license.status(now), status);
                assert_eq!(
                    Edition::from(Some(&license)),
                    edition,
                    "{tier:?} while {status:?}"
                );
            }
        }
    }

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = [Edition::Community, Edition::Team, Edition::Enterprise]
            .iter()
            .map(|edition| serde_json::to_string(edition).unwrap())
            .collect();
        assert_eq!(values, [r#""community""#, r#""team""#, r#""enterprise""#]);
    }
}
