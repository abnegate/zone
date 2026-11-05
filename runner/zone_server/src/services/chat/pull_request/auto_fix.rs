//! Starting a run that fixes a pull request the chat is watching.
//!
//! Auto-fix is zone's own preference rather than something GitHub holds. What
//! acts on it is a status read: whenever a fresh status says the pull request
//! is failing or cannot merge, the row is claimed for a run and that run is
//! asked to fix it. The claim is what keeps a read that repeats every poll
//! from starting the same run over and over.

use uuid::Uuid;

use super::checks::Checks;
use super::mergeable::Mergeable;
use super::status::Status;
use crate::db::chat_pull_requests::{self, PullRequestRow};
use crate::db::tasks;
use crate::state::AppState;

/// Why a pull request wants fixing, in the words the run is asked to act on.
fn complaint(status: &Status) -> Option<&'static str> {
    if !status.open() {
        return None;
    }
    match (status.checks, status.mergeable) {
        // A conflict outranks a failing check: the checks that ran did so
        // against a merge that no longer applies, so the conflict is first.
        (_, Mergeable::Conflicted) => Some("it conflicts with its base branch"),
        (Checks::Failing, _) => Some("its checks are failing"),
        // Pending, passing, absent or unreadable checks on a mergeable branch
        // are not a complaint. Pending especially: a run started here would be
        // fixing a verdict that has not arrived.
        _ => None,
    }
}

/// Starts a run to fix `row` when its fresh `status` asks for one and nothing
/// is already answering for that state.
///
/// Never fails the status read that called it. A pull request that could not
/// be fixed this time is read again on the next poll, and the complaint is
/// still there if it was real.
pub async fn consider(state: &AppState, workspace_id: Uuid, row: &PullRequestRow, status: &Status) {
    if !row.auto_fix {
        return;
    }
    let Some(complaint) = complaint(status) else {
        // Nothing to fix. Release the row so the next thing that goes wrong
        // is not mistaken for the one this run already answered.
        if row.auto_fix_run_id.is_some()
            && let Err(error) =
                chat_pull_requests::claim_auto_fix(state.db(), row.id, row.auto_fix_run_id, None)
                    .await
        {
            tracing::warn!(pull_request = %row.id, %error, "could not release an auto-fix claim");
        }
        return;
    };
    if row.auto_fix_run_id.is_some() {
        // A run is already answering for this pull request. Whether it is
        // still going or has finished without fixing anything, starting a
        // second one on the same complaint stacks runs on one branch.
        return;
    }
    if let Err(error) = start(state, workspace_id, row, status, complaint).await {
        tracing::warn!(pull_request = %row.id, %error, "could not start an auto-fix run");
    }
}

async fn start(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
    status: &Status,
    complaint: &str,
) -> Result<(), sqlx::Error> {
    let title = format!("Fix {}/{} #{}", row.owner, row.repo, row.number);
    let description = prompt(row, status, complaint);
    let task = tasks::create_task(
        state.db(),
        workspace_id,
        &[],
        &title,
        &description,
        Some(ACCEPTANCE),
        None,
        true,
        row.source_id,
    )
    .await?;
    let run = tasks::create_task_run(state.db(), task.id).await?;
    // Claimed only after the run exists, and only while the row still holds
    // what this caller saw. A reader that lost the race leaves a task nothing
    // points at rather than a second run on the branch.
    if !chat_pull_requests::claim_auto_fix(state.db(), row.id, row.auto_fix_run_id, Some(run.id))
        .await?
    {
        tracing::info!(
            pull_request = %row.id,
            "another reader claimed this pull request first; the run it started stands"
        );
    }
    Ok(())
}

const ACCEPTANCE: &str = "The pull request's checks pass, it merges without conflicts, and every \
     unresolved review comment is either addressed in the branch or answered saying why it was \
     not. No existing test is weakened or deleted to make a check pass.";

fn prompt(row: &PullRequestRow, status: &Status, complaint: &str) -> String {
    format!(
        "# Fix pull request #{number}\n\n\
         {url}\n\n\
         This pull request is set to fix itself, and {complaint}.\n\n\
         Check out its head branch and do all of the following that apply:\n\n\
         - Make the failing checks pass. Read the failing job's log for the real cause rather \
           than guessing from the job name.\n\
         - Resolve any conflict with the base branch.\n\
         - Address every unresolved review comment, from a person or a review bot. Where you \
           disagree with one, reply saying why instead of changing the code.\n\n\
         Push to the same branch. Do not open a second pull request, and do not merge this one. \
         Do not weaken or delete a test to make a check pass: a check that fails because the code \
         is wrong is fixed by fixing the code.\n\n\
         What GitHub last reported: checks {checks:?}, mergeability {mergeable:?}.",
        number = row.number,
        url = row.url,
        complaint = complaint,
        checks = status.checks,
        mergeable = status.mergeable,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn status(checks: Checks, mergeable: Mergeable) -> Status {
        Status {
            title: "A change".to_string(),
            state: super::super::state::State::Open,
            draft: false,
            additions: 1,
            deletions: 0,
            changed_files: 1,
            checks,
            mergeable,
            auto_merge: super::super::auto_merge::AutoMerge {
                enabled: false,
                unavailable: None,
            },
            draft_unavailable: None,
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn a_failing_check_is_a_complaint() {
        assert_eq!(
            complaint(&status(Checks::Failing, Mergeable::Unstable)),
            Some("its checks are failing")
        );
    }

    #[test]
    fn a_conflict_outranks_a_failing_check() {
        assert_eq!(
            complaint(&status(Checks::Failing, Mergeable::Conflicted)),
            Some("it conflicts with its base branch")
        );
    }

    #[test]
    fn a_pending_check_is_not_yet_a_complaint() {
        assert_eq!(complaint(&status(Checks::Pending, Mergeable::Clean)), None);
    }

    #[test]
    fn a_healthy_pull_request_is_left_alone() {
        assert_eq!(complaint(&status(Checks::Passing, Mergeable::Clean)), None);
        assert_eq!(complaint(&status(Checks::None, Mergeable::Clean)), None);
        assert_eq!(complaint(&status(Checks::Unknown, Mergeable::Clean)), None);
    }

    #[test]
    fn a_closed_pull_request_is_never_fixed() {
        let mut closed = status(Checks::Failing, Mergeable::Conflicted);
        closed.state = super::super::state::State::Closed;
        assert_eq!(complaint(&closed), None);
    }
}
