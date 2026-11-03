use abnegate_llm::ReasoningEffort;
use futures::future::try_join_all;
use serde::Serialize;
use uuid::Uuid;
use zone_core::llm::AgentKind;

use super::{Availability, Offered, Provider, Refusal};
use crate::config::Config;
use crate::db::ai_settings::EffectiveAiSettings;
use crate::db::chats::provider::Choice;
use crate::services::endpoint::{Endpoint, Origin};
use crate::services::model::Model;
use crate::services::stages::{self, Catalog, Preferences};
use crate::state::AppState;

/// A provider as one chat may choose it: whether it can run the chat, the
/// models and efforts it offers, and whether the chat must name a model. It
/// never carries a key, secret or URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offer {
    pub provider: Provider,
    pub label: &'static str,
    pub usable: bool,
    pub reason: Option<String>,
    pub models: Vec<Offered>,
    pub efforts: &'static [ReasoningEffort],
    pub requires_model: bool,
}

impl Offer {
    fn new(
        provider: Provider,
        availability: Availability,
        reached: Option<&(EffectiveAiSettings, Endpoint)>,
        catalog: &Catalog,
    ) -> Self {
        let (models, efforts, requires_model) = match (provider.agent(), reached) {
            (Some(agent), _) => (agent_models(agent), agent.efforts(None), false),
            (None, Some((settings, endpoint))) if endpoint.origin() == Origin::Settings => {
                let saved = saved(settings, endpoint);
                let requires_model = saved.is_empty();
                (
                    saved.into_iter().map(http_model).collect(),
                    ReasoningEffort::HTTP,
                    requires_model,
                )
            }
            (None, Some(_)) => (
                catalog
                    .completions()
                    .map(|installed| http_model(installed.name.clone()))
                    .collect(),
                ReasoningEffort::HTTP,
                false,
            ),
            (None, None) => (Vec::new(), ReasoningEffort::HTTP, false),
        };
        Self {
            provider,
            label: provider.label(),
            usable: availability.usable(),
            reason: availability.reason(provider),
            models,
            efforts,
            requires_model,
        }
    }
}

/// Every provider, in [`Provider::ALL`] order, as a chat of `organization`
/// whose workspace has `settings` may choose it. Each agent's sign-in and the
/// instance's installed models are read once.
pub async fn offers(
    state: &AppState,
    organization: Uuid,
    settings: &EffectiveAiSettings,
) -> Result<Vec<Offer>, sqlx::Error> {
    let availabilities = try_join_all(
        Provider::ALL.map(|provider| Availability::of(state, organization, settings, provider)),
    )
    .await?;
    let reached: Vec<_> = Provider::ALL
        .into_iter()
        .zip(&availabilities)
        .map(|(provider, availability)| reach(state.config(), settings, provider, *availability))
        .collect();
    let catalog = if reached
        .iter()
        .flatten()
        .any(|(_, endpoint)| endpoint.origin() == Origin::Instance)
    {
        Catalog::load(&state.config().ollama_host).await
    } else {
        Catalog::default()
    };
    Ok(Provider::ALL
        .into_iter()
        .zip(availabilities)
        .zip(&reached)
        .map(|((provider, availability), reached)| {
            Offer::new(provider, availability, reached.as_ref(), &catalog)
        })
        .collect())
}

/// `provider` and `model` checked as one chat's choice, in order: the
/// provider is named, the model isn't blank, the provider can run the chat,
/// and the model is one it runs: one an agent offers, one the settings save
/// for an endpoint they name, which a chat must name when they save none, or
/// one the instance doesn't list as unable to chat. [`stages::AUTO`] leaves
/// the model to the provider.
pub async fn choose(
    state: &AppState,
    organization: Uuid,
    settings: &EffectiveAiSettings,
    provider: &str,
    model: Option<&str>,
) -> Result<Choice, Refusal> {
    let provider = Provider::named(provider).ok_or(Refusal::Unnamed)?;
    let model = match model.map(str::trim) {
        Some("") => return Err(Refusal::Blank),
        model => model.filter(|model| !stages::is_auto(model)),
    };
    let availability = Availability::of(state, organization, settings, provider)
        .await
        .map_err(|error| {
            tracing::error!(%organization, %error, "Could not read the agent's sign-ins");
            Refusal::Unreadable
        })?;
    if !availability.usable() {
        return Err(Refusal::Unavailable(provider, availability));
    }
    match provider.agent() {
        Some(agent) => {
            if model.is_some_and(|model| !agent.models().contains(&model)) {
                return Err(Refusal::Unoffered(provider));
            }
        }
        None => runs(state.config(), settings, provider, model).await?,
    }
    Ok(Choice {
        provider,
        model: model.map(str::to_string),
    })
}

