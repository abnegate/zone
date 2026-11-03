use uuid::Uuid;

use super::Provider;
use crate::db::ai_settings::EffectiveAiSettings;
use crate::db::chats::provider::Choice;
use crate::services::endpoint::{Endpoint, UrlError};
use crate::services::login::status::AgentStatus;
use crate::state::AppState;

/// Whether one chat can run on a provider, and why not when it can't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Usable,
    /// No key, URL or credentials are saved for an endpoint provider.
    Unconfigured,
    /// The organization holds no sign-in a coding agent can start on.
    SignedOut,
    /// The URL the provider would be sent to fails validation.
    Refused(UrlError),
    /// The provider runs only as a workspace's default, never for one chat.
    Unchoosable,
}

impl Availability {
    /// How `provider` stands for a chat of `organization` whose workspace has
    /// `settings`. An agent's sign-in is judged from its stored logins.
    pub async fn of(
        state: &AppState,
        organization: Uuid,
        settings: &EffectiveAiSettings,
        provider: Provider,
    ) -> Result<Self, sqlx::Error> {
        if !provider.choosable() {
            return Ok(Self::Unchoosable);
        }
        if let Some(agent) = provider.agent() {
            let signed_in = AgentStatus::signed_in(state, organization, agent).await?;
            return Ok(if signed_in {
                Self::Usable
            } else {
                Self::SignedOut
            });
        }
        let chosen = settings.choosing(&Choice {
            provider,
            model: None,
        });
        if !provider.configured(&chosen) {
            return Ok(Self::Unconfigured);
        }
        Ok(match Endpoint::try_resolve(state.config(), &chosen) {
            Ok(_) => Self::Usable,
            Err(error) => Self::Refused(error),
        })
    }

    pub fn usable(self) -> bool {
        self == Self::Usable
    }

