use serde_json::{Value, json};
use sqlx::{Acquire, PgConnection, Postgres};
use uuid::Uuid;

use crate::db::{DbResult, chats, tasks};

/// How a notice about a task run is marked, so the chat can draw it as
/// something an automation wrote rather than a turn somebody typed.
pub const TASK_RUN: &str = "task_run";

/// A message stored in a chat, to publish once whatever stored it commits.
#[must_use]
#[derive(Debug)]
pub struct Notice {
    chat_id: Uuid,
    message: Value,
}

impl Notice {
    pub fn publish(self) {
        crate::db::actions::publish(self.chat_id, self.message);
    }
}

/// Say `content` in `chat_id` in a transaction of its own, or in a savepoint
/// when `connection` is already in one, so a chat that cannot be written to
/// never undoes the caller's own work.
pub async fn post<'c>(
    connection: impl Acquire<'c, Database = Postgres>,
    chat_id: Uuid,
    content: &str,
    metadata: Value,
) -> Option<Notice> {
    match store(connection, chat_id, content, metadata).await {
        Ok(message) => Some(Notice { chat_id, message }),
        Err(error) if gone(&error) => {
            tracing::debug!(%chat_id, "the chat a notice was meant for is gone");
            None
        }
        Err(error) => {
            tracing::warn!(%chat_id, %error, "could not say a notice in its chat");
            None
        }
    }
}

async fn store<'c>(
    connection: impl Acquire<'c, Database = Postgres>,
    chat_id: Uuid,
    content: &str,
    metadata: Value,
) -> DbResult<Value> {
    let mut transaction = connection.begin().await?;
    let message = chats::announce(&mut transaction, chat_id, content, metadata).await?;
    transaction.commit().await?;
    Ok(message)
}

fn gone(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|error| error.is_foreign_key_violation())
}

/// Say in the chat that started a task how one of its runs ended, from inside
/// the transaction that ended it. Every terminal transition matches an active
/// run once, so each run is announced exactly once, and a run auto-fix claimed
/// is left to auto-fix, which says how it ended in its own words.
pub async fn announce_run(
    connection: &mut PgConnection,
    run: Uuid,
    status: &str,
    error: Option<&str>,
) -> DbResult<Option<Notice>> {
    let destination: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT t.id, t.chat_id, t.title FROM task_runs r JOIN tasks t ON t.id = r.task_id \
         WHERE r.id = $1 AND t.chat_id IS NOT NULL \
           AND NOT EXISTS (SELECT 1 FROM chat_pull_requests p WHERE p.auto_fix_run_id = r.id)",
    )
    .bind(run)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((task, chat, title)) = destination else {
        return Ok(None);
    };
    let metadata = json!({
        "source": TASK_RUN,
        "task_id": task,
        "run_id": run,
        "status": status,
    });
    Ok(post(connection, chat, &content(&title, status, error), metadata).await)
}

