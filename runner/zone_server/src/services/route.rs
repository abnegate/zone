//! Where a workspace's completions go, read from its AI settings once.

use uuid::Uuid;
use zone_core::llm::LlmBackend;

use crate::config::Config;
use crate::db::ai_settings::{self, EffectiveAiSettings};
use crate::db::workspaces;
use crate::services::backend;
use crate::services::endpoint::Endpoint;
use crate::services::stages::Preferences;
use crate::state::AppState;

/// What a workspace's AI settings choose for its completions: the backend
/// they run on, the endpoint an HTTP backend sends them to, and the settings
/// themselves, read once. A workspace or settings that cannot be read leave
/// the completions on the instance.
pub struct Route {
    pub backend: Result<LlmBackend, backend::Error>,
    pub endpoint: Endpoint,
    pub settings: Option<EffectiveAiSettings>,
}

impl Route {
    pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Self {
        let Some((organization, settings)) = saved(state, workspace).await else {
            return Self::instance(state.config());
        };
        Self {
            backend: backend::for_settings(state, organization, &settings).await,
            endpoint: Endpoint::resolve(state.config(), &settings),
            settings: Some(settings),
        }
    }

    pub fn instance(config: &Config) -> Self {
        Self {
            backend: Ok(backend::instance(config)),
            endpoint: Endpoint::instance(config),
            settings: None,
        }
    }

    /// The models these settings prefer for completions sent to `endpoint`.
    pub fn preferences(&self, endpoint: &Endpoint, classifier: &str) -> Preferences {
        match &self.settings {
            Some(settings) => Preferences::for_endpoint(settings, classifier, endpoint),
            None => Preferences::from_optional_settings(None, classifier),
        }
    }
}

/// The organization `workspace` belongs to and its effective AI settings, or
/// nothing when either cannot be read, with the reason logged.
pub async fn saved(state: &AppState, workspace: Uuid) -> Option<(Uuid, EffectiveAiSettings)> {
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
