//! The shipped fixes a regression alert already went out for, shared by every replica.

use chrono::NaiveDateTime;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

use super::DbResult;

/// Record an alert for `task_id`, and say whether this caller is the first to.
pub async fn claim<'e, E>(executor: E, task_id: Uuid, at: NaiveDateTime) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let claimed: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO regression_alerts (task_id, alerted_at) VALUES ($1, $2)
         ON CONFLICT (task_id) DO NOTHING
         RETURNING task_id",
    )
    .bind(task_id)
    .bind(at)
    .fetch_optional(executor)
    .await?;
    Ok(claimed.is_some())
}

/// Forget alerts sent before `before`.
pub async fn prune<'e, E>(executor: E, before: NaiveDateTime) -> DbResult<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    let pruned = sqlx::query("DELETE FROM regression_alerts WHERE alerted_at < $1")
        .bind(before)
        .execute(executor)
        .await?;
    Ok(pruned.rows_affected())
}