/// Whether the endpoint `provider` sends to runs `model`, or its own choice
/// when `model` is `None`. An endpoint the settings name runs only the models
/// they save, and must be named one when they save none. The instance runs
/// any model but one its catalog lists as unable to chat.
async fn runs(
    config: &Config,
    settings: &EffectiveAiSettings,
    provider: Provider,
    model: Option<&str>,
) -> Result<(), Refusal> {
    let chosen = settings.choosing(&Choice {
        provider,
        model: None,
    });
    let Ok(endpoint) = Endpoint::try_resolve(config, &chosen) else {
        return Ok(());
    };
    match (endpoint.origin(), model) {
        (Origin::Settings, model) => {
            let saved = Preferences::for_endpoint(&chosen, "", &endpoint);
            match model {
                None if saved.completions().is_empty() => Err(Refusal::Unset(provider)),
                Some(model) if !saved.completions().is_empty() && !saved.admits(model) => {
                    Err(Refusal::Unoffered(provider))
                }
                _ => Ok(()),
            }
        }
        (Origin::Instance, Some(model))
            if Model::completion(&config.ollama_host, model).await == Some(false) =>
        {
            Err(Refusal::Unoffered(provider))
        }
        (Origin::Instance, _) => Ok(()),
    }
}

/// The settings a chat choosing `provider` runs on and the endpoint they
/// send it to, for a provider whose models are listed from its endpoint:
/// one that can run a chat, or Bedrock, which runs as a workspace's default.
fn reach(
    config: &Config,
    settings: &EffectiveAiSettings,
    provider: Provider,
    availability: Availability,
) -> Option<(EffectiveAiSettings, Endpoint)> {
    let listed = matches!(
        availability,
        Availability::Usable | Availability::Unchoosable
    );
    if provider.agent().is_some() || !listed {
        return None;
    }
    let chosen = settings.choosing(&Choice {
        provider,
        model: None,
    });
    let endpoint = Endpoint::try_resolve(config, &chosen).ok()?;
    Some((chosen, endpoint))
}

