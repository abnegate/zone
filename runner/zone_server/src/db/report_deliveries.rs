//! The last digest slot each workspace was sent, shared by every replica.

use chrono::NaiveDateTime;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;

/// The slot last delivered to `workspace_id`, recording `seed` first if the workspace has none.
///
/// Two statements rather than one: a seed another session commits while the insert waits on it
/// is invisible to the rest of that same statement's snapshot.
pub async fn last(
    pool: &PgPool,
    workspace_id: Uuid,
    seed: NaiveDateTime,
) -> DbResult<NaiveDateTime> {
    sqlx::query(
        "INSERT INTO report_deliveries (workspace_id, slot) VALUES ($1, $2)
         ON CONFLICT (workspace_id) DO NOTHING",
    )
    .bind(workspace_id)
    .bind(seed)
    .execute(pool)
    .await?;
    sqlx::query_scalar("SELECT slot FROM report_deliveries WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(pool)
        .await
}

/// Move the workspace from `from` to `to`, and say whether this caller was the one that did.
pub async fn advance<'e, E>(
    executor: E,
    workspace_id: Uuid,
    from: NaiveDateTime,
    to: NaiveDateTime,
) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let moved =
        sqlx::query("UPDATE report_deliveries SET slot = $3 WHERE workspace_id = $1 AND slot = $2")
            .bind(workspace_id)
            .bind(from)
            .bind(to)
            .execute(executor)
            .await?;
    Ok(moved.rows_affected() == 1)
}
