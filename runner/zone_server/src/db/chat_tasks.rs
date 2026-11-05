//! The tasks each chat's agent created or started, with the run each one is on.

use chrono::NaiveDateTime;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use super::{DbResult, workspace_members};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ChatTaskRow {
    pub id: Uuid,
    pub title: String,
    pub title_pending: bool,
    pub status: String,
    pub is_agentic: bool,
    pub pr_url: Option<String>,
    pub pr_status: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub run_id: Option<Uuid>,
    pub run_status: Option<String>,
    pub run_phase: Option<String>,
    pub run_progress: Option<i32>,
    pub run_started_at: Option<NaiveDateTime>,
    pub run_completed_at: Option<NaiveDateTime>,
}

/// The chat's tasks in `workspace`, newest first, each with its active run or
/// else the one that started last.
pub async fn list<'connection, E>(
    executor: E,
    workspace: Uuid,
    chat: Uuid,
) -> DbResult<Vec<ChatTaskRow>>
where
    E: Executor<'connection, Database = Postgres>,
{
    sqlx::query_as(
        "SELECT t.id, t.title, t.title_pending, t.status, t.is_agentic, t.pr_url, t.pr_status, \
                t.created_at, t.updated_at, \
                r.id AS run_id, r.status AS run_status, r.current_phase AS run_phase, \
                r.progress_percent AS run_progress, r.started_at AS run_started_at, \
                r.completed_at AS run_completed_at \
         FROM tasks t \
         LEFT JOIN LATERAL ( \
             SELECT id, status, current_phase, progress_percent, started_at, completed_at \
             FROM task_runs \
             WHERE task_id = t.id \
             ORDER BY id = t.active_run_id DESC, started_at DESC NULLS LAST, id DESC \
             LIMIT 1 \
         ) r ON TRUE \
         WHERE t.chat_id = $1 AND t.workspace_id = $2 \
         ORDER BY t.created_at DESC, t.id DESC",
    )
    .bind(chat)
    .bind(workspace)
    .fetch_all(executor)
    .await
}

