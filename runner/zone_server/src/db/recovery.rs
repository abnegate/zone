//! Reconcile committed terminal runs with their owning task pointers.

use sqlx::PgPool;
use uuid::Uuid;

/// Match normal completion's task-before-run lock order, preserving the result.
pub async fn reconcile(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let tasks: Vec<Uuid> = sqlx::query_scalar(
        "SELECT tasks.id FROM tasks JOIN task_runs ON task_runs.id=tasks.active_run_id WHERE task_runs.status IN ('completed','failed','cancelled') ORDER BY tasks.id FOR UPDATE OF tasks",
    ).fetch_all(&mut *transaction).await?;
    let mut changed = 0;
    for task in tasks {
        sqlx::query("SELECT task_runs.id FROM task_runs JOIN tasks ON tasks.active_run_id=task_runs.id WHERE tasks.id=$1 FOR UPDATE OF task_runs")
            .bind(task).execute(&mut *transaction).await?;
        changed += sqlx::query("UPDATE tasks SET active_run_id=NULL,status=CASE WHEN task_runs.status='completed' THEN 'review' ELSE 'blocked' END,completed_at=COALESCE(task_runs.completed_at,NOW()),updated_at=NOW() FROM task_runs WHERE tasks.id=$1 AND tasks.active_run_id=task_runs.id AND task_runs.task_id=tasks.id AND task_runs.status IN ('completed','failed','cancelled')")
            .bind(task).execute(&mut *transaction).await?.rows_affected();
    }
    transaction.commit().await?;
    Ok(changed)
}