    /// Why `provider` can't be chosen, in words for the person choosing.
    pub fn reason(self, provider: Provider) -> Option<String> {
        let label = provider.label();
        match self {
            Self::Usable => None,
            Self::Unconfigured => Some(format!(
                "No key, endpoint or credentials are saved for {label} in AI Settings."
            )),
            Self::SignedOut => Some(format!(
                "{label} is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings."
            )),
            Self::Refused(error) => {
                Some(format!("{label}'s saved endpoint can't be used: {error}"))
            }
            Self::Unchoosable => Some(format!(
                "{label} runs on this instance like {} and can't be chosen for a single chat.",
                Provider::SelfHosted.label()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_secret::SecretValue;
    use zone_context::embeddings::providers::{PROVIDER_BEDROCK, PROVIDER_SELF_HOSTED};

    use super::*;
    use crate::services::endpoint::testing::settings;
    use crate::services::provider::testing::Fixture;

    const METADATA_URL: &str = "http://169.254.169.254/v1";

    async fn of(
        fixture: &Fixture,
        settings: &EffectiveAiSettings,
        provider: Provider,
    ) -> Availability {
        Availability::of(&fixture.state, fixture.organization, settings, provider)
            .await
            .expect("the sign-ins are read")
    }

    #[tokio::test]
    async fn an_endpoint_provider_is_usable_on_a_saved_key_or_url_and_nothing_else() {
        let fixture = Fixture::new().await;
        let mut key = settings(PROVIDER_SELF_HOSTED);
        key.openai_api_key = Some(SecretValue::new("sk-openai"));
        let mut url = settings(PROVIDER_SELF_HOSTED);
        url.anthropic_base_url = Some("https://anthropic-gateway.example".to_string());
        let neither = settings(PROVIDER_SELF_HOSTED);

        assert_eq!(
            of(&fixture, &key, Provider::OpenAI).await,
            Availability::Usable
        );
        assert_eq!(
            of(&fixture, &url, Provider::Anthropic).await,
            Availability::Usable
        );
        assert_eq!(
            of(&fixture, &neither, Provider::OpenAI).await,
            Availability::Unconfigured
        );
        assert_eq!(
            of(&fixture, &key, Provider::Anthropic).await,
            Availability::Unconfigured,
            "OpenAI's key configures only OpenAI"
        );
        assert_eq!(
            of(&fixture, &neither, Provider::SelfHosted).await,
            Availability::Usable,
            "Self-Hosted runs on the instance with nothing saved"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn a_link_local_endpoint_is_refused() {
        let fixture = Fixture::new().await;
        let mut refused = settings(PROVIDER_SELF_HOSTED);
        refused.openai_api_key = Some(SecretValue::new("sk-openai"));
        refused.openai_base_url = Some(METADATA_URL.to_string());
        refused.litellm_host = Some(METADATA_URL.to_string());

        assert_eq!(
            of(&fixture, &refused, Provider::OpenAI).await,
            Availability::Refused(UrlError::Metadata)
        );
        assert_eq!(
            of(&fixture, &refused, Provider::SelfHosted).await,
            Availability::Refused(UrlError::Metadata)
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn bedrock_is_never_choosable_however_it_is_configured() {
        let fixture = Fixture::new().await;
        let mut role = settings(PROVIDER_BEDROCK);
        role.bedrock_use_iam_role = true;

        for configured in [role, settings(PROVIDER_BEDROCK)] {
            assert_eq!(
                of(&fixture, &configured, Provider::Bedrock).await,
                Availability::Unchoosable
            );
        }
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn an_agent_is_usable_only_once_the_organization_signs_it_in() {
        let fixture = Fixture::new().await;
        let empty = settings(PROVIDER_SELF_HOSTED);

        for provider in [Provider::ClaudeCode, Provider::Codex] {
            assert_eq!(
                of(&fixture, &empty, provider).await,
                Availability::SignedOut,
                "{provider:?}"
            );
        }

        fixture.sign_in_claude().await;
        assert_eq!(
            of(&fixture, &empty, Provider::ClaudeCode).await,
            Availability::Usable
        );
        assert_eq!(
            of(&fixture, &empty, Provider::Codex).await,
            Availability::SignedOut,
            "a Claude login doesn't sign in Codex"
        );
        fixture.cleanup().await;
    }

    #[test]
    fn each_reason_is_one_sentence_naming_its_provider() {
        assert_eq!(Availability::Usable.reason(Provider::OpenAI), None);
        assert_eq!(
            Availability::Unconfigured
                .reason(Provider::Anthropic)
                .as_deref(),
            Some("No key, endpoint or credentials are saved for Anthropic in AI Settings.")
        );
        assert_eq!(
            Availability::SignedOut
                .reason(Provider::ClaudeCode)
                .as_deref(),
            Some(
                "Claude Code is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings."
            )
        );
        assert_eq!(
            Availability::Refused(UrlError::Metadata)
                .reason(Provider::OpenAI)
                .as_deref(),
            Some(
                "OpenAI's saved endpoint can't be used: The URL must not point at a link-local or cloud metadata address."
            )
        );
        assert_eq!(
            Availability::Unchoosable
                .reason(Provider::Bedrock)
                .as_deref(),
            Some(
                "AWS Bedrock runs on this instance like Self-Hosted and can't be chosen for a single chat."
            )
        );

        let refusals = [
            UrlError::Unparseable,
            UrlError::Scheme,
            UrlError::Host,
            UrlError::Userinfo,
            UrlError::Query,
            UrlError::Fragment,
            UrlError::Metadata,
            UrlError::Unlisted,
        ]
        .map(Availability::Refused);
        let unusable = [
            Availability::Unconfigured,
            Availability::SignedOut,
            Availability::Unchoosable,
        ]
        .into_iter()
        .chain(refusals);
        for availability in unusable {
            for provider in Provider::ALL {
                let reason = availability.reason(provider).expect("a reason");
                assert!(
                    reason.ends_with('.') && !reason.ends_with(".."),
                    "{reason:?} ends on one period"
                );
            }
        }
    }
}
