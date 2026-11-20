use serde::{Deserialize, Deserializer};

use crate::policy::agent::Agent;
use crate::policy::field::Field;
use crate::policy::provider::Provider;
use crate::policy::tier::Tier;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    #[serde(default, deserialize_with = "present")]
    pub providers: Option<Option<Vec<Provider>>>,
    #[serde(default, deserialize_with = "present")]
    pub agents: Option<Option<Vec<Agent>>>,
    #[serde(default, deserialize_with = "present")]
    pub models: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "present")]
    pub ceiling: Option<Option<Tier>>,
    #[serde(default, deserialize_with = "present")]
    pub mcp_servers: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "present")]
    pub domains: Option<Option<Vec<String>>>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.fields().is_empty()
    }

    pub fn fields(&self) -> Vec<Field> {
        [
            (Field::Providers, self.providers.is_some()),
            (Field::Agents, self.agents.is_some()),
            (Field::Models, self.models.is_some()),
            (Field::Ceiling, self.ceiling.is_some()),
            (Field::McpServers, self.mcp_servers.is_some()),
            (Field::Domains, self.domains.is_some()),
        ]
        .into_iter()
        .filter_map(|(field, present)| present.then_some(field))
        .collect()
    }
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> Patch {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn an_empty_body_changes_nothing() {
        let patch = parse("{}");
        assert!(patch.is_empty());
        assert!(patch.fields().is_empty());
        assert_eq!(patch.providers, None);
        assert_eq!(patch.ceiling, None);
    }

    #[test]
    fn null_clears_and_a_value_replaces() {
        let patch = parse(r#"{"providers": null, "ceiling": "host", "models": ["claude-*"]}"#);
        assert!(!patch.is_empty());
        assert_eq!(patch.providers, Some(None));
        assert_eq!(patch.ceiling, Some(Some(Tier::Host)));
        assert_eq!(patch.models, Some(Some(vec!["claude-*".to_owned()])));
        assert_eq!(patch.agents, None);
        assert_eq!(
            patch.fields(),
            [Field::Providers, Field::Models, Field::Ceiling]
        );
    }

    #[test]
    fn lists_every_present_key_in_field_order() {
        let patch = parse(
            r#"{"domains": [], "mcp_servers": null, "ceiling": null, "models": [], "agents": [], "providers": []}"#,
        );
        assert_eq!(patch.fields(), Field::ALL);
    }

    #[test]
    fn refuses_unknown_keys_and_values() {
        for body in [
            r#"{"tier": "host"}"#,
            r#"{"providers": ["gemini"]}"#,
            r#"{"agents": "claude"}"#,
            r#"{"ceiling": "none"}"#,
        ] {
            assert!(serde_json::from_str::<Patch>(body).is_err(), "{body}");
        }
    }
}
