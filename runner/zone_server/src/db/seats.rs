use chrono::{DateTime, Utc};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

use super::DbResult;

macro_rules! membership {
    () => {
        "u.is_active IS NOT FALSE
           AND EXISTS (
               SELECT 1 FROM organization_members m
               JOIN organizations o ON o.id = m.organization_id
               WHERE m.user_id = u.id AND m.is_active AND o.is_active IS NOT FALSE)"
    };
}

/// Revoked sessions still count: a session revoked this period shows its user
/// was active in it.
pub async fn used<'e, E>(executor: E, since: DateTime<Utc>) -> DbResult<u32>
where
    E: Executor<'e, Database = Postgres>,
{
    let count: i64 = sqlx::query_scalar(concat!(
        "SELECT COUNT(*) FROM users u
         WHERE ",
        membership!(),
        "
           AND (u.last_login_at >= ($1 AT TIME ZONE 'UTC')
                OR u.id IN (SELECT s.user_id FROM sessions s WHERE s.last_active_at >= $1))"
    ))
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Whether `user` is an active user with an active membership of an active
/// organization, whenever they last signed in.
pub async fn member<'e, E>(executor: E, user: Uuid) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM users u WHERE u.id = $1 AND ",
        membership!(),
        ")"
    ))
    .bind(user)
    .fetch_one(executor)
    .await
}
