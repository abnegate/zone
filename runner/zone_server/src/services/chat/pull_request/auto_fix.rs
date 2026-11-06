//! Starting a run that fixes a pull request the chat is watching.
//!
//! Auto-fix is zone's own preference rather than something GitHub holds. What
//! acts on it is a status read: whenever a fresh status says the pull request
//! is failing or cannot merge and no run of its own is still working, the row
//! is claimed for a new run and that run is set going to fix it. The claim is
//! what keeps a read that repeats every poll from starting the same run over
//! and over.
//!
//! One run is not a promise. A fix that pushes and still fails leaves the
//! pull request broken, so once the claimed run settles and the complaint is
//! still there, another starts. Auto-fix keeps going until the checks pass,
//! and only one run is ever in flight, so it cannot spin faster than a run
//! takes to finish.

use abnegate_vcs::{BranchName, CommitSha, GitService};
use uuid::Uuid;

use super::checks::Checks;
use super::mergeable::Mergeable;
use super::status::Status;
use crate::db::chat_pull_requests::{self, Claim, PullRequestRow};
use crate::db::projects;
use crate::db::tasks::{Create, Origin};
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

/// Starts a run to fix `row` on its head `branch` when its fresh `status` asks
/// for one and no run of its own is still working, and leaves `row` saying
/// what it claims now.
///
/// Never fails the status read that called it. A pull request that could not
/// be fixed this time is read again on the next poll, and the complaint is
/// still there if it was real.
pub async fn consider(
    state: &AppState,
    workspace_id: Uuid,
    row: &mut PullRequestRow,
    status: &Status,
    branch: &BranchName,
    head: &CommitSha,
) {
    // A run still working answers for whatever the branch shows now, and a
    // second run on the same branch would fight it.
    if !row.auto_fix || row.auto_fixing {
        return;
    }
    // A run that settled without moving the head changed nothing, and another
    // started from the same commit would only do the same.
    if row.auto_fix_head.as_deref() == Some(head.as_str()) {
        return;
    }
    let Some(complaint) = complaint(status) else {
        return;
    };
    let Some(actor) = row.auto_fix_by else {
        tracing::warn!(pull_request = %row.id, "auto-fix is on with no one to act as");
        return;
    };
    let project = match project(state, workspace_id, row).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::warn!(
                pull_request = %row.id,
                "no project of the workspace can push to this pull request's repository"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(pull_request = %row.id, %error, "could not read the workspace's projects");
            return;
        }
    };
    let fix = Fix {
        actor,
        project,
        branch,
        head,
        complaint,
    };
    match start(state, workspace_id, row, status, &fix).await {
        Ok(Some(run_id)) => {
            row.auto_fix_run_id = Some(run_id);
            row.auto_fixing = true;
        }
        Ok(None) => {
            tracing::info!(
                pull_request = %row.id,
                "another reader claimed this pull request first; the run it started stands"
            );
            refresh(state, row).await;
        }
        Err(error) => {
            tracing::warn!(pull_request = %row.id, %error, "could not start an auto-fix run");
        }
    }
}

async fn refresh(state: &AppState, row: &mut PullRequestRow) {
    match chat_pull_requests::get(state.db(), row.chat_id, row.id).await {
        Ok(Some(current)) => *row = current,
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(pull_request = %row.id, %error, "could not reread an auto-fix claim");
        }
    }
}

/// The workspace project that holds a push credential for the pull request's
/// repository. A task's checkout reaches a repository only through one.
async fn project(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> Result<Option<Uuid>, sqlx::Error> {
    let Some(wanted) = repository(&format!("https://github.com/{}/{}", row.owner, row.repo)) else {
        return Ok(None);
    };
    Ok(projects::list_projects(state.db(), workspace_id, None)
        .await?
        .into_iter()
        .find(|project| {
            project.github_access_token.is_some()
                && project.github_repo_url.as_deref().and_then(repository) == Some(wanted.clone())
        })
        .map(|project| project.id))
}

fn repository(url: &str) -> Option<String> {
    GitService::repository_url(url)
        .ok()
        .map(|url| url.as_str().to_ascii_lowercase())
}

struct Fix<'a> {
    actor: Uuid,
    project: Uuid,
    branch: &'a BranchName,
    head: &'a CommitSha,
    complaint: &'a str,
}

/// Claims `row` for a new run and sets that run going, answering with its id,
/// or with nothing when another reader or a toggle got there first.
async fn start(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
    status: &Status,
    fix: &Fix<'_>,
) -> Result<Option<Uuid>, sqlx::Error> {
    let title = format!("Fix {}/{} #{}", row.owner, row.repo, row.number);
    let description = prompt(row, status, fix.complaint);
    let project_ids = [fix.project];
    let task = Create {
        workspace_id,
        project_ids: &project_ids,
        title: Some(&title),
        description: &description,
        acceptance_criteria: Some(ACCEPTANCE),
        priority: None,
        is_agentic: true,
        require_plan_approval: false,
        source_id: row.source_id,
        created_by: Some(fix.actor),
        chat_id: Some(row.chat_id),
        origin: Origin::Automation,
    };
    let claim = Claim {
        id: row.id,
        previous: row.auto_fix_run_id,
        branch: fix.branch.as_str(),
        head: fix.head.as_str(),
        url: &row.url,
    };
    let Some((task, run)) = chat_pull_requests::start_auto_fix(state.db(), &claim, &task).await?
    else {
        return Ok(None);
    };
    let dispatched = state.clone();
    tokio::spawn(async move {
        crate::workers::task::execute_task_run(&dispatched, run.id, task.id).await;
    });
    Ok(Some(run.id))
}

