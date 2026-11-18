//! The agent and model policy an organization, and each of its workspaces, holds its agents to,
//! enforced while an Enterprise license is installed, lapsed or not.

pub mod auditor;
pub mod background;
pub mod denied;
pub mod subject;
pub mod upstream;

pub use auditor::Auditor;
pub use background::Background;
pub use denied::Denied;
pub use subject::Subject;

use abnegate_agent::tool;
use abnegate_agent_cli::AgentKind;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_core::llm::LlmBackend;
use zone_enterprise::Feature;
use zone_enterprise::policy::{self, Denial, Effective, Rules};

use crate::db::ai_settings::Layer;
use crate::db::audit::{self, AuditContext};
use crate::db::{policies, workspaces};
use crate::services::endpoint::{Endpoint, Origin};
use crate::services::license::Stored;
use crate::services::provider::Provider;
use crate::state::AppState;

const CODE: &str = "code";
const DETAIL: &str = "detail";

/// The policy `organization`'s rules, and `workspace`'s when given, make, or none when the
/// license enforces no policy. A license that has not loaded, or rules that cannot be read,
/// deny everything with [`Denial::Unreadable`].
pub async fn load(
    state: &AppState,
    organization: Uuid,
    workspace: Option<Uuid>,
) -> Result<Option<Effective>, Denial> {
    if !enforced(state).await? {
        return Ok(None);
    }
    read(state, organization, workspace).await.map(Some)
}

/// [`load`] for `workspace` under the organization it belongs to. A workspace that does not
/// exist has none.
pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Result<Option<Effective>, Denial> {
    if !enforced(state).await? {
        return Ok(None);
    }
    let organization = match workspaces::get_workspace(state.db(), workspace).await {
        Ok(Some(row)) => row.organization_id,
        Ok(None) => return Ok(None),
        Err(error) => {
            tracing::warn!(%workspace, %error, "Could not read the workspace, so its agent policy denies everything until it can be read");
            return Err(Denial::Unreadable);
        }
    };
    read(state, organization, Some(workspace)).await.map(Some)
}

pub fn agent(kind: AgentKind) -> Option<policy::Agent> {
    policy::Agent::named(kind.as_str())
}

/// Whether `effective` lets `kind` run. An agent the policy has no name for is never named in
/// one, so nothing restricts it.
pub fn admits(effective: &Effective, kind: AgentKind) -> Result<(), Denial> {
    agent(kind).map_or(Ok(()), |agent| effective.agent(agent))
}

/// Whether `effective` lets a workspace run on `provider`: a coding agent's by its agent, a
/// direct API's by that API. Self-Hosted and Bedrock run on a LiteLLM whose upstream is known
/// only per model, so [`admit`] judges them.
pub fn provider(effective: &Effective, provider: Provider) -> Result<(), Denial> {
    match provider {
        Provider::ClaudeCode | Provider::Codex => match provider.agent() {
            Some(kind) => admits(effective, kind),
            None => Ok(()),
        },
        Provider::OpenAI => effective.provider(policy::Provider::OpenAI),
        Provider::Anthropic => effective.provider(policy::Provider::Anthropic),
        Provider::SelfHosted | Provider::Bedrock => Ok(()),
    }
}

pub fn ceiling(effective: &Effective) -> Option<tool::Tier> {
    effective.ceiling().map(|tier| match tier {
        policy::Tier::Read => tool::Tier::Read,
        policy::Tier::Write => tool::Tier::Write,
        policy::Tier::Host => tool::Tier::Host,
        policy::Tier::Outward => tool::Tier::Outward,
    })
}

/// Whether `effective` lets `model` run on `backend`: the model itself, then a coding agent by
/// its agent, a direct API by that API, and anything else over HTTP by where `endpoint` sends it.
pub async fn admit(
    effective: &Effective,
    endpoint: &Endpoint,
    backend: &LlmBackend,
    provider: Option<Provider>,
    model: &str,
) -> Result<(), Denial> {
    effective.model(model)?;
    match backend {
        LlmBackend::Cli { agent, .. } => admits(effective, *agent),
        LlmBackend::Http => over_http(effective, endpoint, provider, model).await,
    }
}

/// [`admit`] over HTTP. The upstream is looked up only while providers are restricted.
pub(crate) async fn over_http(
    effective: &Effective,
    endpoint: &Endpoint,
    provider: Option<Provider>,
    model: &str,
) -> Result<(), Denial> {
    match provider {
        Some(Provider::OpenAI) => effective.provider(policy::Provider::OpenAI),
        Some(Provider::Anthropic) => effective.provider(policy::Provider::Anthropic),
        _ if effective.restricts_providers() => effective.provider(upstream(endpoint, model).await),
        _ => Ok(()),
    }
}

