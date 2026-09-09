//! Task-run snapshots disclosed only to current workspace members.

use sqlx::PgPool;
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
    let Some(run) = tasks::get_task_run(&mut *transaction, run).await? else {
        return Ok(None);
    };
    let logs = tasks::get_task_run_logs(&mut *transaction, run.id).await?;
    transaction.commit().await?;
    Ok(Some(Snapshot { run, logs }))
}
