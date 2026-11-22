use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::deactivation::Deactivation;
use super::removal::Removal;
use crate::db::DbResult;
use crate::db::organization_members;
use crate::db::suspensions::{self, Resumed};

pub async fn deactivate(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Deactivation> {
    let owners = organization_members::lock_owners(transaction, organization_id).await?;
    if last_owner(&owners, user_id) {
        return Ok(Deactivation::LastOwner);
    }
    let suspension = suspensions::suspend(transaction, organization_id, user_id).await?;
    Ok(Deactivation::Deactivated(suspension))
}

pub async fn reactivate(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Resumed> {
    suspensions::resume(transaction, organization_id, user_id).await
}

pub async fn remove(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Removal> {
    let owners = organization_members::lock_owners(transaction, organization_id).await?;
    let member: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if member.is_none() {
        return Ok(Removal::Missing);
    }
    if last_owner(&owners, user_id) {
        return Ok(Removal::LastOwner);
    }

    let successors = suspensions::remove(transaction, organization_id, user_id).await?;
    sqlx::query(
        "DELETE FROM scim_group_members AS member USING scim_groups AS scim_group \
         WHERE scim_group.id = member.group_id \
           AND scim_group.organization_id = $1 AND member.user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM scim_users WHERE organization_id = $1 AND user_id = $2")
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    Ok(Removal::Removed(successors))
}

fn last_owner(owners: &[Uuid], user_id: Uuid) -> bool {
    owners.len() <= 1 && owners.contains(&user_id)
}
