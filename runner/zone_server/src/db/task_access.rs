//! Task-run snapshots disclosed only to current workspace members.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::auth::Permission;

use super::{DbResult, tasks, workspace_members};

/// A run and its ordered logs from one authorized read.
pub struct Snapshot {
    pub run: tasks::TaskRunRow,
    pub logs: Vec<tasks::TaskRunLogRow>,
}

/// Read run state and logs while preventing membership changes between the two.
///
/// A revocation or demotion that already holds the member row wins before any
/// data is read. Reading terminal state before logs includes every previously
/// awaited receipt.
pub async fn read(pool: &PgPool, run: Uuid, actor: Uuid) -> DbResult<Option<Snapshot>> {
    let mut transaction = pool.begin().await?;
    if !permits(&mut transaction, run, actor, Permission::WorkspaceRead).await? {
        return Ok(None);
    }
    let Some(run) = tasks::get_task_run(&mut *transaction, run).await? else {
        return Ok(None);
    };
    let logs = tasks::get_task_run_logs(&mut *transaction, run.id).await?;
    transaction.commit().await?;
    Ok(Some(Snapshot { run, logs }))
}

/// Authorize a writer against a run and return the run alone.
///
/// A viewer may read a run and must never answer for it, so this needs
/// `workspace:write` where [`read`] needs `workspace:read`. The logs are left
/// unread: a caller answering a parked run needs `pending_question`, and
/// nothing else the run has said.
pub async fn write(pool: &PgPool, run: Uuid, actor: Uuid) -> DbResult<Option<tasks::TaskRunRow>> {
    let mut transaction = pool.begin().await?;
    if !permits(&mut transaction, run, actor, Permission::WorkspaceWrite).await? {
        return Ok(None);
    }
    let run = tasks::get_task_run(&mut *transaction, run).await?;
    transaction.commit().await?;
    Ok(run)
}

/// Whether `actor` holds `permission` in the workspace `run` belongs to. The
/// member row is share-locked before it is judged, so a change that holds it
/// first is waited out and its committed role, not the one this statement
/// began with, decides.
async fn permits(
    connection: &mut PgConnection,
    run: Uuid,
    actor: Uuid,
    permission: Permission,
) -> DbResult<bool> {
    let workspace: Option<Uuid> = sqlx::query_scalar(
        "SELECT tasks.workspace_id FROM task_runs runs \
         JOIN tasks ON tasks.id = runs.task_id WHERE runs.id = $1",
    )
    .bind(run)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(workspace) = workspace else {
        return Ok(false);
    };
    Ok(
        workspace_members::lock_membership(connection, workspace, actor)
            .await?
            .is_some_and(|membership| membership.grant.allows(permission)),
    )
}