/// The Fast and Reasoning models `settings` save for `endpoint`.
fn saved(settings: &EffectiveAiSettings, endpoint: &Endpoint) -> Vec<String> {
    Preferences::for_endpoint(settings, "", endpoint)
        .completions()
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn agent_models(agent: AgentKind) -> Vec<Offered> {
    agent
        .models()
        .iter()
        .map(|name| Offered {
            name: (*name).to_string(),
            label: agent.label(name),
            efforts: agent.efforts(Some(name)),
        })
        .collect()
}

fn http_model(name: String) -> Offered {
    Offered {
        label: name.clone(),
        name,
        efforts: ReasoningEffort::HTTP,
    }
}

#[cfg(test)]
mod tests {
    use abnegate_secret::SecretValue;
    use serde_json::json;
    use zone_context::embeddings::providers::{
        PROVIDER_BEDROCK, PROVIDER_OPENAI, PROVIDER_SELF_HOSTED,
    };

    use super::*;
    use crate::services::endpoint::testing::settings;
    use crate::services::provider::testing::Fixture;

    const KEY: &str = "sk-offer-the-tenants-own-key";
    const HTTP: [&str; 4] = ["off", "low", "medium", "high"];
    const CLAUDE: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
    const CODEX: [&str; 4] = ["low", "medium", "high", "xhigh"];

    async fn listed(fixture: &Fixture, settings: &EffectiveAiSettings) -> Vec<Offer> {
        offers(&fixture.state, fixture.organization, settings)
            .await
            .expect("the offers are read")
    }

    fn of(offers: &[Offer], provider: Provider) -> &Offer {
        offers
            .iter()
            .find(|offer| offer.provider == provider)
            .expect("every provider is offered")
    }

    fn names(offer: &Offer) -> Vec<&str> {
        offer
            .models
            .iter()
            .map(|model| model.name.as_str())
            .collect()
    }

    async fn chosen(
        fixture: &Fixture,
        settings: &EffectiveAiSettings,
        provider: &str,
        model: Option<&str>,
    ) -> Result<Choice, Refusal> {
        choose(
            &fixture.state,
            fixture.organization,
            settings,
            provider,
            model,
        )
        .await
    }

    fn openai_key(provider: &str) -> EffectiveAiSettings {
        let mut settings = settings(provider);
        settings.openai_api_key = Some(SecretValue::new(KEY));
        settings
    }

    #[tokio::test]
    async fn every_provider_is_offered_in_order_under_its_label() {
        let fixture = Fixture::new().await;
        let offers = listed(&fixture, &settings(PROVIDER_SELF_HOSTED)).await;

        assert_eq!(
            offers
                .iter()
                .map(|offer| offer.provider)
                .collect::<Vec<_>>(),
            Provider::ALL
        );
        for offer in &offers {
            assert_eq!(offer.label, offer.provider.label());
            assert_eq!(offer.usable, offer.reason.is_none(), "{offer:?}");
        }
        let usable: Vec<Provider> = offers
            .iter()
            .filter(|offer| offer.usable)
            .map(|offer| offer.provider)
            .collect();
        assert_eq!(usable, [Provider::SelfHosted]);
        let bedrock = of(&offers, Provider::Bedrock);
        assert_eq!(
            bedrock.reason,
            Availability::Unchoosable.reason(Provider::Bedrock)
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn agents_offer_their_own_models_with_each_models_efforts() {
        let fixture = Fixture::new().await;
        let offers = listed(&fixture, &settings(PROVIDER_SELF_HOSTED)).await;

        let claude = serde_json::to_value(of(&offers, Provider::ClaudeCode)).expect("serialise");
        assert_eq!(
            claude,
            json!({
                "provider": "claude_code",
                "label": "Claude Code",
                "usable": false,
                "reason": Availability::SignedOut.reason(Provider::ClaudeCode),
                "models": [
                    { "name": "sonnet", "label": "Sonnet", "efforts": CLAUDE },
                    { "name": "opus", "label": "Opus", "efforts": CLAUDE },
                    { "name": "haiku", "label": "Haiku", "efforts": [] },
                    { "name": "fable", "label": "Fable", "efforts": CLAUDE },
                ],
                "efforts": CLAUDE,
                "requires_model": false,
            })
        );

        let codex = of(&offers, Provider::Codex);
        assert_eq!(names(codex), AgentKind::Codex.models());
        for model in &codex.models {
            assert_eq!(
                model.label, model.name,
                "codex shows its models as it names them"
            );
            assert_eq!(model.efforts, AgentKind::Codex.efforts(Some(&model.name)));
        }
        assert_eq!(
            serde_json::to_value(codex.efforts).expect("serialise"),
            json!(CODEX),
            "Automatic offers only what every codex model takes"
        );
        assert!(!codex.requires_model);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn endpoint_providers_offer_their_saved_models_with_the_http_efforts() {
        let fixture = Fixture::new().await;
        let mut saved = openai_key(PROVIDER_OPENAI);
        saved.model_fast = Some("gpt-4o-mini".to_string());
        saved.model_reasoning = Some("gpt-4o".to_string());
        let offers = listed(&fixture, &saved).await;

        let openai = of(&offers, Provider::OpenAI);
        assert!(openai.usable);
        assert_eq!(names(openai), ["gpt-4o-mini", "gpt-4o"]);
        assert!(!openai.requires_model);
        let serialised = serde_json::to_value(openai).expect("serialise");
        assert_eq!(serialised["efforts"], json!(HTTP));
        for model in serialised["models"].as_array().expect("models") {
            assert_eq!(model["efforts"], json!(HTTP), "{model}");
            assert_eq!(model["label"], model["name"]);
        }
        assert!(
            !serialised.to_string().contains(KEY),
            "an offer carries the saved key: {serialised}"
        );

        let self_hosted = of(&offers, Provider::SelfHosted);
        assert_eq!(
            serde_json::to_value(self_hosted.efforts).expect("serialise"),
            json!(HTTP)
        );
        assert!(
            self_hosted.models.is_empty(),
            "OpenAI's saved models are not Self-Hosted's, and no Ollama answers"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn only_an_endpoint_the_settings_name_with_no_saved_model_requires_one() {
        let fixture = Fixture::new().await;
        let instance = fixture.state.config().litellm_host.clone();
        let mut own_host = settings(PROVIDER_SELF_HOSTED);
        own_host.litellm_host = Some("http://ollama-gateway.example:4000".to_string());
        let mut instance_host = settings(PROVIDER_SELF_HOSTED);
        instance_host.litellm_host = Some(instance);
        let mut saved_model = openai_key(PROVIDER_OPENAI);
        saved_model.model_fast = Some("gpt-4o".to_string());

        for (settings, provider, requires) in [
            (openai_key(PROVIDER_SELF_HOSTED), Provider::OpenAI, true),
            (openai_key(PROVIDER_OPENAI), Provider::OpenAI, true),
            (saved_model.clone(), Provider::OpenAI, false),
            (saved_model, Provider::Anthropic, false),
            (own_host, Provider::SelfHosted, true),
            (instance_host, Provider::SelfHosted, false),
            (settings(PROVIDER_SELF_HOSTED), Provider::SelfHosted, false),
            (settings(PROVIDER_SELF_HOSTED), Provider::ClaudeCode, false),
            (settings(PROVIDER_SELF_HOSTED), Provider::Codex, false),
            (settings(PROVIDER_BEDROCK), Provider::Bedrock, false),
        ] {
            assert_eq!(
                matches!(
                    chosen(&fixture, &settings, provider.as_str(), None).await,
                    Err(Refusal::Unset(_))
                ),
                requires,
                "{provider:?} on {}",
                settings.provider
            );
            assert_eq!(
                of(&listed(&fixture, &settings).await, provider).requires_model,
                requires,
                "{provider:?} offered on {}",
                settings.provider
            );
        }
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn a_usable_provider_is_chosen_with_or_without_a_model() {
        let fixture = Fixture::new().await;
        fixture.sign_in_claude().await;
        let mut saved = openai_key(PROVIDER_OPENAI);
        saved.model_fast = Some("gpt-4o".to_string());

        for (settings, provider, model, expected) in [
            (
                openai_key(PROVIDER_SELF_HOSTED),
                "openai",
                Some(" gpt-4o "),
                Choice {
                    provider: Provider::OpenAI,
                    model: Some("gpt-4o".to_string()),
                },
            ),
            (
                saved,
                "openai",
                None,
                Choice {
                    provider: Provider::OpenAI,
                    model: None,
                },
            ),
            (
                settings(PROVIDER_SELF_HOSTED),
                "self_hosted",
                None,
                Choice {
                    provider: Provider::SelfHosted,
                    model: None,
                },
            ),
            (
                settings(PROVIDER_SELF_HOSTED),
                "claude_code",
                Some("opus"),
                Choice {
                    provider: Provider::ClaudeCode,
                    model: Some("opus".to_string()),
                },
            ),
            (
                settings(PROVIDER_SELF_HOSTED),
                "claude_code",
                Some("auto"),
                Choice {
                    provider: Provider::ClaudeCode,
                    model: None,
                },
            ),
        ] {
            assert_eq!(
                chosen(&fixture, &settings, provider, model).await,
                Ok(expected),
                "{provider} with {model:?}"
            );
        }
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn an_endpoint_provider_runs_only_a_model_its_settings_save() {
        let fixture = Fixture::new().await;
        let mut saved = openai_key(PROVIDER_OPENAI);
        saved.model_fast = Some("gpt-4o-mini".to_string());

        for model in ["o1-pro", "gpt-4o", "gpt-4o-mini-2024"] {
            assert_eq!(
                chosen(&fixture, &saved, "openai", Some(model)).await,
                Err(Refusal::Unoffered(Provider::OpenAI)),
                "{model} is not saved, so the workspace's key must not run it"
            );
        }
        for model in [None, Some("gpt-4o-mini"), Some(" gpt-4o-mini ")] {
            assert_eq!(
                chosen(&fixture, &saved, "openai", model).await,
                Ok(Choice {
                    provider: Provider::OpenAI,
                    model: model.map(|model| model.trim().to_string()),
                }),
                "{model:?} is what the settings save"
            );
        }
        assert_eq!(
            chosen(
                &fixture,
                &openai_key(PROVIDER_OPENAI),
                "openai",
                Some("o1-pro")
            )
            .await,
            Ok(Choice {
                provider: Provider::OpenAI,
                model: Some("o1-pro".to_string()),
            }),
            "settings that save no model leave the chat to name one"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn each_refused_choice_says_why() {
        let fixture = Fixture::new().await;
        let mut bedrock = settings(PROVIDER_BEDROCK);
        bedrock.bedrock_use_iam_role = true;
        let keyed = openai_key(PROVIDER_SELF_HOSTED);

        let refusals = [
            (&keyed, "gemini", None, Refusal::Unnamed),
            (&keyed, "OpenAI", Some("gpt-4o"), Refusal::Unnamed),
            (&keyed, "openai", Some("   "), Refusal::Blank),
            (&keyed, "openai", Some(""), Refusal::Blank),
            (
                &keyed,
                "anthropic",
                Some("claude-sonnet-4-5"),
                Refusal::Unavailable(Provider::Anthropic, Availability::Unconfigured),
            ),
            (
                &keyed,
                "claude_code",
                None,
                Refusal::Unavailable(Provider::ClaudeCode, Availability::SignedOut),
            ),
            (
                &keyed,
                "codex",
                Some("gpt-6-sol"),
                Refusal::Unavailable(Provider::Codex, Availability::SignedOut),
            ),
            (
                &bedrock,
                "bedrock",
                None,
                Refusal::Unavailable(Provider::Bedrock, Availability::Unchoosable),
            ),
            (&keyed, "openai", None, Refusal::Unset(Provider::OpenAI)),
            (
                &keyed,
                "openai",
                Some("auto"),
                Refusal::Unset(Provider::OpenAI),
            ),
        ];
        for (settings, provider, model, refusal) in refusals {
            assert_eq!(
                chosen(&fixture, settings, provider, model).await,
                Err(refusal),
                "{provider} with {model:?}"
            );
        }

        fixture.sign_in_claude().await;
        fixture.sign_in_codex().await;
        for (provider, model) in [
            (Provider::ClaudeCode, "gpt-4o"),
            (Provider::ClaudeCode, "qwen3:8b"),
            (Provider::ClaudeCode, "claude sonnet"),
            (Provider::ClaudeCode, "claude-opus-4-8"),
            (Provider::ClaudeCode, "sonnet[1m]"),
            (Provider::ClaudeCode, "Opus"),
            (Provider::Codex, "gpt-4o"),
            (Provider::Codex, "gpt-7"),
            (Provider::Codex, "opus"),
        ] {
            assert_eq!(
                chosen(&fixture, &keyed, provider.as_str(), Some(model)).await,
                Err(Refusal::Unoffered(provider)),
                "{provider:?} with {model}"
            );
        }
        for agent in AgentKind::ALL {
            let provider = Provider::ALL
                .into_iter()
                .find(|provider| provider.agent() == Some(agent))
                .expect("every agent is a provider");
            for model in agent.models() {
                assert_eq!(
                    chosen(&fixture, &keyed, provider.as_str(), Some(model)).await,
                    Ok(Choice {
                        provider,
                        model: Some((*model).to_string()),
                    }),
                    "{agent} offers {model}"
                );
            }
        }
        fixture.cleanup().await;
    }
}
