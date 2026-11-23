use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;
use uuid::Uuid;

use super::applied::Applied;
use super::candidate::Candidate;
use super::group_members;
use super::outcome::Outcome;
use super::precedence;
use super::trigger::Trigger;
use crate::auth::{Grant, Scope};
use crate::db::DbResult;
use crate::db::custom_roles::{self, Membership, Summary};
use crate::db::organization_members::OrgRole;
use crate::db::suspensions::Seat;

const ROLE_COLUMN: &str = "role";

#[derive(sqlx::FromRow)]
struct MappedGroup {
    role: Option<String>,
    custom_role_id: Option<Uuid>,
}

pub async fn apply(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
    trigger: Trigger,
) -> DbResult<Applied> {
    let Some(seat) = Seat::organization(connection, organization_id, user_id).await? else {
        return Ok(Applied::Skipped);
    };
    let role = parse(&seat.role)?;
    if role == OrgRole::Owner || !linked(connection, organization_id, user_id).await? {
        return Ok(Applied::Skipped);
    }

    let groups: Vec<MappedGroup> = sqlx::query_as(
        r#"
        SELECT g.role, g.custom_role_id
        FROM scim_group_members m
        INNER JOIN scim_groups g ON g.id = m.group_id
        WHERE g.organization_id = $1 AND m.user_id = $2
          AND (g.role IS NOT NULL OR g.custom_role_id IS NOT NULL)
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_all(&mut *connection)
    .await?;

    let mut built_in = BTreeSet::new();
    let mut custom = BTreeSet::new();
    for group in groups {
        match (group.role, group.custom_role_id) {
            (Some(role), _) => match parse(&role)? {
                mapped @ (OrgRole::Admin | OrgRole::Member) => {
                    built_in.insert(mapped);
                }
                OrgRole::Owner => {}
            },
            (None, Some(id)) => {
                custom.insert(id);
            }
            (None, None) => {}
        }
    }

    let held = lock_roles(
        connection,
        organization_id,
        custom.iter().copied().chain(seat.custom_role_id),
    )
    .await?;

    let candidates: Vec<Candidate> = built_in
        .into_iter()
        .map(Candidate::BuiltIn)
        .chain(custom.iter().filter_map(|id| {
            held.get(id)
                .and_then(Option::as_ref)
                .map(|(role, grant)| Candidate::Custom {
                    role: role.clone(),
                    grant: grant.clone(),
                })
        }))
        .collect();

    let target = match precedence::choose(&candidates) {
        Some(winner) => winner.clone(),
        None if governed(connection, organization_id, trigger).await? => {
            Candidate::BuiltIn(OrgRole::Member)
        }
        None => return Ok(Applied::Skipped),
    };

    let to = target.membership();
    let assigned = to.custom_role.as_ref().map(|summary| summary.id);
    if !seat.is_active {
        suspend_as(connection, organization_id, user_id, to.role, assigned).await?;
        return Ok(Applied::Skipped);
    }
    if role == to.role && seat.custom_role_id == assigned {
        return Ok(Applied::Unchanged);
    }

    let from = match seat.custom_role_id {
        None => Membership {
            role,
            custom_role: None,
            grant: Grant::organization(role),
        },
        Some(id) => match held.get(&id).and_then(Option::as_ref) {
            Some((summary, grant)) => Membership {
                role,
                custom_role: Some(summary.clone()),
                grant: grant.clone(),
            },
            None => Membership {
                role,
                custom_role: None,
                grant: Grant::default(),
            },
        },
    };

    sqlx::query(
        "UPDATE organization_members SET role = $3, custom_role_id = $4, updated_at = NOW() \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(to.role.as_str())
    .bind(assigned)
    .execute(&mut *connection)
    .await?;

    Ok(Applied::Changed { from, to })
}

pub async fn apply_group(
    connection: &mut PgConnection,
    organization_id: Uuid,
    group_id: Uuid,
    trigger: Trigger,
) -> DbResult<Vec<Outcome>> {
    let users = group_members::users(&mut *connection, organization_id, group_id).await?;
    apply_users(connection, organization_id, &users, trigger).await
}

pub async fn apply_users(
    connection: &mut PgConnection,
    organization_id: Uuid,
    users: &[Uuid],
    trigger: Trigger,
) -> DbResult<Vec<Outcome>> {
    let users = lock(connection, organization_id, users).await?;
    let mut outcomes = Vec::with_capacity(users.len());
    for user_id in users {
        let applied = apply(connection, organization_id, user_id, trigger).await?;
        outcomes.push(Outcome { user_id, applied });
    }
    Ok(outcomes)
}

/// Locks the member rows of `users` by ascending user, and must come before [`apply`] share-locks
/// any custom role: a role edit share-locks its editor's row before the role, so the other order
/// deadlocks with it.
pub async fn lock(
    connection: &mut PgConnection,
    organization_id: Uuid,
    users: &[Uuid],
) -> DbResult<Vec<Uuid>> {
    let users: Vec<Uuid> = users
        .iter()
        .copied()
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect();
    sqlx::query(
        "SELECT user_id FROM organization_members \
         WHERE organization_id = $1 AND user_id = ANY($2) ORDER BY user_id FOR UPDATE",
    )
    .bind(organization_id)
    .bind(&users)
    .execute(&mut *connection)
    .await?;
    Ok(users)
}

fn parse(role: &str) -> DbResult<OrgRole> {
    role.parse().map_err(|_| sqlx::Error::ColumnDecode {
        index: ROLE_COLUMN.to_owned(),
        source: format!("{role} is not an organization role").into(),
    })
}

async fn linked(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM scim_users WHERE organization_id = $1 AND user_id = $2)",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_one(connection)
    .await
}

async fn suspend_as(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrgRole,
    custom_role_id: Option<Uuid>,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE organization_suspensions SET role = $3, custom_role_id = $4 \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(role.as_str())
    .bind(custom_role_id)
    .execute(connection)
    .await?;
    Ok(())
}

async fn governed(
    connection: &mut PgConnection,
    organization_id: Uuid,
    trigger: Trigger,
) -> DbResult<bool> {
    match trigger {
        Trigger::Mapping => Ok(true),
        Trigger::Membership | Trigger::Reactivation => {
            sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM scim_groups WHERE organization_id = $1 \
             AND (role IS NOT NULL OR custom_role_id IS NOT NULL))",
            )
            .bind(organization_id)
            .fetch_one(connection)
            .await
        }
    }
}

async fn lock_roles(
    connection: &mut PgConnection,
    organization_id: Uuid,
    roles: impl Iterator<Item = Uuid>,
) -> DbResult<BTreeMap<Uuid, Option<(Summary, Grant)>>> {
    let mut held = BTreeMap::new();
    for id in roles.collect::<BTreeSet<Uuid>>() {
        let role = custom_roles::lock(connection, organization_id, id, Scope::Organization).await?;
        held.insert(id, role);
    }
    Ok(held)
}
