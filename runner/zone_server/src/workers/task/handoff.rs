use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::fleet::Refusal;

use crate::db::DbResult;
use crate::db::audit::{AuditContext, actions, log_action, resources};
use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::{eligibility, labels, policy};
use crate::db::tasks::{self, TaskRunRow};
use crate::db::workspaces::{self, WorkspaceRow};
use crate::services::fleet::refusal;
use crate::state::AppState;

/// What became of a task run this runner was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Handoff {
    /// Another connected runner may take it, so it waits to be picked up.
    Left,
    /// No runner could take it, so it failed with this refusal.
    Failed(Refusal),
    /// It ended, or another runner claimed it, before it could fail.
    Taken,
}

/// Leave a run this runner may not take to another runner that may, or fail it naming what a
/// runner would need. The fail is fenced on the run being unclaimed, so a runner that picked it
/// up meanwhile keeps it.
pub(crate) async fn hand_off(
    state: &AppState,
    run: &TaskRunRow,
    claimant: Claimant,
    shortfall: Shortfall,
) -> Handoff {
    let pool = state.db();
    let code = refusal::of(shortfall).code();
    match eligibility::exists(pool, run.id, claimant.runner).await {
        Ok(true) => {
            tracing::info!(run = %run.id, code, "This runner may not take the task run, so it is left for another runner");
            return Handoff::Left;
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(run = %run.id, code, %error, "Could not look for another runner, so the task run is left for one to pick up");
            return Handoff::Left;
        }
    }
    let workspace = match workspace(pool, run.task_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return Handoff::Taken,
        Err(error) => {
            tracing::warn!(run = %run.id, %error, "Could not read the task run's workspace, so it is left for a runner to pick up");
            return Handoff::Left;
        }
    };
    let refusal = match no_runner(pool, workspace.id).await {
        Ok(refusal) => refusal,
        Err(error) => {
            tracing::warn!(run = %run.id, %error, "Could not read what the task run needs of a runner, so it is left for one to pick up");
            return Handoff::Left;
        }
    };
    let failed = tasks::complete_owned_task_run(
        pool,
        run.id,
        None,
        tasks::FAILED,
        Some(&refusal.message()),
        None,
    )
    .await;
    match failed {
        Ok(Some(_)) => {
            tracing::warn!(run = %run.id, code = refusal.code(), "No runner in the fleet can take the task run, so it failed");
            audit(pool, &workspace, run.task_id, &refusal).await;
            Handoff::Failed(refusal)
        }
        Ok(None) => Handoff::Taken,
        Err(error) => {
            tracing::warn!(run = %run.id, %error, "Could not fail the task run no runner can take; the sweeper will");
            Handoff::Left
        }
    }
}

async fn workspace(pool: &PgPool, task: Uuid) -> DbResult<Option<WorkspaceRow>> {
    match tasks::get_task(pool, task).await? {
        Some(task) => workspaces::get_workspace(pool, task.workspace_id).await,
        None => Ok(None),
    }
}

async fn no_runner(pool: &PgPool, workspace: Uuid) -> DbResult<Refusal> {
    let minimum = policy::get(pool).await?.minimum;
    Ok(Refusal::NoRunner {
        minimum: minimum.map(|minimum| minimum.version().clone()),
        labels: labels::required(pool, workspace).await?,
    })
}

async fn audit(pool: &PgPool, workspace: &WorkspaceRow, task: Uuid, refusal: &Refusal) {
    let Refusal::NoRunner { minimum, labels } = refusal else {
        return;
    };
    let context = AuditContext {
        org_id: Some(workspace.organization_id),
        workspace_id: Some(workspace.id),
        actor_id: None,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    if let Err(error) = log_action(
        pool,
        &context,
        actions::FLEET_RUN_REFUSED,
        resources::TASK,
        Some(task),
        None,
        Some(json!({
            "code": refusal.code(),
            "minimum": minimum.as_ref().map(ToString::to_string),
            "labels": labels.to_vec(),
        })),
    )
    .await
    {
        tracing::error!(%error, action = actions::FLEET_RUN_REFUSED, "Failed to record audit log");
    }
}
