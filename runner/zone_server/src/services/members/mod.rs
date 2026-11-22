mod admission_error;
mod deprovision;
mod deprovisioned;
mod halt;
mod halted;

use chrono::Utc;
use uuid::Uuid;
use zone_enterprise::{Members, period_start};

use crate::db::seats;
use crate::state::AppState;

pub use admission_error::AdmissionError;
pub use deprovision::deprovision;
pub use deprovisioned::Deprovisioned;
pub use halt::halt;
pub use halted::Halted;

/// Admits `user`, or a person without an account when `None`, unless the
/// server is capped and full. An existing member is always admitted, however
/// far over the cap the server is.
pub async fn admit(state: &AppState, user: Option<Uuid>) -> Result<(), AdmissionError> {
    let Some(cap) = cap(state).await? else {
        return Ok(());
    };
    if let Some(user) = user
        && seats::member(state.db(), user).await?
    {
        return Ok(());
    }
    make_room(state, cap, 1).await
}

/// Admits everyone that activating `organization` would make a member again.
pub async fn reactivate(state: &AppState, organization: Uuid) -> Result<(), AdmissionError> {
    let Some(cap) = cap(state).await? else {
        return Ok(());
    };
    match seats::returning(state.db(), organization).await? {
        0 => Ok(()),
        returning => make_room(state, cap, returning).await,
    }
}

async fn cap(state: &AppState) -> Result<Option<u32>, AdmissionError> {
    #[cfg(any(test, feature = "testing"))]
    if !state.members_capped() {
        return Ok(None);
    }
    let license = state.settled_license().await?;
    Ok(Members::cap(license.as_ref(), Utc::now()))
}

async fn make_room(state: &AppState, cap: u32, joining: u32) -> Result<(), AdmissionError> {
    let active = seats::used(state.db(), period_start(Utc::now())).await?;
    if active.saturating_add(joining) > cap {
        return Err(AdmissionError::Full { cap });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
