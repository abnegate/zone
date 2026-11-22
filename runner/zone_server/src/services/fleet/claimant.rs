use uuid::Uuid;
use zone_enterprise::Feature;

use crate::db::fleet::claimant::Claimant;
use crate::services::fleet::registration;
use crate::state::AppState;

/// The claimant this process claims work as. Fleet policy is enforced under an Enterprise
/// license, a stored key that no longer verifies, or a license that can't be read. An enforced
/// claimant that hasn't registered registers first; if that fails it has no runner, and every
/// claim it makes is refused as unregistered.
///
/// Registering writes `fleet_runners`, so this must not run inside an open transaction.
pub async fn claimant(state: &AppState) -> Claimant {
    let enforced = match state.stored_license().await {
        Ok(stored) => stored.enforces(Feature::FleetPolicy),
        Err(error) => {
            tracing::warn!(%error, "Could not read the license, so fleet policy stays enforced until it can be read");
            true
        }
    };
    let runner = match (state.runner(), enforced) {
        (Some(runner), _) => Some(runner),
        (None, false) => None,
        (None, true) => register(state).await,
    };
    Claimant { runner, enforced }
}

/// Registers this process lazily, once for every claim that asks at the same time: claims that
/// queued behind an attempt take its outcome rather than each registering and warning again.
async fn register(state: &AppState) -> Option<Uuid> {
    let attempt = async {
        if let Some(runner) = state.runner() {
            return Some(runner);
        }
        match registration::register(state).await {
            Ok(runner) => Some(runner),
            Err(error) => {
                tracing::warn!(%error, "Could not register as a runner, so this server takes no work until it can");
                None
            }
        }
    };
    match state.registration().join(attempt).await {
        Some(runner) => runner,
        None => state.runner(),
    }
}
