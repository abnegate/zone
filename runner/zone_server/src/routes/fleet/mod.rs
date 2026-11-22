//! Runner fleet policy. Server admins read the fleet on every edition and, under an Enterprise
//! license, set its minimum version and cordon, revoke or remove its runners. Whoever edits an
//! organization's or a workspace's agent policy also edits the runner labels its unattended work
//! requires. Every change is audited.

pub mod labels;
pub mod policy;
pub mod refusal;
pub mod requests;
pub mod responses;
pub mod runners;
#[cfg(test)]
mod tests;

use chrono::Utc;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
use zone_enterprise::Feature;

use crate::auth::AuthUser;
use crate::db::fleet::runners::{self as fleet_runners, Change};
use crate::routes::policy::standing::Standing;
use crate::state::AppState;
use refusal::Refusal;

fn caller(auth: &AuthUser) -> Result<Uuid, Refusal> {
    auth.0.user_id().map_err(|_| Refusal::Unauthenticated)
}

async fn standing(state: &AppState) -> Result<Standing, Refusal> {
    Ok(Standing::of(
        &state.stored_license().await?,
        Utc::now(),
        Feature::FleetPolicy,
    ))
}

/// Refuses `change` when it would leave no connected runner able to take work, unless the admin
/// confirmed it. The caller holds `fleet_policy` locked, so two admins can't each leave the
/// other's runner as the last one.
async fn guard(
    transaction: &mut Transaction<'_, Postgres>,
    change: Change,
    confirm: bool,
) -> Result<(), Refusal> {
    if confirm {
        return Ok(());
    }
    match fleet_runners::remaining(transaction, change).await? {
        remaining if remaining > 0 => Ok(()),
        remaining => Err(Refusal::NoRunnerLeft { remaining }),
    }
}
