//! The pull requests each chat opened.

use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;
use super::tasks::{self, Create, TaskRow, TaskRunRow};

pub const GITHUB: &str = "github";

macro_rules! columns {
    () => {
        "id, chat_id, provider, owner, repo, number, url, title, source_id, dismissed_at, created_at, auto_fix, auto_fix_run_id, auto_fix_by, auto_fix_head,
         EXISTS (
             SELECT 1 FROM task_runs
             WHERE task_runs.id = chat_pull_requests.auto_fix_run_id AND task_runs.completed_at IS NULL
         ) AS auto_fixing"
    };
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PullRequestRow {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub provider: String,
    pub owner: String,
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: Option<String>,
    pub source_id: Option<Uuid>,
    pub dismissed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Whether a failing or conflicted state should start a run that fixes it.
    pub auto_fix: bool,
    /// The newest run auto-fix started, settled or not.
    pub auto_fix_run_id: Option<Uuid>,
    /// Who turned auto-fix on, and so whom its runs act as.
    pub auto_fix_by: Option<Uuid>,
    /// The head commit the newest run started from.
    pub auto_fix_head: Option<String>,
    /// Whether that run is still working.
    pub auto_fixing: bool,
}

/// What a run claiming a pull request for auto-fix starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim<'a> {
    pub id: Uuid,
    /// The run the reader saw holding the pull request.
    pub previous: Option<Uuid>,
    /// The pull request's head branch, which the run works on.
    pub branch: &'a str,
    /// The commit at its tip.
    pub head: &'a str,
    pub url: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPullRequest<'a> {
    pub chat_id: Uuid,
    pub owner: &'a str,
    pub repo: &'a str,
    pub number: i64,
    pub url: &'a str,
    pub title: Option<&'a str>,
    pub source_id: Option<Uuid>,
}

/// Records that `opened.chat_id` opened this pull request. Recording one the
/// chat already owns keeps the row, filling in what the new sighting knows and
/// the old one did not.
pub async fn record<'e, E>(executor: E, opened: &NewPullRequest<'_>) -> DbResult<PullRequestRow>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "INSERT INTO chat_pull_requests (chat_id, provider, owner, repo, number, url, title, source_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (chat_id, provider, lower(owner), lower(repo), number) DO UPDATE SET
             title = COALESCE(EXCLUDED.title, chat_pull_requests.title),
             source_id = COALESCE(EXCLUDED.source_id, chat_pull_requests.source_id)
         RETURNING ",
        columns!()
    ))
    .bind(opened.chat_id)
    .bind(GITHUB)
    .bind(opened.owner)
    .bind(opened.repo)
    .bind(opened.number)
    .bind(opened.url)
    .bind(opened.title)
    .bind(opened.source_id)
    .fetch_one(executor)
    .await
}

/// The pull requests `chat_id` opened and has not dismissed, oldest first.
pub async fn list<'e, E>(executor: E, chat_id: Uuid) -> DbResult<Vec<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM chat_pull_requests
         WHERE chat_id = $1 AND dismissed_at IS NULL
         ORDER BY created_at, id"
    ))
    .bind(chat_id)
    .fetch_all(executor)
    .await
}

/// One pull request `chat_id` opened, dismissed or not.
pub async fn get<'e, E>(executor: E, chat_id: Uuid, id: Uuid) -> DbResult<Option<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM chat_pull_requests WHERE chat_id = $1 AND id = $2"
    ))
    .bind(chat_id)
    .bind(id)
    .fetch_optional(executor)
    .await
}

/// Hides the pull request from its chat, or with `false` shows it again.
pub async fn dismiss<'e, E>(
    executor: E,
    chat_id: Uuid,
    id: Uuid,
    dismissed: bool,
) -> DbResult<Option<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "UPDATE chat_pull_requests
         SET dismissed_at = CASE WHEN $3 THEN COALESCE(dismissed_at, NOW()) END
         WHERE chat_id = $1 AND id = $2
         RETURNING ",
        columns!()
    ))
    .bind(chat_id)
    .bind(id)
    .bind(dismissed)
    .fetch_optional(executor)
    .await
}