/// Where `endpoint` runs `model`, by the LiteLLM route it publishes. A workspace admin can point
/// a workspace's endpoint at a proxy that publishes any routes it likes, so only the routes of
/// the instance's endpoint, or of one the organization saved, are believed.
async fn upstream(endpoint: &Endpoint, model: &str) -> policy::Provider {
    let believed = match endpoint.origin() {
        Origin::Instance => true,
        Origin::Settings => endpoint.layer() == Some(Layer::Organization),
    };
    if !believed {
        return policy::Provider::Other;
    }
    upstream::route(endpoint, model)
        .await
        .map_or(policy::Provider::Other, |routed| {
            policy::Provider::routed(&routed)
        })
}

/// A failure to record `denied` is logged and never fails the caller.
pub async fn record(state: &AppState, action: &'static str, denied: &Denied<'_>) {
    let organization = match denied.organization {
        Some(organization) => Some(organization),
        None => organization_of(state, denied.workspace).await,
    };
    let context = AuditContext {
        org_id: organization,
        workspace_id: Some(denied.workspace),
        actor_id: denied.actor,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    let values = json!({ CODE: denied.denial.code(), DETAIL: denied.detail });
    if let Err(error) = audit::log_action(
        state.db(),
        &context,
        action,
        denied.subject.resource(),
        Some(denied.subject.id()),
        None,
        Some(values),
    )
    .await
    {
        tracing::error!(%error, action, workspace = %denied.workspace, "Failed to record an agent policy denial");
    }
}

/// [`record`] for what `actor` asked of `organization` itself, such as a sign-in or its AI
/// settings.
pub async fn record_organization(
    state: &AppState,
    action: &'static str,
    organization: Uuid,
    actor: Uuid,
    denial: Denial,
    detail: Option<&str>,
) {
    let context = AuditContext {
        org_id: Some(organization),
        workspace_id: None,
        actor_id: Some(actor),
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    let values = json!({ CODE: denial.code(), DETAIL: detail });
    if let Err(error) = audit::log_action(
        state.db(),
        &context,
        action,
        audit::resources::ORGANIZATION,
        Some(organization),
        None,
        Some(values),
    )
    .await
    {
        tracing::error!(%error, action, %organization, "Failed to record an agent policy denial");
    }
}

async fn organization_of(state: &AppState, workspace: Uuid) -> Option<Uuid> {
    match workspaces::get_workspace(state.db(), workspace).await {
        Ok(row) => row.map(|row| row.organization_id),
        Err(error) => {
            tracing::warn!(%workspace, %error, "Could not read the workspace a denial is audited for");
            None
        }
    }
}

/// A stored key that no longer verifies, such as after a keyring change, keeps the saved
/// policies enforced: only removing the license stops enforcement.
pub fn enforces(stored: &Stored) -> bool {
    match stored {
        Stored::Unverified(_) => true,
        Stored::Empty | Stored::Verified(_) => Feature::Policy.enforced(stored.license()),
    }
}

async fn enforced(state: &AppState) -> Result<bool, Denial> {
    match state.stored_license().await {
        Ok(stored) => {
            if let Some(error) = stored.refusal() {
                tracing::warn!(
                    code = error.code(),
                    "The stored license key does not verify, so agent policies stay enforced and can't be changed until an admin replaces or removes it"
                );
            }
            Ok(enforces(&stored))
        }
        Err(error) => {
            tracing::warn!(%error, "Could not read the license, so agent policies deny everything until it can be read");
            Err(Denial::Unreadable)
        }
    }
}

async fn read(
    state: &AppState,
    organization: Uuid,
    workspace: Option<Uuid>,
) -> Result<Effective, Denial> {
    let layers = policies::layers(state.db(), organization, workspace)
        .await
        .map_err(|error| {
            tracing::warn!(%organization, ?workspace, %error, "Could not read the agent policies, so they deny everything until they can be read");
            Denial::Unreadable
        })?;
    Ok(Effective::new(
        rules(layers.organization).map_err(|error| {
            tracing::warn!(%organization, error, "The organization's stored agent policy can't be read, so it denies everything until it is saved again");
            Denial::Unreadable
        })?,
        rules(layers.workspace).map_err(|error| {
            tracing::warn!(?workspace, error, "The workspace's stored agent policy can't be read, so it denies everything until it is saved again");
            Denial::Unreadable
        })?,
    ))
}

fn rules(stored: Option<Value>) -> Result<Rules, String> {
    let Some(stored) = stored else {
        return Ok(Rules::default());
    };
    serde_json::from_value::<Rules>(stored)
        .map_err(|error| error.to_string())?
        .normalized()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::CliSettings;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::services::endpoint::testing::{endpoint, saved};
    use crate::state::{test_config, unreachable_pool};

    const KEY: &str = "sk-policy-test";
    const MODEL: &str = "claude-sonnet-4-5";

    fn effective(rules: Rules) -> Effective {
        Effective::new(rules, Rules::default())
    }

    fn providers(allowed: &[policy::Provider]) -> Effective {
        effective(Rules {
            providers: Some(allowed.to_vec()),
            ..Rules::default()
        })
    }

    fn cli(kind: AgentKind) -> LlmBackend {
        LlmBackend::cli(kind, CliSettings::default())
    }

    async fn routing(model: &str) -> MockServer {
        // Unpooled: a pooled server's URL may still have another test's routes cached.
        let server = MockServer::builder().start().await;
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"model_name": "*", "litellm_params": {"model": model}}],
                "total_pages": 1
            })))
            .mount(&server)
            .await;
        server
    }

    async fn admitted(
        effective: &Effective,
        server: &MockServer,
        backend: &LlmBackend,
        provider: Option<Provider>,
        model: &str,
    ) -> Result<(), Denial> {
        admit(
            effective,
            &endpoint(&server.uri(), KEY),
            backend,
            provider,
            model,
        )
        .await
    }

    #[tokio::test]
    async fn a_model_the_policy_does_not_list_is_denied_on_any_backend() {
        let server = MockServer::builder().start().await;
        let effective = effective(Rules {
            models: Some(vec!["claude-opus-*".into()]),
            ..Rules::default()
        });
        for backend in [LlmBackend::Http, cli(AgentKind::Claude)] {
            assert_eq!(
                admitted(&effective, &server, &backend, None, MODEL).await,
                Err(Denial::Model)
            );
            assert_eq!(
                admitted(&effective, &server, &backend, None, "claude-opus-4-1").await,
                Ok(())
            );
        }
    }

    #[tokio::test]
    async fn a_direct_api_is_judged_by_that_api_without_a_lookup() {
        let server = MockServer::builder().start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let effective = providers(&[policy::Provider::Anthropic]);

        assert_eq!(
            admitted(
                &effective,
                &server,
                &LlmBackend::Http,
                Some(Provider::Anthropic),
                MODEL
            )
            .await,
            Ok(())
        );
        assert_eq!(
            admitted(
                &effective,
                &server,
                &LlmBackend::Http,
                Some(Provider::OpenAI),
                "gpt-5"
            )
            .await,
            Err(Denial::Provider(policy::Provider::OpenAI))
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_self_hosted_endpoint_is_judged_by_the_upstream_it_routes_to() {
        let server = routing("bedrock/anthropic.claude-sonnet-4").await;

        assert_eq!(
            admitted(
                &providers(&[policy::Provider::Vertex]),
                &server,
                &LlmBackend::Http,
                Some(Provider::SelfHosted),
                MODEL
            )
            .await,
            Err(Denial::Provider(policy::Provider::Bedrock))
        );
        assert_eq!(
            admitted(
                &providers(&[policy::Provider::Bedrock]),
                &server,
                &LlmBackend::Http,
                Some(Provider::SelfHosted),
                MODEL
            )
            .await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn only_the_instance_and_the_organization_are_believed_about_their_routes() {
        let server = routing("ollama_chat/*").await;
        let local = providers(&[policy::Provider::Local]);
        let over = |endpoint: Endpoint| {
            let local = local.clone();
            async move {
                admit(
                    &local,
                    &endpoint,
                    &LlmBackend::Http,
                    Some(Provider::SelfHosted),
                    MODEL,
                )
                .await
            }
        };

        assert_eq!(over(endpoint(&server.uri(), KEY)).await, Ok(()));
        assert_eq!(
            over(saved(&server.uri(), KEY, Layer::Organization)).await,
            Ok(())
        );
        assert_eq!(
            over(saved(&server.uri(), KEY, Layer::Workspace)).await,
            Err(Denial::Provider(policy::Provider::Other))
        );
    }

    #[tokio::test]
    async fn an_endpoint_without_routes_is_another_provider() {
        let server = MockServer::builder().start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        assert_eq!(
            admitted(
                &providers(&[policy::Provider::Local]),
                &server,
                &LlmBackend::Http,
                Some(Provider::SelfHosted),
                MODEL
            )
            .await,
            Err(Denial::Provider(policy::Provider::Other))
        );
        assert_eq!(
            admitted(
                &providers(&[policy::Provider::Other]),
                &server,
                &LlmBackend::Http,
                None,
                MODEL
            )
            .await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn nothing_is_looked_up_while_providers_are_unrestricted() {
        let server = MockServer::builder().start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let effective = effective(Rules {
            agents: Some(vec![]),
            ..Rules::default()
        });

        assert_eq!(
            admitted(
                &effective,
                &server,
                &LlmBackend::Http,
                Some(Provider::SelfHosted),
                MODEL
            )
            .await,
            Ok(())
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_coding_agent_is_judged_by_its_agent_and_model_alone() {
        let server = MockServer::builder().start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let claude_only = effective(Rules {
            providers: Some(vec![]),
            agents: Some(vec![policy::Agent::Claude]),
            ..Rules::default()
        });

        assert_eq!(
            admitted(
                &claude_only,
                &server,
                &cli(AgentKind::Claude),
                Some(Provider::ClaudeCode),
                MODEL
            )
            .await,
            Ok(())
        );
        assert_eq!(
            admitted(
                &claude_only,
                &server,
                &cli(AgentKind::Codex),
                Some(Provider::Codex),
                "gpt-5"
            )
            .await,
            Err(Denial::Agent(policy::Agent::Codex))
        );
        server.verify().await;
    }

    #[test]
    fn each_provider_is_judged_by_what_it_runs_on() {
        let nothing = effective(Rules {
            providers: Some(vec![]),
            agents: Some(vec![]),
            ..Rules::default()
        });
        for (provider, expected) in [
            (
                Provider::ClaudeCode,
                Err(Denial::Agent(policy::Agent::Claude)),
            ),
            (Provider::Codex, Err(Denial::Agent(policy::Agent::Codex))),
            (
                Provider::OpenAI,
                Err(Denial::Provider(policy::Provider::OpenAI)),
            ),
            (
                Provider::Anthropic,
                Err(Denial::Provider(policy::Provider::Anthropic)),
            ),
            (Provider::SelfHosted, Ok(())),
            (Provider::Bedrock, Ok(())),
        ] {
            assert_eq!(
                super::provider(&nothing, provider),
                expected,
                "{provider:?}"
            );
            assert_eq!(
                super::provider(&effective(Rules::default()), provider),
                Ok(())
            );
        }
    }

    #[test]
    fn every_agent_has_its_policy_name() {
        assert_eq!(agent(AgentKind::Claude), Some(policy::Agent::Claude));
        assert_eq!(agent(AgentKind::Codex), Some(policy::Agent::Codex));
    }

    #[test]
    fn the_ceiling_is_the_tool_tier_of_the_same_name() {
        for (ceiling, expected) in [
            (None, None),
            (Some(policy::Tier::Write), Some(tool::Tier::Write)),
            (Some(policy::Tier::Host), Some(tool::Tier::Host)),
        ] {
            let effective = effective(Rules {
                ceiling,
                ..Rules::default()
            });
            assert_eq!(super::ceiling(&effective), expected);
        }
    }

    #[test]
    fn stored_rules_that_cannot_be_read_are_refused() {
        assert_eq!(rules(None), Ok(Rules::default()));
        assert_eq!(
            rules(Some(json!({"agents": ["claude"]}))).map(|rules| rules.agents),
            Ok(Some(vec![policy::Agent::Claude]))
        );
        for stored in [
            json!({"agents": "claude"}),
            json!({"providers": ["gemini"]}),
            json!({"ceiling": "outward"}),
            json!("rules"),
        ] {
            assert!(rules(Some(stored.clone())).is_err(), "{stored}");
        }
    }

    #[tokio::test]
    async fn an_unreachable_database_with_no_license_loaded_denies_everything() {
        let state = AppState::new(test_config(), unreachable_pool(), None);
        state.disable_mcp();

        assert_eq!(state.license().await, None);
        assert_eq!(
            load(&state, Uuid::new_v4(), Some(Uuid::new_v4())).await,
            Err(Denial::Unreadable)
        );
        assert_eq!(
            for_workspace(&state, Uuid::new_v4()).await,
            Err(Denial::Unreadable)
        );
    }
}