/// A run that ended any way but completed says so and why, because that is
/// the outcome somebody has to act on.
fn content(title: &str, status: &str, error: Option<&str>) -> String {
    let title = title.trim();
    match status {
        tasks::COMPLETED => format!("The task “{title}” finished."),
        status => {
            let reason = error
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default();
            format!("The task “{title}” ended as {status}{reason}.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chat_pull_requests::{self, Claim, NewPullRequest};
    use crate::db::sources::testing::pool;
    use crate::db::tasks::{Create, Mutation, Origin, TaskRow, TaskRunRow};
    use crate::db::workspace_members::WorkspaceRole;
    use crate::services::task_progress::{self, ProgressMessage};
    use sqlx::PgPool;

    const TITLE: &str = "Review abnegate/zone #276";

    #[test]
    fn a_completed_run_is_announced_by_its_task() {
        assert_eq!(
            content(TITLE, tasks::COMPLETED, None),
            "The task “Review abnegate/zone #276” finished."
        );
    }

    #[test]
    fn a_run_that_ended_any_other_way_says_so_and_why() {
        assert_eq!(
            content(TITLE, tasks::FAILED, Some("the branch was held")),
            "The task “Review abnegate/zone #276” ended as failed: the branch was held."
        );
        assert_eq!(
            content(TITLE, tasks::CANCELLED, None),
            "The task “Review abnegate/zone #276” ended as cancelled."
        );
    }

    #[test]
    fn an_error_of_nothing_but_space_adds_no_empty_reason() {
        assert_eq!(
            content(TITLE, tasks::FAILED, Some("   ")),
            "The task “Review abnegate/zone #276” ended as failed."
        );
    }

    struct Member {
        workspace: Uuid,
        user: Uuid,
        chat: Uuid,
    }

    async fn member(pool: &PgPool) -> Member {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Notices', $1::text)")
            .bind(organization)
            .execute(pool)
            .await
            .expect("an organization");
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Notices', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(pool)
        .await
        .expect("a workspace");
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name) VALUES ($1, $1::text, 'test-only', 'Asker')",
        )
        .bind(user)
        .execute(pool)
        .await
        .expect("a user");
        crate::db::testing::seat(pool, workspace, user, WorkspaceRole::Member).await;
        let chat = chats::create_chat(pool, Some(workspace), "Asking", "qwen3:8b", true, true)
            .await
            .expect("a chat")
            .id;
        Member {
            workspace,
            user,
            chat,
        }
    }

    fn create(member: &Member, title: Option<&'static str>, chat: Option<Uuid>) -> Create<'static> {
        Create {
            workspace_id: member.workspace,
            project_ids: &[],
            title,
            description: "Review the pull request and say what is wrong with it.",
            acceptance_criteria: None,
            priority: None,
            is_agentic: true,
            require_plan_approval: false,
            source_id: None,
            created_by: Some(member.user),
            chat_id: chat,
            origin: Origin::Member,
        }
    }

    async fn task(pool: &PgPool, member: &Member, chat: Option<Uuid>) -> TaskRow {
        match tasks::create_task_authorized(pool, member.user, create(member, Some(TITLE), chat))
            .await
            .expect("the task")
        {
            Mutation::Applied(task) => task,
            Mutation::NotFound => panic!("a member can create a task in their workspace"),
        }
    }

    async fn run(pool: &PgPool, task: &TaskRow) -> TaskRunRow {
        tasks::create_task_run(pool, task.id).await.expect("a run")
    }

    async fn notices(pool: &PgPool, run: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT content FROM messages \
             WHERE metadata->>'source' = $1 AND metadata->>'run_id' = $2::text \
             ORDER BY created_at",
        )
        .bind(TASK_RUN)
        .bind(run)
        .fetch_all(pool)
        .await
        .expect("the notices")
    }

    async fn notices_in(pool: &PgPool, chat: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE chat_id = $1 AND metadata->>'source' = $2",
        )
        .bind(chat)
        .bind(TASK_RUN)
        .fetch_one(pool)
        .await
        .expect("the count")
    }

    #[tokio::test]
    async fn a_chat_started_run_that_completes_says_so_once() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let run = run(&pool, &task).await;

        tasks::complete_task_run(&pool, run.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion")
            .expect("the run was active");
        tasks::complete_task_run(&pool, run.id, tasks::FAILED, Some("again"), None)
            .await
            .expect("a second completion");

        assert_eq!(
            notices(&pool, run.id).await,
            vec![format!("The task “{TITLE}” finished.")]
        );
        assert_eq!(notices_in(&pool, member.chat).await, 1);
    }

    #[tokio::test]
    async fn a_failed_run_says_so_once_with_its_reason() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let run = run(&pool, &task).await;

        tasks::complete_task_run(&pool, run.id, tasks::FAILED, Some("checks fail"), None)
            .await
            .expect("the failure")
            .expect("the run was active");

        assert_eq!(
            notices(&pool, run.id).await,
            vec![format!("The task “{TITLE}” ended as failed: checks fail.")]
        );
    }

    #[tokio::test]
    async fn a_stopped_run_says_so_once() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let run = run(&pool, &task).await;

        for _ in 0..2 {
            tasks::stop_task_run_authorized(&pool, member.user, run.id)
                .await
                .expect("the stop");
        }

        assert_eq!(
            notices(&pool, run.id).await,
            vec![format!(
                "The task “{TITLE}” ended as cancelled: {}.",
                tasks::STOPPED
            )]
        );
    }

    #[tokio::test]
    async fn a_run_swept_after_a_restart_says_so_once() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let run = run(&pool, &task).await;
        sqlx::query(
            "UPDATE task_runs SET heartbeat_at = NOW() - INTERVAL '2 minutes' WHERE id = $1",
        )
        .bind(run.id)
        .execute(&pool)
        .await
        .expect("the lease lapses");

        tasks::sweep_task_runs(&pool).await.expect("the sweep");
        tasks::sweep_task_runs(&pool).await.expect("a second sweep");

        let swept = notices(&pool, run.id).await;
        assert_eq!(swept.len(), 1, "{swept:?}");
        assert!(
            swept[0].starts_with(&format!("The task “{TITLE}” ended as failed")),
            "{swept:?}"
        );
    }

    #[tokio::test]
    async fn a_task_no_chat_started_says_nothing() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, None).await;
        let run = run(&pool, &task).await;

        tasks::complete_task_run(&pool, run.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion")
            .expect("the run was active");

        assert!(notices(&pool, run.id).await.is_empty());
    }

    #[tokio::test]
    async fn every_run_of_a_chat_task_says_so_including_a_rerun() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let first = run(&pool, &task).await;
        tasks::complete_task_run(&pool, first.id, tasks::FAILED, Some("flaky"), None)
            .await
            .expect("the failure");

        let again = run(&pool, &task).await;
        tasks::complete_task_run(&pool, again.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion")
            .expect("the rerun was active");

        assert_eq!(
            notices(&pool, again.id).await,
            vec![format!("The task “{TITLE}” finished.")]
        );
        assert_eq!(notices_in(&pool, member.chat).await, 2);
    }

    #[tokio::test]
    async fn a_notice_names_the_task_by_its_current_title() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = match tasks::create_task_authorized(
            &pool,
            member.user,
            create(&member, None, Some(member.chat)),
        )
        .await
        .expect("the task")
        {
            Mutation::Applied(task) => task,
            Mutation::NotFound => panic!("a member can create a task in their workspace"),
        };
        assert!(task.title_pending, "an untitled task waits for its title");
        let run = run(&pool, &task).await;
        assert!(
            tasks::complete_title(&pool, task.id, Some("Name the review"))
                .await
                .expect("the title")
        );

        tasks::complete_task_run(&pool, run.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion");

        assert_eq!(
            notices(&pool, run.id).await,
            vec!["The task “Name the review” finished.".to_string()]
        );
    }

    #[tokio::test]
    async fn a_run_auto_fix_claimed_is_left_to_auto_fix() {
        let pool = pool().await;
        let member = member(&pool).await;
        let url = "https://github.com/acme/widgets/pull/7";
        let watched = chat_pull_requests::record(
            &pool,
            &NewPullRequest {
                chat_id: member.chat,
                owner: "acme",
                repo: "widgets",
                number: 7,
                url,
                title: None,
                source_id: None,
                opened_by: Some(member.user),
            },
        )
        .await
        .expect("the pull request");
        let claim = Claim {
            id: watched.id,
            previous: None,
            branch: "fix/widgets",
            head: "0123456789abcdef0123456789abcdef01234567",
            url,
        };
        let started = chat_pull_requests::start_auto_fix(
            &pool,
            &claim,
            &Create {
                origin: Origin::Automation,
                ..create(&member, Some("Fix acme/widgets #7"), Some(member.chat))
            },
            "Fixing acme/widgets#7.",
        )
        .await
        .expect("the start")
        .expect("auto-fix is on and nothing is claimed");

        tasks::complete_task_run(&pool, started.run.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion")
            .expect("the run was active");

        assert!(notices(&pool, started.run.id).await.is_empty());
    }

    #[tokio::test]
    async fn a_deleted_chat_is_told_nothing_and_the_run_still_ends() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let run = run(&pool, &task).await;
        assert!(
            chats::delete_chat(&pool, member.chat)
                .await
                .expect("the delete")
        );

        let ended = tasks::complete_task_run(&pool, run.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion");

        assert!(ended.is_some(), "a gone chat never holds a run open");
        assert!(notices(&pool, run.id).await.is_empty());
    }

    #[tokio::test]
    async fn a_chat_gone_mid_transition_is_recognised_and_spares_the_transaction() {
        let pool = pool().await;
        let mut transaction = pool.begin().await.expect("a transaction");

        let refused = store(
            &mut *transaction,
            Uuid::new_v4(),
            "Anyone there?",
            json!({}),
        )
        .await
        .expect_err("a chat that does not exist cannot be written to");
        assert!(gone(&refused), "{refused:?}");
        assert!(
            post(
                &mut *transaction,
                Uuid::new_v4(),
                "Anyone there?",
                json!({})
            )
            .await
            .is_none()
        );

        let alive: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&mut *transaction)
            .await
            .expect("the caller's transaction survives a notice that could not be said");
        assert_eq!(alive, 1);
        transaction.commit().await.expect("the commit");
    }

    #[tokio::test]
    async fn the_notice_is_stored_before_a_waiter_hears_the_run_end() {
        let pool = pool().await;
        let member = member(&pool).await;
        let task = task(&pool, &member, Some(member.chat)).await;
        let run = run(&pool, &task).await;
        let mut events = task_progress::progress().subscribe(run.id);

        tasks::complete_task_run(&pool, run.id, tasks::COMPLETED, None, None)
            .await
            .expect("the completion");

        match events.recv().await {
            Ok(ProgressMessage::Completed { .. }) => {}
            other => panic!("expected a completed frame, got {other:?}"),
        }
        assert_eq!(
            notices(&pool, run.id).await.len(),
            1,
            "an agent woken by the run ending replies after the notice"
        );
    }
}