/// Turns the auto-fix preference on for `user`, whose runs then act as them,
/// or off. The newest run stays claimed either way, so turning it back on
/// while that run works cannot start a second one on the same branch.
/// Turning it on forgets the head that run started from, so it is also how
/// to retry a head a settled run could do nothing with.
pub async fn set_auto_fix<'e, E>(
    executor: E,
    chat_id: Uuid,
    id: Uuid,
    auto_fix: bool,
    user: Uuid,
) -> DbResult<Option<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "UPDATE chat_pull_requests
         SET auto_fix = $3,
             auto_fix_by = CASE WHEN $3 THEN $4 END,
             auto_fix_head = CASE WHEN $3 THEN NULL ELSE auto_fix_head END
         WHERE chat_id = $1 AND id = $2
         RETURNING ",
        columns!()
    ))
    .bind(chat_id)
    .bind(id)
    .bind(auto_fix)
    .bind(user)
    .fetch_optional(executor)
    .await
}

/// Creates `task` with a run on the pull request's own branch, and claims
/// the pull request for that run, all or nothing.
///
/// Nothing is created unless auto-fix is still on for the task's creator, the
/// row still holds `claim.previous` and that run has settled, the head has
/// moved since that run started, and the task's source is still an active one
/// of its workspace. So two readers observing the same failure start one run
/// between them rather than one each, a run that changed nothing is not
/// repeated, and the run acts only as whoever turned auto-fix on, with their
/// write access checked as it is admitted.
pub async fn start_auto_fix(
    pool: &PgPool,
    claim: &Claim<'_>,
    task: &Create<'_>,
) -> DbResult<Option<(TaskRow, TaskRunRow)>> {
    let Some(actor) = task.created_by else {
        return Ok(None);
    };
    let mut transaction = pool.begin().await?;
    // Everything the new rows reference is held before the pull request row,
    // the order deleting any of them takes, since each deletion clears the
    // pull request's reference to it.
    if let Some(source_id) = task.source_id {
        let active: Option<Option<bool>> = sqlx::query_scalar(
            "SELECT is_active FROM sources WHERE id = $1 AND workspace_id = $2 FOR SHARE",
        )
        .bind(source_id)
        .bind(task.workspace_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if !matches!(active, Some(None) | Some(Some(true))) {
            return Ok(None);
        }
    }
    if let Some(chat_id) = task.chat_id {
        let chat: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM chats WHERE id = $1 FOR KEY SHARE")
                .bind(chat_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if chat.is_none() {
            return Ok(None);
        }
    }
    let user: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE id = $1 FOR KEY SHARE")
        .bind(actor)
        .fetch_optional(&mut *transaction)
        .await?;
    if user.is_none() {
        return Ok(None);
    }
    let claimable: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM chat_pull_requests
         WHERE id = $1 AND auto_fix AND auto_fix_by = $3
           AND auto_fix_run_id IS NOT DISTINCT FROM $2
           AND auto_fix_head IS DISTINCT FROM $4
           AND NOT EXISTS (
               SELECT 1 FROM task_runs
               WHERE task_runs.id = chat_pull_requests.auto_fix_run_id
                 AND task_runs.completed_at IS NULL
           )
         FOR UPDATE OF chat_pull_requests",
    )
    .bind(claim.id)
    .bind(claim.previous)
    .bind(actor)
    .bind(claim.head)
    .fetch_optional(&mut *transaction)
    .await?;
    if claimable.is_none() {
        return Ok(None);
    }
    let created = tasks::create_in(&mut transaction, task).await?;
    sqlx::query("UPDATE tasks SET branch_name = $2, pr_url = $3 WHERE id = $1")
        .bind(created.id)
        .bind(claim.branch)
        .bind(claim.url)
        .execute(&mut *transaction)
        .await?;
    let run = tasks::create_task_run_in(&mut transaction, created.id, Some(actor)).await?;
    sqlx::query(
        "UPDATE chat_pull_requests SET auto_fix_run_id = $2, auto_fix_head = $3 WHERE id = $1",
    )
    .bind(claim.id)
    .bind(run.id)
    .bind(claim.head)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Some((created, run)))
}

