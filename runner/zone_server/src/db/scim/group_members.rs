use std::collections::BTreeSet;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::admission::Admission;
use super::group_member::GroupMember;
use crate::db::DbResult;

pub async fn list(executor: impl PgExecutor<'_>, group_id: Uuid) -> DbResult<Vec<GroupMember>> {
    sqlx::query_as(
        r#"
        SELECT m.user_id, COALESCE(s.display_name, u.display_name, s.user_name, u.email) AS display
        FROM scim_group_members m
        INNER JOIN scim_groups g ON g.id = m.group_id
        INNER JOIN users u ON u.id = m.user_id
        LEFT JOIN scim_users s ON s.organization_id = g.organization_id AND s.user_id = m.user_id
        WHERE m.group_id = $1
        ORDER BY m.created_at, m.user_id
        "#,
    )
    .bind(group_id)
    .fetch_all(executor)
    .await
}

pub async fn add(
    connection: &mut PgConnection,
    organization_id: Uuid,
    group_id: Uuid,
    users: &[Uuid],
) -> DbResult<Admission> {
    lock_group(connection, organization_id, group_id).await?;
    let users = distinct(users);
    let strangers = strangers(connection, organization_id, &users).await?;
    if !strangers.is_empty() {
        return Ok(Admission::Strangers(strangers));
    }
    let added = insert(connection, group_id, &users).await?;
    Ok(Admission::Admitted(added.into_iter().collect()))
}

pub async fn remove(
    connection: &mut PgConnection,
    organization_id: Uuid,
    group_id: Uuid,
    users: &[Uuid],
) -> DbResult<Vec<Uuid>> {
    lock_group(connection, organization_id, group_id).await?;
    let removed: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM scim_group_members WHERE group_id = $1 AND user_id = ANY($2) \
         RETURNING user_id",
    )
    .bind(group_id)
    .bind(distinct(users))
    .fetch_all(&mut *connection)
    .await?;
    Ok(removed
        .into_iter()
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect())
}

pub async fn replace(
    connection: &mut PgConnection,
    organization_id: Uuid,
    group_id: Uuid,
    users: &[Uuid],
) -> DbResult<Admission> {
    lock_group(connection, organization_id, group_id).await?;
    let users = distinct(users);
    let strangers = strangers(connection, organization_id, &users).await?;
    if !strangers.is_empty() {
        return Ok(Admission::Strangers(strangers));
    }
    let removed: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM scim_group_members WHERE group_id = $1 AND NOT (user_id = ANY($2)) \
         RETURNING user_id",
    )
    .bind(group_id)
    .bind(&users)
    .fetch_all(&mut *connection)
    .await?;
    let mut affected = insert(connection, group_id, &users).await?;
    affected.extend(removed);
    Ok(Admission::Admitted(affected.into_iter().collect()))
}

fn distinct(users: &[Uuid]) -> Vec<Uuid> {
    users
        .iter()
        .copied()
        .collect::<BTreeSet<Uuid>>()
        .into_iter()
        .collect()
}

async fn lock_group(
    connection: &mut PgConnection,
    organization_id: Uuid,
    group_id: Uuid,
) -> DbResult<()> {
    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM scim_groups WHERE organization_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(group_id)
    .fetch_optional(connection)
    .await?;
    locked.map(|_| ()).ok_or(sqlx::Error::RowNotFound)
}

async fn strangers(
    connection: &mut PgConnection,
    organization_id: Uuid,
    users: &[Uuid],
) -> DbResult<Vec<Uuid>> {
    let members: BTreeSet<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM organization_members \
         WHERE organization_id = $1 AND user_id = ANY($2) ORDER BY user_id FOR SHARE",
    )
    .bind(organization_id)
    .bind(users)
    .fetch_all(connection)
    .await?
    .into_iter()
    .collect();
    Ok(users
        .iter()
        .copied()
        .filter(|user| !members.contains(user))
        .collect())
}

async fn insert(
    connection: &mut PgConnection,
    group_id: Uuid,
    users: &[Uuid],
) -> DbResult<BTreeSet<Uuid>> {
    let added: Vec<Uuid> = sqlx::query_scalar(
        "INSERT INTO scim_group_members (group_id, user_id) SELECT $1, unnest($2::uuid[]) \
         ON CONFLICT DO NOTHING RETURNING user_id",
    )
    .bind(group_id)
    .bind(users)
    .fetch_all(connection)
    .await?;
    Ok(added.into_iter().collect())
}
