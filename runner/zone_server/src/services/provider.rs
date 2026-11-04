//! The AI providers a workspace, or one chat, can run its completions on.

mod availability;
mod offer;
mod offered;
mod refusal;
#[cfg(test)]
mod testing;

pub use availability::Availability;
pub use offer::{Offer, choose, offers};
pub use offered::Offered;
pub use refusal::Refusal;

use abnegate_secret::SecretValue;
use serde::{Deserialize, Serialize};
use zone_context::embeddings::providers::{
    PROVIDER_BEDROCK, PROVIDER_OPENAI, PROVIDER_SELF_HOSTED,
};
use zone_core::llm::AgentKind;

use crate::db::ai_settings::{
    EffectiveAiSettings, PROVIDER_ANTHROPIC, PROVIDER_CLAUDE_CODE, PROVIDER_CODEX,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    SelfHosted,
    #[serde(rename = "openai")]
    OpenAI,
    Anthropic,
    Bedrock,
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: [Self; 6] = [
        Self::SelfHosted,
        Self::OpenAI,
        Self::Anthropic,
        Self::Bedrock,
        Self::ClaudeCode,
        Self::Codex,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelfHosted => PROVIDER_SELF_HOSTED,
            Self::OpenAI => PROVIDER_OPENAI,
            Self::Anthropic => PROVIDER_ANTHROPIC,
            Self::Bedrock => PROVIDER_BEDROCK,
            Self::ClaudeCode => PROVIDER_CLAUDE_CODE,
            Self::Codex => PROVIDER_CODEX,
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SelfHosted => "Self-Hosted",
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Bedrock => "AWS Bedrock",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub fn of(agent: AgentKind) -> Self {
        match agent {
            AgentKind::Claude => Self::ClaudeCode,
            AgentKind::Codex => Self::Codex,
        }
    }

    pub fn agent(self) -> Option<AgentKind> {
        match self {
            Self::ClaudeCode => Some(AgentKind::Claude),
            Self::Codex => Some(AgentKind::Codex),
            _ => None,
        }
    }

    /// Bedrock runs on the instance exactly as Self-Hosted does, so choosing it
    /// for one chat would change nothing.
    pub fn choosable(self) -> bool {
        self != Self::Bedrock
    }

    /// Whether `settings` hold what this provider needs to be reached. The
    /// instance stands in for an unsaved Self-Hosted host, and an agent signs in
    /// through its logins rather than settings.
    pub fn configured(self, settings: &EffectiveAiSettings) -> bool {
        match self {
            Self::SelfHosted | Self::ClaudeCode | Self::Codex => true,
            Self::OpenAI => {
                saved_key(settings.openai_api_key.as_ref())
                    || saved(settings.openai_base_url.as_deref())
            }
            Self::Anthropic => {
                saved_key(settings.anthropic_api_key.as_ref())
                    || saved(settings.anthropic_base_url.as_deref())
            }
            Self::Bedrock => {
                settings.bedrock_use_iam_role
                    || (saved_key(settings.bedrock_access_key.as_ref())
                        && saved_key(settings.bedrock_secret_key.as_ref()))
            }
        }
    }
}

fn saved(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn saved_key(key: Option<&SecretValue>) -> bool {
    saved(key.map(SecretValue::expose))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ai_settings::PROVIDERS;
    use crate::services::endpoint::testing::settings;

    #[test]
    fn each_provider_is_named_by_its_settings_constant() {
        assert_eq!(Provider::ALL.map(Provider::as_str), PROVIDERS);
        for provider in Provider::ALL {
            assert_eq!(Provider::named(provider.as_str()), Some(provider));
        }
        for name in ["", "gemini", "OpenAI", "claude", " openai"] {
            assert_eq!(Provider::named(name), None, "{name:?} names no provider");
        }
    }

    #[test]
    fn serde_spells_each_provider_as_its_settings_constant() {
        for provider in Provider::ALL {
            let json = serde_json::to_value(provider).expect("a provider serializes");
            assert_eq!(json, serde_json::Value::from(provider.as_str()));
            assert_eq!(
                serde_json::from_value::<Provider>(json).expect("a provider deserializes"),
                provider
            );
        }
        assert_eq!(
            serde_json::to_string(&Provider::OpenAI).unwrap(),
            "\"openai\""
        );
        assert!(serde_json::from_str::<Provider>("\"open_a_i\"").is_err());
        assert!(serde_json::from_str::<Provider>("\"gemini\"").is_err());
    }

    #[test]
    fn only_bedrock_cannot_be_chosen_for_a_chat() {
        for provider in Provider::ALL {
            assert_eq!(provider.choosable(), provider != Provider::Bedrock);
        }
    }

    #[test]
    fn only_the_coding_agents_name_an_agent() {
        for agent in AgentKind::ALL {
            assert_eq!(Provider::of(agent).agent(), Some(agent));
        }
        assert_eq!(Provider::ClaudeCode.agent(), Some(AgentKind::Claude));
        assert_eq!(Provider::Codex.agent(), Some(AgentKind::Codex));
        for provider in [
            Provider::SelfHosted,
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Bedrock,
        ] {
            assert_eq!(provider.agent(), None);
        }
    }

    #[test]
    fn each_provider_has_a_label() {
        assert_eq!(
            Provider::ALL.map(Provider::label),
            [
                "Self-Hosted",
                "OpenAI",
                "Anthropic",
                "AWS Bedrock",
                "Claude Code",
                "Codex"
            ]
        );
    }

    #[test]
    fn nothing_saved_configures_only_the_providers_that_need_no_settings() {
        let empty = settings(PROVIDER_SELF_HOSTED);
        let configured: Vec<Provider> = Provider::ALL
            .into_iter()
            .filter(|provider| provider.configured(&empty))
            .collect();
        assert_eq!(
            configured,
            [Provider::SelfHosted, Provider::ClaudeCode, Provider::Codex]
        );
    }

    #[test]
    fn a_saved_key_or_url_configures_its_endpoint_provider() {
        let mut key = settings(PROVIDER_SELF_HOSTED);
        key.openai_api_key = Some(SecretValue::new("sk-openai"));
        assert!(Provider::OpenAI.configured(&key));
        assert!(!Provider::Anthropic.configured(&key));

        let mut url = settings(PROVIDER_SELF_HOSTED);
        url.openai_base_url = Some("https://openai.example.com/v1".to_string());
        assert!(Provider::OpenAI.configured(&url));

        let mut key = settings(PROVIDER_SELF_HOSTED);
        key.anthropic_api_key = Some(SecretValue::new("sk-ant"));
        assert!(Provider::Anthropic.configured(&key));
        assert!(!Provider::OpenAI.configured(&key));

        let mut url = settings(PROVIDER_SELF_HOSTED);
        url.anthropic_base_url = Some("https://anthropic.example.com".to_string());
        assert!(Provider::Anthropic.configured(&url));
    }

    #[test]
    fn a_blank_key_or_url_configures_nothing() {
        let mut blank = settings(PROVIDER_OPENAI);
        blank.openai_api_key = Some(SecretValue::new("  "));
        blank.openai_base_url = Some(String::new());
        blank.anthropic_api_key = Some(SecretValue::new(""));
        blank.anthropic_base_url = Some(" ".to_string());
        blank.bedrock_access_key = Some(SecretValue::new(" "));
        blank.bedrock_secret_key = Some(SecretValue::new("secret"));
        assert!(!Provider::OpenAI.configured(&blank));
        assert!(!Provider::Anthropic.configured(&blank));
        assert!(!Provider::Bedrock.configured(&blank));
    }

    #[test]
    fn bedrock_needs_an_iam_role_or_both_access_keys() {
        let mut role = settings(PROVIDER_BEDROCK);
        role.bedrock_use_iam_role = true;
        assert!(Provider::Bedrock.configured(&role));

        let mut access = settings(PROVIDER_BEDROCK);
        access.bedrock_access_key = Some(SecretValue::new("AKIATEST"));
        assert!(!Provider::Bedrock.configured(&access));
        access.bedrock_secret_key = Some(SecretValue::new("secret"));
        assert!(Provider::Bedrock.configured(&access));
    }
}
