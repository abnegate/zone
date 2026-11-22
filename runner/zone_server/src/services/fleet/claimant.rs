use zone_enterprise::Feature;

use crate::db::fleet::claimant::Claimant;
use crate::services::fleet::registration;
use crate::state::AppState;

/// The claimant this process claims work as. Fleet policy is enforced under an Enterprise
/// license, a stored key that no longer verifies, or a license that can't be read. An enforced
/// claimant that hasn't registered registers first; if that fails it has no runner, and every
/// claim it makes is refused as unregistered. Claims are refused the same way while another live
/// process sharing the runner's name holds the row, until this process's next heartbeat
/// registers it again, so each process is judged on its own version and labels.
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
        (None, true) => match registration::register(state).await {
            Ok(runner) => Some(runner),
            Err(error) => {
                tracing::warn!(%error, "Could not register as a runner, so this server takes no work until it can");
                None
            }
        },
    };
    Claimant {
        runner,
        instance: state.instance(),
        enforced,
    }
}