pub async fn retitle<'e, E>(executor: E, id: Uuid, title: &str) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE chat_pull_requests SET title = $2 WHERE id = $1 AND title IS DISTINCT FROM $2",
    )
    .bind(id)
    .bind(title)
    .execute(executor)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::db::chats::create_chat;

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    async fn chat(pool: &PgPool) -> Uuid {
        create_chat(pool, None, "Changes", "qwen3:8b", false, true)
            .await
            .expect("a chat")
            .id
    }

    fn opened(
        chat_id: Uuid,
        owner: &'static str,
        title: Option<&'static str>,
    ) -> NewPullRequest<'static> {
        NewPullRequest {
            chat_id,
            owner,
            repo: "widgets",
            number: 7,
            url: "https://github.com/acme/widgets/pull/7",
            title,
            source_id: None,
        }
    }

    #[tokio::test]
    async fn recording_a_pull_request_twice_keeps_one_row_and_what_either_knew() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;

        let first = record(&pool, &opened(chat_id, "acme", Some("Add widgets")))
            .await
            .expect("the first sighting");
        let second = record(&pool, &opened(chat_id, "ACME", None))
            .await
            .expect("the second sighting");

        assert_eq!(first.id, second.id);
        assert_eq!(second.title.as_deref(), Some("Add widgets"));
        assert_eq!(list(&pool, chat_id).await.expect("the list"), [second]);
    }

    #[tokio::test]
    async fn a_dismissed_pull_request_leaves_the_list_until_it_is_restored() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let row = record(&pool, &opened(chat_id, "acme", None))
            .await
            .expect("a pull request");

        let dismissed = dismiss(&pool, chat_id, row.id, true)
            .await
            .expect("dismissing")
            .expect("the row");
        assert!(dismissed.dismissed_at.is_some());
        assert!(list(&pool, chat_id).await.expect("the list").is_empty());

        let restored = dismiss(&pool, chat_id, row.id, false)
            .await
            .expect("restoring")
            .expect("the row");
        assert_eq!(restored.dismissed_at, None);
        assert_eq!(list(&pool, chat_id).await.expect("the list"), [restored]);
    }

    #[tokio::test]
    async fn another_chats_pull_request_is_out_of_reach() {
        let pool = pool().await;
        let owner = chat(&pool).await;
        let other = chat(&pool).await;
        let row = record(&pool, &opened(owner, "acme", None))
            .await
            .expect("a pull request");

        assert_eq!(get(&pool, other, row.id).await.expect("the lookup"), None);
        assert_eq!(
            dismiss(&pool, other, row.id, true)
                .await
                .expect("the update"),
            None
        );
    }

    const BRANCH: &str = "fix/widgets";
    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
    const NEXT: &str = "fedcba9876543210fedcba9876543210fedcba98";

    struct Watched {
        workspace: Uuid,
        user: Uuid,
        row: PullRequestRow,
    }

    async fn member(pool: &PgPool) -> (Uuid, Uuid) {
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
        (workspace, user)
    }

    fn fix(watched: &Watched) -> Create<'static> {
        Create {
            workspace_id: watched.workspace,
            project_ids: &[],
            title: Some("Fix acme/widgets #7"),
            description: "Make the checks pass.",
            acceptance_criteria: None,
            priority: None,
            is_agentic: true,
            require_plan_approval: false,
            source_id: None,
            created_by: Some(watched.user),
            chat_id: None,
        }
    }

    async fn start_at(
        pool: &PgPool,
        watched: &Watched,
        previous: Option<Uuid>,
        head: &str,
    ) -> Option<TaskRunRow> {
        let claim = Claim {
            id: watched.row.id,
            previous,
            branch: BRANCH,
            head,
            url: &watched.row.url,
        };
        start_auto_fix(pool, &claim, &fix(watched))
            .await
            .expect("the start")
            .map(|(_, run)| run)
    }

    async fn start(pool: &PgPool, watched: &Watched, previous: Option<Uuid>) -> Option<TaskRunRow> {
        start_at(pool, watched, previous, HEAD).await
    }

    async fn tasks_in(pool: &PgPool, workspace_id: Uuid) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_one(pool)
            .await
            .expect("the count")
    }

    async fn watched(pool: &PgPool, auto_fix: bool) -> Watched {
        let (workspace, user) = member(pool).await;
        let chat_id = chat(pool).await;
        let row = record(pool, &opened(chat_id, "acme", None))
            .await
            .expect("the pull request");
        let row = set_auto_fix(pool, chat_id, row.id, auto_fix, user)
            .await
            .expect("the update")
            .expect("the row");
        Watched {
            workspace,
            user,
            row,
        }
    }

    async fn reread(pool: &PgPool, row: &PullRequestRow) -> PullRequestRow {
        get(pool, row.chat_id, row.id)
            .await
            .expect("the read")
            .expect("the row")
    }

    #[tokio::test]
    async fn an_auto_fix_run_acts_on_the_pull_request_as_whoever_turned_it_on() {
        let pool = pool().await;
        let watched = watched(&pool, true).await;
        assert_eq!(watched.row.auto_fix_by, Some(watched.user));

        let run = start(&pool, &watched, None)
            .await
            .expect("auto-fix is on and nothing is claimed");

        let task = tasks::get_task(&pool, run.task_id)
            .await
            .expect("the read")
            .expect("the task");
        assert_eq!(
            task.created_by,
            Some(watched.user),
            "an actorless task can never publish"
        );
        assert_eq!(run.triggered_by, Some(watched.user));
        assert_eq!(
            task.branch_name.as_deref(),
            Some(BRANCH),
            "the fix lands on the pull request's branch"
        );
        assert_eq!(task.pr_url.as_deref(), Some(watched.row.url.as_str()));
    }

    #[tokio::test]
    async fn an_auto_fix_run_is_claimed_and_reads_as_working_until_it_settles() {
        let pool = pool().await;
        let watched = watched(&pool, true).await;

        let run = start(&pool, &watched, None)
            .await
            .expect("auto-fix is on and nothing is claimed");
        let claimed = reread(&pool, &watched.row).await;
        assert_eq!(claimed.auto_fix_run_id, Some(run.id));
        assert!(
            claimed.auto_fixing,
            "a run that has not settled is still working"
        );

        tasks::complete_task_run(&pool, run.id, "failed", Some("checks still fail"), None)
            .await
            .expect("the run settles");
        let settled = reread(&pool, &watched.row).await;
        assert_eq!(
            settled.auto_fix_run_id,
            Some(run.id),
            "the claim outlives its run"
        );
        assert!(
            !settled.auto_fixing,
            "a settled run no longer reads as working"
        );
    }

    #[tokio::test]
    async fn a_settled_run_is_followed_only_once_the_head_moves() {
        let pool = pool().await;
        let watched = watched(&pool, true).await;
        let run = start(&pool, &watched, None).await.expect("the first run");
        tasks::complete_task_run(&pool, run.id, "failed", Some("could not push"), None)
            .await
            .expect("the run settles");

        let unchanged = start(&pool, &watched, Some(run.id)).await;
        assert!(
            unchanged.is_none(),
            "a run that left the head where it was is not repeated"
        );

        let next = start_at(&pool, &watched, Some(run.id), NEXT).await;
        assert!(next.is_some(), "a new head is worth another run");
        assert_eq!(tasks_in(&pool, watched.workspace).await, 2);
    }

    #[tokio::test]
    async fn turning_auto_fix_on_again_retries_the_same_head() {
        let pool = pool().await;
        let watched = watched(&pool, true).await;
        let run = start(&pool, &watched, None).await.expect("the first run");
        tasks::complete_task_run(&pool, run.id, "failed", Some("no project could push"), None)
            .await
            .expect("the run settles");

        let retried = set_auto_fix(
            &pool,
            watched.row.chat_id,
            watched.row.id,
            true,
            watched.user,
        )
        .await
        .expect("the update")
        .expect("the row");
        assert_eq!(retried.auto_fix_head, None);

        let again = start(&pool, &watched, Some(run.id)).await;
        assert!(again.is_some(), "turning it on again is an explicit retry");
    }

    #[tokio::test]
    async fn a_reader_that_lost_the_claim_leaves_no_task_behind() {
        let pool = pool().await;
        let watched = watched(&pool, true).await;

        start(&pool, &watched, None)
            .await
            .expect("the first reader claims it");
        let lost = start(&pool, &watched, None).await;

        assert!(
            lost.is_none(),
            "a reader holding a stale claim starts nothing"
        );
        assert_eq!(tasks_in(&pool, watched.workspace).await, 1);
    }

    #[tokio::test]
    async fn auto_fix_turned_off_starts_nothing() {
        let pool = pool().await;
        let watched = watched(&pool, false).await;
        assert_eq!(watched.row.auto_fix_by, None);

        let started = start(&pool, &watched, None).await;

        assert!(started.is_none());
        assert_eq!(tasks_in(&pool, watched.workspace).await, 0);
    }

    #[tokio::test]
    async fn turning_auto_fix_off_keeps_the_run_it_claimed() {
        let pool = pool().await;
        let watched = watched(&pool, true).await;
        let run = start(&pool, &watched, None).await.expect("the claim");

        let off = set_auto_fix(
            &pool,
            watched.row.chat_id,
            watched.row.id,
            false,
            watched.user,
        )
        .await
        .expect("the update")
        .expect("the row");
        assert_eq!(off.auto_fix_run_id, Some(run.id));
        assert!(off.auto_fixing);

        let on = set_auto_fix(
            &pool,
            watched.row.chat_id,
            watched.row.id,
            true,
            watched.user,
        )
        .await
        .expect("the update")
        .expect("the row");
        let again = start(&pool, &watched, Some(run.id)).await;
        assert!(on.auto_fixing);
        assert!(
            again.is_none(),
            "turning it back on cannot start a second run beside the first"
        );
    }
}
