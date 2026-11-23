use std::collections::BTreeSet;

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::issued::Issued;
use super::remapped::Remapped;
use super::remapping::Remapping;
use super::secret;
use crate::auth::{Grant, OrganizationMember, Scope};
use crate::db::DbResult;
use crate::db::custom_roles::{self, Assignment, Summary};
use crate::db::organization_members::{self, OrgRole};
use crate::db::scim::{
    GroupRecord, Mapping, NewToken, TokenRecord, Trigger, group_members, groups, roles, tokens,
};

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

pub async fn map(
    pool: &PgPool,
    organization_id: Uuid,
    group_id: Uuid,
    assignment: Assignment<OrgRole>,
    editor: &OrganizationMember,
) -> DbResult<Remapping> {
    remap(pool, organization_id, group_id, Some(assignment), editor).await
}

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
    let mut transaction = pool.begin().await?;
    organization_members::lock_owners(&mut transaction, organization_id).await?;
    let outcomes = roles::apply_group(
        &mut transaction,
        organization_id,
        group_id,
        Trigger::Mapping,
    )
    .await?;
    transaction.commit().await?;
    Ok(Remapping::Remapped(Box::new(Remapped {
        previous: previous.mapping,
        group,
        outcomes,
    })))
}

pub async fn release(
    pool: &PgPool,
    organization_id: Uuid,
    role: &Summary,
    unmapped: Vec<GroupRecord>,
) -> DbResult<Vec<Remapped>> {
    if unmapped.is_empty() {
        return Ok(Vec::new());
    }
    let mut transaction = pool.begin().await?;
    organization_members::lock_owners(&mut transaction, organization_id).await?;
    let mut rosters: Vec<BTreeSet<Uuid>> = Vec::with_capacity(unmapped.len());
    for group in &unmapped {
        let roster = group_members::users(&mut *transaction, organization_id, group.id).await?;
        rosters.push(roster.into_iter().collect());
    }
    let everyone: Vec<Uuid> = rosters.iter().flatten().copied().collect();
    let outcomes = roles::apply_users(
        &mut transaction,
        organization_id,
        &everyone,
        Trigger::Mapping,
    )
    .await?;
    transaction.commit().await?;
    Ok(unmapped
        .into_iter()
        .zip(rosters)
        .map(|(group, roster)| Remapped {
            previous: Mapping::Custom(role.clone()),
            group,
            outcomes: outcomes
                .iter()
                .filter(|outcome| roster.contains(&outcome.user_id))
                .cloned()
                .collect(),
        })
        .collect())
}
