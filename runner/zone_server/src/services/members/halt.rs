use uuid::Uuid;

use super::Halted;
use crate::db::DbResult;
use crate::db::{tasks, workspaces};
use crate::state::AppState;

pub async fn halt(state: &AppState, organization_id: Uuid, user_id: Uuid) -> DbResult<Halted> {
    let workspaces: Vec<Uuid> = workspaces::list_workspaces(state.db(), organization_id)
        .await?
        .into_iter()
        .map(|workspace| workspace.id)
        .collect();

    let turns = crate::ws::chat::cancel_user_turns(user_id, &workspaces);

    let mut runs = 0;
    for run_id in tasks::live_runs_triggered_by(state.db(), user_id, &workspaces).await? {
        match tasks::stop_task_run(state.db(), run_id).await {
            Ok(tasks::Mutation::Applied(tasks::Stop::Stopped(run))) => {
                crate::workers::task::stop::request(run.id);
                crate::workers::auto_project::poke_task(state, run.task_id);
                runs += 1;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, %run_id, %user_id, "Failed to stop a departed member's task run");
            }
        }
    }

    Ok(Halted { turns, runs })
}
