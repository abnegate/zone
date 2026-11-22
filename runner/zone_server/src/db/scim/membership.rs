use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::deactivation::Deactivation;
use super::joining::Joining;
use super::removal::Removal;
use crate::db::DbResult;
use crate::db::organization_members::{self, OrgRole};
use crate::db::suspensions::{self, Resumed};

pub async fn join(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
    active: bool,
) -> DbResult<Joining> {
    organization_members::lock_owners(transaction, organization_id).await?;
    let current: Option<bool> = sqlx::query_scalar(
        "SELECT is_active FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if current == Some(true) {
        return Ok(Joining::AlreadyMember);
    }

    suspensions::forget(&mut **transaction, organization_id, user_id).await?;
    sqlx::query(
        r#"
        INSERT INTO organization_members (organization_id, user_id, role, is_active, invited_at)
        VALUES ($1, $2, $3, $4, NOW())
        ON CONFLICT (organization_id, user_id) DO UPDATE
        SET role = EXCLUDED.role,
            custom_role_id = NULL,
            is_active = EXCLUDED.is_active,
            invited_by = NULL,
            invited_at = EXCLUDED.invited_at,
            updated_at = NOW()
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(OrgRole::Member.as_str())
    .bind(active)
    .execute(&mut **transaction)
    .await?;
    Ok(Joining::Joined)
}

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