/// The chat's tasks as `reader` may see them, or `None` when they are not an
/// active member of `workspace`. Their membership stays locked until the read
/// is done, so a revocation cannot commit between the check and the read.
pub async fn read(
    pool: &PgPool,
    workspace: Uuid,
    chat: Uuid,
    reader: Uuid,
) -> DbResult<Option<Vec<ChatTaskRow>>> {
    let mut transaction = pool.begin().await?;
    if workspace_members::lock_role(&mut transaction, workspace, reader)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    let rows = list(&mut *transaction, workspace, chat).await?;
    transaction.commit().await?;
    Ok(Some(rows))
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::db::actions::{self, StartTask, Task};

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    struct Fixture {
        pool: PgPool,
        workspace: Uuid,
        user: Uuid,
        chat: Uuid,
    }

    async fn fixture() -> Fixture {
        let pool = pool().await;
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Chat tasks', $1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .expect("an organization");
        sqlx::query("INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Chat tasks', $1::text)")
            .bind(workspace)
            .bind(organization)
            .execute(&pool)
            .await
            .expect("a workspace");
        sqlx::query("INSERT INTO users (id, email, password_hash, display_name) VALUES ($1, $1::text, 'test-only', 'Ada')")
            .bind(user)
            .execute(&pool)
            .await
            .expect("a user");
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(workspace)
        .bind(user)
        .execute(&pool)
        .await
        .expect("a member");
        let chat = chat_in(&pool, workspace).await;
        Fixture {
            pool,
            workspace,
            user,
            chat,
        }
    }

    async fn chat_in(pool: &PgPool, workspace: Uuid) -> Uuid {
        sqlx::query_scalar("INSERT INTO chats (workspace_id, title, model_name) VALUES ($1, 'Tasks', 'test') RETURNING id")
            .bind(workspace)
            .fetch_one(pool)
            .await
            .expect("a chat")
    }

    fn started(title: &str) -> StartTask {
        serde_json::from_value(serde_json::json!({"title": title, "description": "Do the work"}))
            .expect("a start request")
    }

    fn created(title: &str) -> Task {
        Task {
            title: Some(title.to_string()),
            description: "Note it".to_string(),
            assignee_id: None,
        }
    }

    async fn stored_chat(pool: &PgPool, task: &str) -> Option<Uuid> {
        sqlx::query_scalar("SELECT chat_id FROM tasks WHERE id = $1::uuid")
            .bind(task)
            .fetch_one(pool)
            .await
            .expect("the task")
    }

    #[tokio::test]
    async fn a_chats_tasks_are_listed_newest_first_with_their_latest_run() {
        let fixture = fixture().await;
        let first = actions::create_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(fixture.chat),
            created("Write the notes"),
        )
        .await
        .expect("a created task");
        let second = actions::start_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(fixture.chat),
            started("Fix the build"),
        )
        .await
        .expect("a started task");
        sqlx::query(
            "UPDATE tasks SET created_at = created_at - INTERVAL '1 minute' WHERE id = $1::uuid",
        )
        .bind(first["id"].as_str().expect("an id"))
        .execute(&fixture.pool)
        .await
        .expect("an older task");
        sqlx::query("UPDATE task_runs SET current_phase = 'implement', progress_percent = 40 WHERE id = $1::uuid")
            .bind(second["run_id"].as_str().expect("a run"))
            .execute(&fixture.pool)
            .await
            .expect("progress");

        let listed = list(&fixture.pool, fixture.workspace, fixture.chat)
            .await
            .expect("the chat's tasks");

        let titles: Vec<&str> = listed.iter().map(|task| task.title.as_str()).collect();
        assert_eq!(titles, ["Fix the build", "Write the notes"]);
        let running = &listed[0];
        assert_eq!(
            running.run_id.map(|id| id.to_string()).as_deref(),
            second["run_id"].as_str()
        );
        assert_eq!(running.run_status.as_deref(), Some("running"));
        assert_eq!(running.run_phase.as_deref(), Some("implement"));
        assert_eq!(running.run_progress, Some(40));
        assert!(running.is_agentic);
        assert_eq!(listed[1].run_id, None, "a created task has no run");
    }

    #[tokio::test]
    async fn the_newest_run_stands_for_a_task_that_has_no_active_one() {
        let fixture = fixture().await;
        let started = actions::start_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(fixture.chat),
            started("Retry"),
        )
        .await
        .expect("a started task");
        let task: Uuid = started["task_id"]
            .as_str()
            .expect("a task")
            .parse()
            .expect("a uuid");
        sqlx::query("UPDATE task_runs SET status = 'failed', completed_at = NOW(), started_at = NOW() - INTERVAL '1 hour' WHERE task_id = $1")
            .bind(task)
            .execute(&fixture.pool)
            .await
            .expect("a failed run");
        sqlx::query("UPDATE tasks SET active_run_id = NULL WHERE id = $1")
            .bind(task)
            .execute(&fixture.pool)
            .await
            .expect("no active run");
        let retried: Uuid = sqlx::query_scalar("INSERT INTO task_runs (task_id, status, started_at) VALUES ($1, 'completed', NOW()) RETURNING id")
            .bind(task)
            .fetch_one(&fixture.pool)
            .await
            .expect("a newer run");

        let listed = list(&fixture.pool, fixture.workspace, fixture.chat)
            .await
            .expect("the chat's tasks");

        assert_eq!(listed[0].run_id, Some(retried));
        assert_eq!(listed[0].run_status.as_deref(), Some("completed"));
    }

    #[tokio::test]
    async fn tasks_from_other_chats_or_without_a_chat_are_not_listed() {
        let fixture = fixture().await;
        let other = chat_in(&fixture.pool, fixture.workspace).await;
        actions::create_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(other),
            created("Elsewhere"),
        )
        .await
        .expect("another chat's task");
        actions::create_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            None,
            created("Unattached"),
        )
        .await
        .expect("a task without a chat");

        let listed = list(&fixture.pool, fixture.workspace, fixture.chat)
            .await
            .expect("the chat's tasks");

        assert!(listed.is_empty(), "{listed:?}");
    }

    #[tokio::test]
    async fn a_chat_from_another_workspace_is_never_recorded_on_a_task() {
        let foreign = fixture().await.chat;
        let fixture = fixture().await;

        let created = actions::create_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(foreign),
            created("Kept apart"),
        )
        .await
        .expect("a created task");
        let started = actions::start_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(foreign),
            started("Kept apart"),
        )
        .await
        .expect("a started task");

        assert_eq!(
            stored_chat(&fixture.pool, created["id"].as_str().expect("an id")).await,
            None
        );
        assert_eq!(
            stored_chat(&fixture.pool, started["task_id"].as_str().expect("an id")).await,
            None
        );
    }

    #[tokio::test]
    async fn only_an_active_member_of_the_workspace_reads_its_chats_tasks() {
        let stranger = fixture().await.user;
        let fixture = fixture().await;
        actions::create_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(fixture.chat),
            created("Members only"),
        )
        .await
        .expect("a created task");

        let member = read(&fixture.pool, fixture.workspace, fixture.chat, fixture.user)
            .await
            .expect("a member's read");
        let outsider = read(&fixture.pool, fixture.workspace, fixture.chat, stranger)
            .await
            .expect("a stranger's read");
        sqlx::query("UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2")
            .bind(fixture.workspace)
            .bind(fixture.user)
            .execute(&fixture.pool)
            .await
            .expect("membership revoked");
        let revoked = read(&fixture.pool, fixture.workspace, fixture.chat, fixture.user)
            .await
            .expect("a revoked member's read");

        assert_eq!(member.map(|rows| rows.len()), Some(1));
        assert_eq!(outsider, None);
        assert_eq!(revoked, None);
    }

    #[tokio::test]
    async fn a_task_outlives_its_chat_without_the_link() {
        let fixture = fixture().await;
        let created = actions::create_task(
            &fixture.pool,
            fixture.workspace,
            fixture.user,
            Some(fixture.chat),
            created("Survivor"),
        )
        .await
        .expect("a created task");
        let id = created["id"].as_str().expect("an id");
        assert_eq!(stored_chat(&fixture.pool, id).await, Some(fixture.chat));

        sqlx::query("DELETE FROM chats WHERE id = $1")
            .bind(fixture.chat)
            .execute(&fixture.pool)
            .await
            .expect("the chat deleted");

        assert_eq!(stored_chat(&fixture.pool, id).await, None);
    }
}
