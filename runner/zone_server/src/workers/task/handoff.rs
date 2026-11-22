use chrono::TimeDelta;
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

/// How long a run left for another runner may go unclaimed, counted from its admission, before
/// the runner that refused it fails it as one no runner took. It is well inside the minute after
/// which the sweeper would fail the run as orphaned, and well past the pickup interval.
pub(crate) const DEADLINE: TimeDelta = TimeDelta::seconds(30);

/// Leave a run this runner may not take to another runner that may, or fail it naming what a
/// runner would need. A run left for another runner is failed the same way if none claims it by
/// [`DEADLINE`]. Every fail is fenced on the run being unclaimed, so a runner that picked it up
/// meanwhile keeps it.
pub(crate) async fn hand_off(
    state: &AppState,
    run: &TaskRunRow,
    claimant: Claimant,
    shortfall: Shortfall,
) {
    let code = refusal::of(shortfall.clone()).code();
    match eligibility::exists(state.db(), run.id, claimant.runner).await {
        Ok(true) => {
            tracing::info!(run = %run.id, code, "This runner may not take the task run, so it is left for another runner");
            watch(state.clone(), run.clone(), shortfall);
        }
        Ok(false) => fail(state, run, &shortfall).await,
        Err(error) => {
            tracing::warn!(run = %run.id, code, %error, "Could not look for another runner, so the task run is left for one to pick up");
            watch(state.clone(), run.clone(), shortfall);
        }
    }
}

/// Fail `run` once it has gone unclaimed for [`DEADLINE`], unless a runner claims it first: the
/// runners that could take it may have gone, or be too busy to queue it.
fn watch(state: AppState, run: TaskRunRow, shortfall: Shortfall) {
    tokio::spawn(async move {
        match eligibility::unclaimed_for(state.db(), run.id, DEADLINE).await {
            Ok(Some(wait)) => tokio::time::sleep(wait).await,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(run = %run.id, %error, "Could not check on the task run left for another runner; the sweeper fails it if none claims it");
                return;
            }
        }
        fail(&state, &run, &shortfall).await;
    });
}

async fn fail(state: &AppState, run: &TaskRunRow, shortfall: &Shortfall) {
    let pool = state.db();
    let workspace = match workspace(pool, run.task_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(run = %run.id, %error, "Could not read the task run's workspace, so it is left for a runner to pick up");
            return;
        }
    };
    let refusal = match no_runner(pool, workspace.id).await {
        Ok(refusal) => refusal,
        Err(error) => {
            tracing::warn!(run = %run.id, %error, "Could not read what the task run needs of a runner, so it is left for one to pick up");
            return;
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
            let refused = refusal::of(shortfall.clone());
            tracing::warn!(
                run = %run.id,
                code = refusal.code(),
                refused = refused.code(),
                reason = %refused,
                "No runner in the fleet took the task run, so it failed"
            );
            audit(pool, &workspace, run.task_id, &refusal).await;
            crate::workers::auto_project::poke_task(state, run.task_id);
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(run = %run.id, %error, "Could not fail the task run no runner can take; the sweeper will");
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
