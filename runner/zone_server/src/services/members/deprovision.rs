use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{Deprovisioned, SignOut};
use crate::db::DbResult;
use crate::db::scim::users as links;
use crate::db::{invitations, organization_members, refresh_tokens, sessions};

pub async fn deprovision(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Deprovisioned> {
    let sign_out = sign_out(transaction, organization_id, user_id).await?;
    let invitations =
        invitations::revoke_pending(&mut **transaction, organization_id, user_id).await?;
    Ok(Deprovisioned {
        sign_out,
        invitations,
    })
}

async fn sign_out(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<SignOut> {
    if !links::created(&mut **transaction, organization_id, user_id).await? {
        return Ok(SignOut::Adopted);
    }
    if organization_members::shared_elsewhere(&mut **transaction, organization_id, user_id).await? {
        return Ok(SignOut::Shared);
    }
    refresh_tokens::revoke_all_user_tokens(&mut **transaction, user_id).await?;
    Ok(SignOut::Revoked {
        sessions: sessions::revoke_all_user_sessions(&mut **transaction, user_id).await?,
    })
}