const ACCEPTANCE: &str = "The pull request's checks pass, it merges without conflicts, and every \
     unresolved review comment is either addressed in the branch or answered saying why it was \
     not. No existing test is weakened or deleted to make a check pass.";

fn prompt(row: &PullRequestRow, status: &Status, complaint: &str) -> String {
    format!(
        "# Fix pull request #{number}\n\n\
         {url}\n\n\
         This pull request is set to fix itself, and {complaint}.\n\n\
         You are on its head branch already. Do all of the following that apply:\n\n\
         - Make the failing checks pass. Read the failing job's log for the real cause rather \
           than guessing from the job name.\n\
         - Resolve any conflict with the base branch.\n\
         - Address every unresolved review comment, from a person or a review bot. Where you \
           disagree with one, reply saying why instead of changing the code.\n\n\
         Commit your work on this branch; it is pushed to the pull request when you finish. Do \
         not switch branches, do not open a second pull request, and do not merge this one. \
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
    use crate::db::chat_pull_requests::NewPullRequest;
    use crate::db::chats::create_chat;
    use chrono::Utc;
    use sqlx::PgPool;
    use std::time::Duration;

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
            threads: None,
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

    async fn watched(pool: &PgPool, repository: Option<&str>) -> (Uuid, PullRequestRow) {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Auto-fix', $1::text)")
            .bind(organization)
            .execute(pool)
            .await
            .expect("an organization");
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Auto-fix', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(pool)
        .await
        .expect("a workspace");
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name) VALUES ($1, $1::text, 'test-only', 'Fixer')",
        )
        .bind(user)
        .execute(pool)
        .await
        .expect("a user");
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(workspace)
        .bind(user)
        .execute(pool)
        .await
        .expect("a membership");
        sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, github_repo_url, github_access_token)
             VALUES ($1, $2, 'Widgets', $3, 'sealed')",
        )
        .bind(Uuid::new_v4())
        .bind(workspace)
        .bind(repository)
        .execute(pool)
        .await
        .expect("a project");
        let chat = create_chat(pool, None, "Changes", "qwen3:8b", false, true)
            .await
            .expect("a chat");
        let row = chat_pull_requests::record(
            pool,
            &NewPullRequest {
                chat_id: chat.id,
                owner: "acme",
                repo: "widgets",
                number: 7,
                url: "https://github.com/acme/widgets/pull/7",
                title: None,
                source_id: None,
            },
        )
        .await
        .expect("the pull request");
        let row = chat_pull_requests::set_auto_fix(pool, chat.id, row.id, true, user)
            .await
            .expect("the update")
            .expect("the row");
        (workspace, row)
    }

    #[tokio::test]
    async fn a_failing_pull_request_sets_its_fix_run_going_once() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let (workspace, mut row) = watched(&pool, Some("https://github.com/Acme/Widgets")).await;
        let state = AppState::new(AppState::for_tests().config().clone(), pool.clone(), None);
        let failing = status(Checks::Failing, Mergeable::Unstable);
        let branch = BranchName::parse("fix/widgets").expect("a branch");
        let head = CommitSha::parse("0123456789abcdef0123456789abcdef01234567").expect("a head");

        consider(&state, workspace, &mut row, &failing, &branch, &head).await;
        let run = row
            .auto_fix_run_id
            .expect("a failing pull request claims a run");
        assert!(row.auto_fixing, "the row reports the run it just started");

        consider(&state, workspace, &mut row, &failing, &branch, &head).await;
        assert_eq!(
            row.auto_fix_run_id,
            Some(run),
            "a working run is not joined by a second"
        );

        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let dispatched: bool = sqlx::query_scalar(
                    "SELECT owner IS NOT NULL OR completed_at IS NOT NULL FROM task_runs WHERE id = $1",
                )
                .bind(run)
                .fetch_one(&pool)
                .await
                .expect("the run");
                if dispatched {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the claimed run is executed rather than left for recovery to fail");
    }

    #[tokio::test]
    async fn a_repository_no_project_can_push_to_is_left_alone() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let (workspace, mut row) = watched(&pool, Some("https://github.com/acme/gadgets")).await;
        let state = AppState::new(AppState::for_tests().config().clone(), pool.clone(), None);
        let branch = BranchName::parse("fix/widgets").expect("a branch");
        let head = CommitSha::parse("0123456789abcdef0123456789abcdef01234567").expect("a head");

        consider(
            &state,
            workspace,
            &mut row,
            &status(Checks::Failing, Mergeable::Unstable),
            &branch,
            &head,
        )
        .await;

        assert_eq!(
            row.auto_fix_run_id, None,
            "a run with no way to push would only spin"
        );
    }
}
