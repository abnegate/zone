//! Task-run snapshots disclosed only to current workspace members.

use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{DbResult, tasks};

/// A run and its ordered logs from one authorized read.
pub struct Snapshot {
    pub run: tasks::TaskRunRow,
    pub logs: Vec<tasks::TaskRunLogRow>,
}

/// Read run state and logs while preventing membership changes between the two.
///
/// A revocation that already holds the member row wins before any data is read.
/// Reading terminal state before logs includes every previously awaited receipt.
pub async fn read(pool: &PgPool, run: Uuid, actor: Uuid) -> DbResult<Option<Snapshot>> {
    let mut transaction = pool.begin().await?;
    let member: Option<Uuid> = sqlx::query_scalar(
        "SELECT members.user_id FROM task_runs runs JOIN tasks ON tasks.id = runs.task_id JOIN workspace_members members ON members.workspace_id = tasks.workspace_id WHERE runs.id = $1 AND members.user_id = $2 AND members.is_active AND members.role IN ('owner', 'admin', 'member', 'viewer') FOR SHARE OF members",
    )
    .bind(run)
    .bind(actor)
    .fetch_optional(&mut *transaction)
    .await?;
    if member.is_none() {
        return Ok(None);
    }
    let Some(row) = sqlx::query(
        "SELECT id, task_id, status, current_phase, progress_percent, started_at, completed_at, error_message, artifacts, triggered_by FROM task_runs WHERE id = $1",
    )
    .bind(run)
    .fetch_optional(&mut *transaction)
    .await? else {
        return Ok(None);
    };
    let run = tasks::TaskRunRow {
        id: row.try_get("id")?,
        task_id: row.try_get("task_id")?,
        triggered_by: row.try_get("triggered_by")?,
        status: row.try_get("status")?,
        current_phase: row.try_get("current_phase")?,
        progress_percent: row.try_get("progress_percent")?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        error_message: row.try_get("error_message")?,
        artifacts: row.try_get("artifacts")?,
    };
    let logs = sqlx::query(
        "SELECT id, task_run_id, phase, agent_type, log_level, message, metadata, created_at FROM task_run_logs WHERE task_run_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(run.id)
    .fetch_all(&mut *transaction)
    .await?
    .into_iter()
    .map(|row| {
        Ok(tasks::TaskRunLogRow {
            id: row.try_get("id")?,
            task_run_id: row.try_get("task_run_id")?,
            phase: row.try_get("phase")?,
            agent_type: row.try_get("agent_type")?,
            log_level: row.try_get("log_level")?,
            message: row.try_get("message")?,
            metadata: row.try_get("metadata")?,
            created_at: row.try_get("created_at")?,
        })
    })
    .collect::<DbResult<Vec<_>>>()?;
    transaction.commit().await?;
    Ok(Some(Snapshot { run, logs }))
}
