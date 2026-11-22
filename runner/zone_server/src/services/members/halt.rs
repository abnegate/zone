use uuid::Uuid;

use super::Halted;
use crate::db::DbResult;
use crate::db::{invitations, tasks, users, workspaces};
use crate::state::AppState;

/// Stops what a member who left an organization still has running in it:
/// every chat turn they started in its workspaces, reminder and automation
/// turns included, every task run they triggered there, and every invitation
/// to it still waiting on their email.
///
/// Their access is not touched here; the caller has already suspended or
/// removed it, so nothing they start next gets past a membership check.
pub async fn halt(state: &AppState, organization_id: Uuid, user_id: Uuid) -> DbResult<Halted> {
    let workspaces: Vec<Uuid> = workspaces::list_workspaces(state.db(), organization_id)
        .await?
        .into_iter()
        .map(|workspace| workspace.id)
        .collect();

    let turns = crate::ws::chat::cancel_user_turns(user_id, &workspaces);

    let mut runs = 0;
    for run_id in tasks::live_runs_triggered_by(state.db(), user_id, &workspaces).await? {
        if let tasks::Mutation::Applied(tasks::Stop::Stopped(run)) =
            tasks::stop_task_run(state.db(), run_id).await?
        {
            crate::workers::task::stop::request(run.id);
            crate::workers::auto_project::poke_task(state, run.task_id);
            runs += 1;
        }
    }

    let invitations = match users::get_user_by_id(state.db(), user_id).await? {
        Some(user) => invitations::revoke_pending(state.db(), organization_id, &user.email).await?,
        None => 0,
    };

    Ok(Halted {
        turns,
        runs,
        invitations,
    })
}
