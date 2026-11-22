use uuid::Uuid;

use super::{Deprovisioned, halt};
use crate::db::DbResult;
use crate::db::{refresh_tokens, sessions};
use crate::state::AppState;

/// Signs a member out everywhere and stops what they have running in the
/// organization. Sessions belong to the account, not the organization, so
/// every one of them is revoked.
pub async fn deprovision(
    state: &AppState,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Deprovisioned> {
    let mut transaction = state.db().begin().await?;
    let sessions = sessions::revoke_all_user_sessions(&mut *transaction, user_id).await?;
    refresh_tokens::revoke_all_user_tokens(&mut *transaction, user_id).await?;
    transaction.commit().await?;

    let halted = halt(state, organization_id, user_id).await?;
    Ok(Deprovisioned { sessions, halted })
}
