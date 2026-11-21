//! Why some of an organization's AI traffic never reaches a spend report: it bypasses the
//! instance's LiteLLM, or reaches it under a key Zone can't attribute.

use std::collections::BTreeSet;

use futures::future::try_join_all;
use uuid::Uuid;
use zone_context::embeddings::providers::{PROVIDER_OPENAI, PROVIDER_SELF_HOSTED};
use zone_enterprise::spend::Uncounted;

use crate::config::Config;
use crate::db::DbResult;
use crate::db::ai_settings::{self, EffectiveAiSettings, PROVIDER_ANTHROPIC};
use crate::db::workspaces;
use crate::services::endpoint::{Endpoint, Origin};
use crate::state::AppState;

/// Every reason the traffic of the organization's active workspaces goes uncounted, in
/// [`Uncounted`] order. A workspace's effective settings already carry the organization's.
pub async fn uncounted(state: &AppState, organization: Uuid) -> DbResult<Vec<Uncounted>> {
    let pool = state.db();
    let workspaces = workspaces::list_workspaces(pool, organization).await?;
    let settings = try_join_all(
        workspaces
            .iter()
            .filter(|workspace| workspace.is_active.unwrap_or(true))
            .map(|workspace| {
                ai_settings::get_effective_ai_settings(pool, organization, workspace.id)
            }),
    )
    .await?;
    Ok(merged(state.config(), &settings))
}

pub fn merged(config: &Config, settings: &[EffectiveAiSettings]) -> Vec<Uncounted> {
    settings
        .iter()
        .flat_map(|settings| reasons(config, settings))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Why traffic under `settings` goes uncounted; none when all of it is attributed.
pub fn reasons(config: &Config, settings: &EffectiveAiSettings) -> Vec<Uncounted> {
    let mut reasons = Vec::new();
    if settings.agent().is_some() || config.model_backend().agent().is_some() {
        reasons.push(Uncounted::CodingAgent);
    }
    let Ok(endpoint) = Endpoint::try_resolve(config, settings) else {
        return reasons;
    };
    let bypass = match endpoint.origin() {
        Origin::Settings => match settings.provider.as_str() {
            PROVIDER_OPENAI | PROVIDER_ANTHROPIC => Some(Uncounted::DirectProvider),
            PROVIDER_SELF_HOSTED => Some(Uncounted::ExternalEndpoint),
            _ => None,
        },
        Origin::Instance => (Endpoint::instance(config).attributable(config)
            && !endpoint.attributable(config))
        .then_some(Uncounted::SavedKey),
    };
    reasons.extend(bypass);
    reasons
}
