//! What an owner does to SCIM provisioning from the console: issuing, rotating and revoking the
//! tokens identity providers present, and mapping pushed groups to organization roles.

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::issued::Issued;
use super::remapped::Remapped;
use super::remapping::Remapping;
use super::secret;
use crate::auth::{Grant, OrganizationMember, Scope};
use crate::db::DbResult;
use crate::db::custom_roles::{self, Assignment};
use crate::db::organization_members::OrgRole;
use crate::db::scim::{Mapping, NewToken, TokenRecord, Trigger, groups, roles, tokens};

pub async fn issue(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    name: &str,
    created_by: Uuid,
) -> DbResult<Issued> {
    let secret = secret::generate();
    let token = tokens::create(
        executor,
        &NewToken {
            organization_id,
            name,
            token_hash: &secret::hash(&secret),
            hint: &secret::hint(&secret),
            created_by: Some(created_by),
        },
    )
    .await?;
    Ok(Issued { token, secret })
}

/// Replaces the token's secret at once: the old one stops working when this commits.
pub async fn rotate(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    token_id: Uuid,
) -> DbResult<Option<Issued>> {
    let secret = secret::generate();
    let token = tokens::rotate(
        executor,
        organization_id,
        token_id,
        &secret::hash(&secret),
        &secret::hint(&secret),
    )
    .await?;
    Ok(token.map(|token| Issued { token, secret }))
}

pub async fn revoke(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    token_id: Uuid,
) -> DbResult<Option<TokenRecord>> {
    tokens::revoke(executor, organization_id, token_id).await
}

/// Maps the group to a built-in or custom role the editor's grant covers, then recomputes the
/// role of every member. Owner is never mappable.
pub async fn map(
    pool: &PgPool,
    organization_id: Uuid,
    group_id: Uuid,
    assignment: Assignment<OrgRole>,
    editor: &OrganizationMember,
) -> DbResult<Remapping> {
    remap(pool, organization_id, group_id, Some(assignment), editor).await
}

/// Leaves the group mapped to nothing, then recomputes the role of every member: one in no other
/// mapped group falls back to Member.
pub async fn unmap(
    pool: &PgPool,
    organization_id: Uuid,
    group_id: Uuid,
    editor: &OrganizationMember,
) -> DbResult<Remapping> {
    remap(pool, organization_id, group_id, None, editor).await
}

async fn remap(
    pool: &PgPool,
    organization_id: Uuid,
    group_id: Uuid,
    assignment: Option<Assignment<OrgRole>>,
    editor: &OrganizationMember,
) -> DbResult<Remapping> {
    if assignment == Some(Assignment::BuiltIn(OrgRole::Owner)) {
        return Ok(Remapping::OwnerUnmappable);
    }
    let mut transaction = pool.begin().await?;
    let Some(previous) = groups::lock(&mut transaction, organization_id, group_id).await? else {
        return Ok(Remapping::GroupMissing);
    };
    let (mapping, grant) = match assignment {
        None => (Mapping::Unmapped, Grant::default()),
        Some(Assignment::BuiltIn(role)) => (Mapping::BuiltIn(role), Grant::organization(role)),
        Some(Assignment::Custom(id)) => {
            match custom_roles::lock(&mut transaction, organization_id, id, Scope::Organization)
                .await?
            {
                Some((summary, grant)) => (Mapping::Custom(summary), grant),
                None => return Ok(Remapping::RoleMissing),
            }
        }
    };
    if !editor.grant.covers(&grant) {
        return Ok(Remapping::Exceeds);
    }
    let Some(group) = groups::map(
        &mut *transaction,
        organization_id,
        group_id,
        &mapping,
        editor.user_id,
    )
    .await?
    else {
        return Ok(Remapping::GroupMissing);
    };
    transaction.commit().await?;
    let outcomes = roles::apply_group(pool, organization_id, group_id, Trigger::Mapping).await?;
    Ok(Remapping::Remapped(Box::new(Remapped {
        previous: previous.mapping,
        group,
        outcomes,
    })))
}
