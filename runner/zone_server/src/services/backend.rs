//! Where a workspace's completions go: the endpoint, or a coding agent CLI.

use uuid::Uuid;
use zone_core::llm::{AgentKind, LlmBackend};

use crate::config::Config;
use crate::db::ai_settings::EffectiveAiSettings;
use crate::state::AppState;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "The {agent} CLI is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings."
    )]
    SignedOut { agent: AgentKind },
    #[error(
        "The {agent} CLI's sign-in could not be renewed: {message}. Sign in again under Organization Settings > AI Settings."
    )]
    Renewal { agent: AgentKind, message: String },
    #[error("Could not prepare the {agent} CLI's state directory: {message}")]
    Home { agent: AgentKind, message: String },
}

pub fn instance(config: &Config) -> LlmBackend {
    crate::state::llm_backend(config)
}

pub async fn for_workspace(state: &AppState, _workspace: Uuid) -> Result<LlmBackend, Error> {
    Ok(instance(state.config()))
}

pub async fn for_settings(
    state: &AppState,
    _organization: Uuid,
    _settings: &EffectiveAiSettings,
) -> Result<LlmBackend, Error> {
    Ok(instance(state.config()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use std::path::PathBuf;
    use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

    use crate::config::ModelBackend;
    use crate::db::{organizations, workspaces};

    struct Fixture {
        pool: PgPool,
        organization: Uuid,
        workspace: Uuid,
    }

    impl Fixture {
        async fn self_hosted() -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
            )
            .await
            .expect("the test database");
            let organization = organizations::create_organization(
                &pool,
                "Backend test",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .expect("an organization");
            let workspace = workspaces::create_workspace(
                &pool,
                organization.id,
                "Backend",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .expect("a workspace");
            sqlx::query(
                "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2)",
            )
            .bind(organization.id)
            .bind(PROVIDER_SELF_HOSTED)
            .execute(&pool)
            .await
            .expect("the organization's AI settings");
            Self {
                pool,
                organization: organization.id,
                workspace: workspace.id,
            }
        }

        async fn resolve(&self, backend: ModelBackend) -> Result<LlmBackend, Error> {
            let state = AppState::new(
                Config {
                    model_backend: backend,
                    ..crate::state::test_config()
                },
                self.pool.clone(),
                None,
            );
            for_workspace(&state, self.workspace).await
        }

        async fn remove(self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
        }
    }

    #[tokio::test]
    async fn a_self_hosted_workspace_keeps_the_endpoint_the_instance_defaults_to() {
        let fixture = Fixture::self_hosted().await;
        let resolved = fixture.resolve(ModelBackend::LiteLlm).await;
        fixture.remove().await;

        assert!(matches!(resolved, Ok(LlmBackend::Http)), "{resolved:?}");
    }

    #[tokio::test]
    async fn a_self_hosted_workspace_keeps_the_agent_the_instance_defaults_to() {
        let fixture = Fixture::self_hosted().await;
        let executable = PathBuf::from("/opt/homebrew/bin/claude");
        let resolved = fixture
            .resolve(ModelBackend::Cli {
                agent: AgentKind::Claude,
                executable: Some(executable.clone()),
            })
            .await;
        fixture.remove().await;

        let (agent, settings) = match resolved {
            Ok(LlmBackend::Cli { agent, settings }) => (agent, settings),
            other => panic!("the instance's claude was replaced: {other:?}"),
        };
        assert_eq!(agent, AgentKind::Claude);
        assert_eq!(settings.executable, Some(executable));
        assert_eq!(
            settings.working_directory, None,
            "the instance's own agent runs where it always has"
        );
    }
}
