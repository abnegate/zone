use abnegate_secret::SecretValue;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DestinationChanges {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub settings: Option<Value>,
    pub secrets: Option<SecretValue>,
}

impl DestinationChanges {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.enabled.is_none()
            && self.settings.is_none()
            && self.secrets.is_none()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_the_default_is_empty() {
        assert!(DestinationChanges::default().is_empty());
        let changes = [
            DestinationChanges {
                name: Some("Splunk".into()),
                ..Default::default()
            },
            DestinationChanges {
                enabled: Some(false),
                ..Default::default()
            },
            DestinationChanges {
                settings: Some(json!({"url": "https://siem.example.com/hook"})),
                ..Default::default()
            },
            DestinationChanges {
                secrets: Some(SecretValue::new("{\"signing_secret\":\"rotated\"}")),
                ..Default::default()
            },
        ];
        for change in changes {
            assert!(!change.is_empty(), "{change:?}");
        }
    }

    #[test]
    fn debug_never_shows_the_secrets() {
        let changes = DestinationChanges {
            secrets: Some(SecretValue::new("whsec-plaintext")),
            ..Default::default()
        };
        assert!(!format!("{changes:?}").contains("whsec-plaintext"));
    }
}
