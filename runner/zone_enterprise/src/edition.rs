use std::fmt;

use serde::{Deserialize, Serialize};

use crate::license::License;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Community,
    Enterprise,
}

impl Edition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Community => "community",
            Self::Enterprise => "enterprise",
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<Option<&License>> for Edition {
    fn from(license: Option<&License>) -> Self {
        license.map_or(Self::Community, |_| Self::Enterprise)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures;
    use crate::status::Status;

    #[test]
    fn no_license_is_community() {
        assert_eq!(Edition::from(None), Edition::Community);
    }

    #[test]
    fn a_license_is_enterprise_whatever_its_status() {
        let license = fixtures::license(&fixtures::payload());
        let moments = [
            (license.issued_at(), Status::Active),
            (license.expires_at(), Status::Grace),
            (license.grace_ends_at() + TimeDelta::days(1), Status::Lapsed),
        ];
        for (now, status) in moments {
            assert_eq!(license.status(now), status);
            assert_eq!(
                Edition::from(Some(&license)),
                Edition::Enterprise,
                "while {status:?}"
            );
        }
    }

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = [Edition::Community, Edition::Enterprise]
            .iter()
            .map(|edition| serde_json::to_string(edition).unwrap())
            .collect();
        assert_eq!(values, [r#""community""#, r#""enterprise""#]);
    }

    #[test]
    fn displays_its_wire_value() {
        for edition in [Edition::Community, Edition::Enterprise] {
            assert_eq!(
                serde_json::to_value(edition).unwrap(),
                edition.to_string(),
                "{edition:?}"
            );
        }
    }

    #[test]
    fn refuses_the_retired_team_edition() {
        assert!(serde_json::from_str::<Edition>(r#""team""#).is_err());
    }

    #[test]
    fn refuses_an_unknown_edition() {
        for value in [r#""Enterprise""#, r#""business""#, r#""""#] {
            assert!(
                serde_json::from_str::<Edition>(value).is_err(),
                "accepted {value}"
            );
        }
    }
}
