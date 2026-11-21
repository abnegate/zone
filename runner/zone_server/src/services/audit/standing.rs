use chrono::Utc;
use zone_enterprise::audit::Standing;

use crate::db::DbResult;
use crate::state::AppState;

/// The license as stored, verified or not: an unverified key streams nothing.
pub async fn standing(state: &AppState) -> DbResult<Standing> {
    let stored = state.stored_license().await?;
    Ok(Standing::of(
        stored.license(),
        stored.refusal().is_some(),
        Utc::now(),
    ))
}
