//! Reconcile committed terminal runs with their owning task pointers.

use sqlx::PgPool;
use uuid::Uuid;

use super::tasks::DRAINING;

/// Bound each pass so a backlog cannot hold task locks for the whole loop.
const BATCH: i64 = 100;

/// Match normal completion's task-before-run lock order, preserving the result.
///
/// A stopped run keeps its task reserved while its owner drains, and is
/// released here only once that owner's lease lapses; the stop already
/// paused the task, so its status is left as the stop wrote it.
pub async fn reconcile(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let tasks: Vec<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT t.id FROM tasks t JOIN task_runs r ON r.id=t.active_run_id WHERE r.status IN ('completed','failed','cancelled') AND NOT ({DRAINING}) ORDER BY t.id LIMIT $1 FOR UPDATE OF t SKIP LOCKED",
    ))).bind(BATCH).fetch_all(&mut *transaction).await?;
    let mut changed = 0;
    for task in tasks {
        sqlx::query("SELECT task_runs.id FROM task_runs JOIN tasks ON tasks.active_run_id=task_runs.id WHERE tasks.id=$1 FOR UPDATE OF task_runs")
            .bind(task).execute(&mut *transaction).await?;
        changed += sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE tasks t SET active_run_id=NULL,status=CASE r.status WHEN 'completed' THEN 'review' WHEN 'cancelled' THEN t.status ELSE 'blocked' END,completed_at=CASE WHEN r.status='cancelled' THEN t.completed_at ELSE COALESCE(r.completed_at,NOW()) END,updated_at=NOW() FROM task_runs r WHERE t.id=$1 AND t.active_run_id=r.id AND r.task_id=t.id AND r.status IN ('completed','failed','cancelled') AND NOT ({DRAINING})")))
            .bind(task).execute(&mut *transaction).await?.rows_affected();
    }
    transaction.commit().await?;
    Ok(changed)
}
