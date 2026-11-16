//! Seat usage: the people who used this server since a moment

use chrono::{DateTime, Utc};
use sqlx::{Executor, Postgres};

use super::DbResult;

/// Revoked sessions still count: a session revoked this period shows its user
/// was active in it.
pub async fn used<'e, E>(executor: E, since: DateTime<Utc>) -> DbResult<u32>
where
    E: Executor<'e, Database = Postgres>,
{
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users u
         WHERE u.is_active IS NOT FALSE
           AND EXISTS (SELECT 1 FROM organization_members m WHERE m.user_id = u.id AND m.is_active)
           AND (u.last_login_at::timestamptz >= $1
                OR EXISTS (SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.last_active_at >= $1))",
    )
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}
