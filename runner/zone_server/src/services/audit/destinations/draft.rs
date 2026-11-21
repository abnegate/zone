use std::fmt;

use abnegate_secret::REDACTED;
use serde_json::Value;

#[derive(Clone, PartialEq)]
pub struct Draft {
    pub name: String,
    pub enabled: bool,
    pub settings: Value,
    pub secrets: Value,
}

impl fmt::Debug for Draft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Draft")
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("settings", &self.settings)
            .field("secrets", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn debug_never_shows_the_secrets() {
        let draft = Draft {
            name: "Splunk".into(),
            enabled: true,
            settings: json!({"kind": "webhook", "url": "https://siem.example.com/hook"}),
            secrets: json!({"signing_secret": "whsec-plaintext"}),
        };
        let shown = format!("{draft:?}");
        assert!(!shown.contains("whsec-plaintext"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
