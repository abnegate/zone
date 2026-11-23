use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{Deprovisioned, SignOut};
use crate::db::DbResult;
use crate::db::{invitations, organization_members, refresh_tokens, sessions};

pub async fn deprovision(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Deprovisioned> {
    let sign_out =
        if organization_members::shared_elsewhere(&mut **transaction, organization_id, user_id)
            .await?
        {
            SignOut::Shared
        } else {
            refresh_tokens::revoke_all_user_tokens(&mut **transaction, user_id).await?;
            SignOut::Revoked {
                sessions: sessions::revoke_all_user_sessions(&mut **transaction, user_id).await?,
            }
        };
    let invitations =
        invitations::revoke_pending(&mut **transaction, organization_id, user_id).await?;
    Ok(Deprovisioned {
        sign_out,
        invitations,
    })
}
