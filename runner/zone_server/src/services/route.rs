//! Where a workspace's completions go, read from its AI settings once.

use uuid::Uuid;
use zone_core::llm::{AgentKind, LlmBackend};

use crate::config::Config;
use crate::db::ai_settings::{self, EffectiveAiSettings};
use crate::db::workspaces;
use crate::services::backend;
use crate::services::endpoint::Endpoint;
use crate::services::stages::Preferences;
use crate::state::AppState;

/// What a workspace's AI settings choose for its completions: the endpoint an
/// HTTP backend sends them to, and the settings themselves, read once. A
/// workspace or settings that cannot be read leave the completions on the
/// instance.
#[derive(Clone)]
pub struct Route {
    pub endpoint: Endpoint,
    saved: Option<Saved>,
}

#[derive(Clone)]
struct Saved {
    organization: Uuid,
    settings: EffectiveAiSettings,
}

impl Route {
    pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Self {
        match saved(state, workspace).await {
            Some((organization, settings)) => {
                Self::new(state.config(), workspace, organization, settings)
            }
            None => Self::instance(state.config()),
        }
    }

    /// The route `settings`, one of `organization`'s for `workspace`, choose.
    pub fn new(
        config: &Config,
        workspace: Uuid,
        organization: Uuid,
        settings: EffectiveAiSettings,
    ) -> Self {
        let endpoint = Endpoint::try_resolve(config, &settings).unwrap_or_else(|error| {
            tracing::warn!(
                %workspace,
                provider = %settings.provider,
                %error,
                "The endpoint URL saved in AI Settings is invalid; using the instance's endpoint"
            );
            Endpoint::instance(config)
        });
        Self {
            endpoint,
            saved: Some(Saved {
                organization,
                settings,
            }),
        }
    }

    pub fn instance(config: &Config) -> Self {
        Self {
            endpoint: Endpoint::instance(config),
            saved: None,
        }
    }

    /// These settings with their completions sent to the instance's own
    /// endpoint instead.
    pub fn on_instance(self, config: &Config) -> Self {
        Self {
            endpoint: Endpoint::instance(config),
            ..self
        }
    }

    pub fn settings(&self) -> Option<&EffectiveAiSettings> {
        self.saved.as_ref().map(|saved| &saved.settings)
    }

    /// The organization whose settings run its workspace on `agent`.
    pub fn organization_on(&self, agent: AgentKind) -> Option<Uuid> {
        self.saved
            .as_ref()
            .filter(|saved| saved.settings.agent() == Some(agent))
            .map(|saved| saved.organization)
    }

    /// The backend these settings choose, resolved when a completion needs
    /// one: a coding agent's sign-in is read only then.
    pub async fn backend(&self, state: &AppState) -> Result<LlmBackend, backend::Error> {
        match &self.saved {
            Some(saved) => {
                backend::for_settings(
                    state,
                    saved.organization,
                    &saved.settings,
                    self.endpoint.origin(),
                )
                .await
            }
            None => Ok(backend::instance(state.config())),
        }
    }

    /// The models these settings prefer for completions sent to this route's
    /// endpoint.
    pub fn preferences(&self, classifier: &str) -> Preferences {
        match self.settings() {
            Some(settings) => Preferences::for_endpoint(settings, classifier, &self.endpoint),
            None => Preferences::from_optional_settings(None, classifier),
        }
    }
}

/// The organization `workspace` belongs to and its effective AI settings, or
/// nothing when either cannot be read, with the reason logged.
async fn saved(state: &AppState, workspace: Uuid) -> Option<(Uuid, EffectiveAiSettings)> {
    #[cfg(test)]
    reads::record(workspace);
    let organization = match workspaces::get_workspace(state.db(), workspace).await {
        Ok(Some(row)) => row.organization_id,
        Ok(None) => {
            tracing::warn!(%workspace, "No such workspace; using the instance's endpoint");
            return None;
        }
        Err(error) => {
            tracing::warn!(
                %workspace,
                %error,
                "Could not read the workspace; using the instance's endpoint"
            );
            return None;
        }
    };
    match ai_settings::get_effective_ai_settings(state.db(), organization, workspace).await {
        Ok(settings) => Some((organization, settings)),
        Err(error) => {
            tracing::warn!(
                %workspace,
                %error,
                "Could not read the AI settings; using the instance's endpoint"
            );
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod reads {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use uuid::Uuid;

    static READS: LazyLock<Mutex<HashMap<Uuid, usize>>> = LazyLock::new(Mutex::default);

    pub(super) fn record(workspace: Uuid) {
        *READS
            .lock()
            .expect("the read counter")
            .entry(workspace)
            .or_default() += 1;
    }

    /// How many times `workspace`'s AI settings have been read.
    pub fn of(workspace: Uuid) -> usize {
        READS
            .lock()
            .expect("the read counter")
            .get(&workspace)
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use sqlx::PgPool;
    use zone_context::embeddings::providers::PROVIDER_OPENAI;

    use super::*;
    use crate::db::organizations;
    use crate::services::endpoint::Origin;

    const INVALID: &str = "The endpoint URL saved in AI Settings is invalid";

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the captured log")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn lines(&self) -> Vec<String> {
            String::from_utf8_lossy(&self.0.lock().expect("the captured log"))
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    #[tokio::test]
    async fn an_invalid_saved_url_is_reported_once_with_its_workspace() {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Route", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Route", &suffix, None)
                .await
                .expect("a workspace");
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider, openai_base_url) \
             VALUES ($1, $2, 'https://proxy.example/v1?key=leak')",
        )
        .bind(organization.id)
        .bind(PROVIDER_OPENAI)
        .execute(&pool)
        .await
        .expect("the organization's AI settings");
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();

        let guard = tracing::subscriber::set_default(subscriber);
        let route = Route::for_workspace(&state, workspace.id).await;
        let backend = route.backend(&state).await;
        drop(guard);
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .expect("the organization to be removed");

        assert_eq!(route.endpoint.origin(), Origin::Instance);
        assert!(matches!(backend, Ok(LlmBackend::Http)), "{backend:?}");
        let warnings: Vec<String> = captured
            .lines()
            .into_iter()
            .filter(|line| line.contains(INVALID))
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:#?}");
        assert!(
            warnings[0].contains(&workspace.id.to_string()),
            "the warning does not say which workspace saved the URL: {}",
            warnings[0]
        );
    }
}
