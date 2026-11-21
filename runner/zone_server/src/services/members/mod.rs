//! The Community edition's cap on active members, checked wherever a person
//! would become a member.

mod admission_error;

use chrono::Utc;
use uuid::Uuid;
use zone_enterprise::{Members, period_start};

use crate::db::seats;
use crate::state::AppState;

pub use admission_error::AdmissionError;

/// Admits `user`, or a person without an account when `None`, unless the
/// server is capped and full. An existing member is always admitted, however
/// far over the cap the server is.
pub async fn admit(state: &AppState, user: Option<Uuid>) -> Result<(), AdmissionError> {
    #[cfg(any(test, feature = "testing"))]
    if !state.members_capped() {
        return Ok(());
    }
    let now = Utc::now();
    let license = state.settled_license().await?;
    let Some(cap) = Members::cap(license.as_ref(), now) else {
        return Ok(());
    };
    if let Some(user) = user
        && seats::member(state.db(), user).await?
    {
        return Ok(());
    }
    let members = Members {
        active: seats::used(state.db(), period_start(now)).await?,
        cap: Some(cap),
    };
    if members.full() {
        return Err(AdmissionError::Full { cap });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
