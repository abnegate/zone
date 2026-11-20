use std::fmt;

use abnegate_secret::REDACTED;
use serde_json::Value;

/// A sparse change: `None` keeps what is stored. `settings` replaces the kind's settings and
/// `secrets` merges into the stored secrets, an empty string clearing an optional one.
#[derive(Clone, Default, PartialEq)]
pub struct Revision {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub settings: Option<Value>,
    pub secrets: Option<Value>,
}

impl Revision {
    pub fn only_disables(&self) -> bool {
        *self
            == Self {
                enabled: Some(false),
                ..Self::default()
            }
    }
}

impl fmt::Debug for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Revision")
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("settings", &self.settings)
            .field("secrets", &self.secrets.as_ref().map(|_| REDACTED))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_a_bare_disable_only_disables() {
        let disable = Revision {
            enabled: Some(false),
            ..Revision::default()
        };
        assert!(disable.only_disables());
        let revisions = [
            Revision::default(),
            Revision {
                enabled: Some(true),
                ..Revision::default()
            },
            Revision {
                name: Some("Splunk".into()),
                ..disable.clone()
            },
            Revision {
                settings: Some(json!({"kind": "webhook", "url": "http://10.0.0.5/hook"})),
                ..disable.clone()
            },
            Revision {
                secrets: Some(json!({"signing_secret": "rotated"})),
                ..disable.clone()
            },
        ];
        for revision in revisions {
            assert!(!revision.only_disables(), "{revision:?}");
        }
    }

    #[test]
    fn debug_never_shows_the_secrets() {
        let revision = Revision {
            secrets: Some(json!({"signing_secret": "whsec-plaintext"})),
            ..Revision::default()
        };
        let shown = format!("{revision:?}");
        assert!(!shown.contains("whsec-plaintext"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
