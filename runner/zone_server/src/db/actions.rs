//! Transactional workspace actions with execution-time membership checks.
use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use tokio::sync::broadcast;
use uuid::Uuid;

/// The last thing a model reads before it decides what to do while a runner
/// runs, which is why the waiting section pins this one too.
pub(crate) const RUNNER_STARTED: &str = "Runner started. Wait for it with wait_for, then read get_task_run and tail_task_log; do not claim the work finished.";

static UPDATES: Lazy<broadcast::Sender<(Uuid, Value)>> = Lazy::new(|| broadcast::channel(256).0);
pub fn subscribe() -> broadcast::Receiver<(Uuid, Value)> {
    UPDATES.subscribe()
}
pub fn publish(chat_id: Uuid, message: Value) {
    let _ = UPDATES.send((chat_id, message));
}

use super::{
    DbResult,
    workspace_members::{self, WorkspaceRole},
};

fn patch<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub assignee_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub task_id: Uuid,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<Status>,
    #[serde(default, deserialize_with = "patch")]
    pub assignee_id: Option<Option<Uuid>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Created,
    InProgress,
    Review,
    Complete,
    Blocked,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::InProgress => "in_progress",
            Self::Review => "review",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }
}

pub fn invalid(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_owned())
}

/// Lock membership so revocation and this transaction have a definite order.
pub async fn authorize(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    user_id: Uuid,
    write: bool,
) -> DbResult<()> {
    let role = workspace_members::lock_role(connection, workspace_id, user_id).await?;
    if !role.is_some_and(|role| {
        role >= WorkspaceRole::Member || (!write && role == WorkspaceRole::Viewer)
    }) {
        return Err(invalid("Workspace access denied"));
    }
    Ok(())
}

pub async fn chat(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    chat_id: Uuid,
) -> DbResult<()> {
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM chats WHERE id = $1 AND workspace_id = $2 FOR UPDATE")
            .bind(chat_id)
            .bind(workspace_id)
            .fetch_optional(connection)
            .await?;
    exists.ok_or_else(|| invalid("Chat not found in this workspace"))?;
    Ok(())
}

pub async fn create_task(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    input: Task,
) -> DbResult<Value> {
    if input.title.trim().is_empty() {
        return Err(invalid("Title must not be blank"));
    }
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    if let Some(assignee) = input.assignee_id {
        authorize(&mut transaction, workspace_id, assignee, false).await?;
    }
    let result = sqlx::query_scalar("INSERT INTO tasks (workspace_id, title, description, assignee_id, created_by) VALUES ($1, $2, $3, $4, $5) RETURNING to_jsonb(tasks.*)")
        .bind(workspace_id).bind(input.title).bind(input.description).bind(input.assignee_id).bind(user_id).fetch_one(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(result)
}

pub async fn update_task(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    input: Update,
) -> DbResult<Value> {
    if input
        .title
        .as_ref()
        .is_some_and(|title| title.trim().is_empty())
    {
        return Err(invalid("Title must not be blank"));
    }
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    if let Some(Some(assignee)) = input.assignee_id {
        authorize(&mut transaction, workspace_id, assignee, false).await?;
    }
    let status = input.status.as_ref().map(Status::as_str);
    let result = sqlx::query_scalar("UPDATE tasks SET title = COALESCE($3, title), description = COALESCE($4, description), status = COALESCE($5, status), assignee_id = CASE WHEN $6 THEN $7 ELSE assignee_id END, started_at = CASE WHEN $5 = 'in_progress' THEN COALESCE(started_at, NOW()) ELSE started_at END, completed_at = CASE WHEN $5 = 'complete' THEN COALESCE(completed_at, NOW()) WHEN $5 IS NOT NULL THEN NULL ELSE completed_at END, updated_at = NOW() WHERE id = $1 AND workspace_id = $2 AND NOT is_agentic RETURNING to_jsonb(tasks.*)")
        .bind(input.task_id).bind(workspace_id).bind(input.title).bind(input.description).bind(status).bind(input.assignee_id.is_some()).bind(input.assignee_id.flatten()).fetch_optional(&mut *transaction).await?;
    transaction.commit().await?;
    result.ok_or_else(|| invalid("Task not found or is managed by the task runner"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub chat_id: Uuid,
    pub content: String,
    #[serde(default)]
    pub mentions: Vec<Uuid>,
    /// Why the model sent this. Declared so `deny_unknown_fields` accepts the
    /// property the tool schema advertises; the console reads it back off the
    /// raw call arguments, and its absence never fails the send.
    pub reason: Option<String>,
}

pub async fn send_message(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    origin: Uuid,
    input: Message,
) -> DbResult<Value> {
    if input.content.trim().is_empty() {
        return Err(invalid("Message must not be blank"));
    }
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    chat(&mut transaction, workspace_id, input.chat_id).await?;
    for member in &input.mentions {
        authorize(&mut transaction, workspace_id, *member, false).await?;
    }
    let content = if input.mentions.is_empty() {
        input.content
    } else {
        let mut labels = Vec::new();
        for member in &input.mentions {
            let name: Option<String> =
                sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
                    .bind(member)
                    .fetch_one(&mut *transaction)
                    .await?;
            labels.push(format!(
                "@{} ({member})",
                name.unwrap_or_else(|| member.to_string())
            ));
        }
        format!("{}\n\n{}", input.content, labels.join(" "))
    };
    let result: Value = sqlx::query_scalar("INSERT INTO messages (chat_id, role, content, metadata) VALUES ($1, 'assistant', $2, $3) RETURNING to_jsonb(messages.*)")
        .bind(input.chat_id).bind(content).bind(json!({"actor_id": user_id, "origin_chat_id": origin, "source": "workspace_tool", "mentions": input.mentions})).fetch_one(&mut *transaction).await?;
    sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
        .bind(input.chat_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    publish(input.chat_id, result.clone());
    Ok(result)
}

pub async fn list_members(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<Value> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, false).await?;
    let result = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('user_id', u.id, 'name', u.display_name, 'role', m.role) ORDER BY u.display_name, u.id), '[]'::jsonb) FROM workspace_members m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 AND m.is_active")
        .bind(workspace_id).fetch_one(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(result)
}

pub async fn list_chats(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<Value> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, false).await?;
    let result = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id, 'title', title, 'archived', archived) ORDER BY updated_at DESC, id), '[]'::jsonb) FROM chats WHERE workspace_id = $1")
        .bind(workspace_id).fetch_one(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(result)
}

/// Assignment-aware inventory uses the same scoped permissions as mutations.
pub async fn list_tasks(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    status: Option<&str>,
) -> DbResult<Value> {
    if status.is_some_and(|status| {
        !matches!(
            status,
            "created" | "queued" | "in_progress" | "review" | "complete" | "blocked"
        )
    }) {
        return Err(invalid("Unknown task status"));
    }
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, false).await?;
    let result = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id, 'title', title, 'description', description, 'status', status, 'assignee_id', assignee_id, 'is_agentic', is_agentic, 'started_at', started_at, 'completed_at', completed_at, 'updated_at', updated_at, 'pr_url', pr_url) ORDER BY created_at DESC, id), '[]'::jsonb) FROM tasks WHERE workspace_id = $1 AND ($2::text IS NULL OR status = $2)")
        .bind(workspace_id).bind(status).fetch_one(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartTask {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub acceptance_criteria: Option<String>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
    pub source_id: Option<Uuid>,
    pub priority: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRunLookup {
    pub run_id: Uuid,
    pub after_log_id: Option<Uuid>,
    pub limit: Option<u32>,
}

fn stamp(value: Option<chrono::NaiveDateTime>) -> Option<String> {
    value.map(|ts| ts.and_utc().to_rfc3339())
}

/// Create an agentic task and a running `task_runs` row. The caller starts the worker.
pub async fn start_task(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    input: StartTask,
) -> DbResult<Value> {
    if input.title.trim().is_empty() {
        return Err(invalid("Title must not be blank"));
    }
    if input.description.trim().is_empty() {
        return Err(invalid("Description must not be blank"));
    }
    if input
        .priority
        .is_some_and(|priority| !(1..=5).contains(&priority))
    {
        return Err(invalid("priority must be between 1 and 5"));
    }
    let (task, run) = match super::tasks::start_task_authorized(
        pool,
        user_id,
        super::tasks::Create {
            workspace_id,
            project_ids: &input.project_ids,
            title: input.title.trim(),
            description: input.description.trim(),
            acceptance_criteria: input.acceptance_criteria.as_deref(),
            priority: input.priority,
            is_agentic: true,
            require_plan_approval: false,
            source_id: input.source_id,
            created_by: Some(user_id),
        },
    )
    .await
    {
        Ok(super::tasks::Mutation::Applied(result)) => result,
        Ok(super::tasks::Mutation::NotFound) => return Err(invalid("Workspace access denied")),
        Err(super::tasks::MutationError::Project) => {
            return Err(invalid("Project is not available in this workspace"));
        }
        Err(super::tasks::MutationError::Source) => {
            return Err(invalid("Source not found in this workspace or inactive"));
        }
        // A task created here has no prior run to conflict with.
        Err(super::tasks::MutationError::ActiveRun) => {
            return Err(invalid("Task has an active run"));
        }
        Err(super::tasks::MutationError::Database(error)) => return Err(error),
    };
    Ok(json!({
        "task_id": task.id,
        "run_id": run.id,
        "title": task.title,
        "is_agentic": true,
        "status": run.status,
        "message": RUNNER_STARTED
    }))
}

pub async fn get_task_run(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    run_id: Uuid,
) -> DbResult<Value> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, false).await?;
    let (run, title): (super::tasks::TaskRunRow, String) = {
        let task: (Uuid, String) = sqlx::query_as("SELECT t.id,t.title FROM tasks t JOIN task_runs r ON r.task_id=t.id WHERE r.id=$1 AND t.workspace_id=$2 FOR SHARE OF t")
            .bind(run_id).bind(workspace_id).fetch_one(&mut *transaction).await?;
        let run = super::tasks::get_task_run(&mut *transaction, run_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        (run, task.1)
    };
    transaction.commit().await?;
    Ok(json!({
        "id": run.id,
        "task_id": run.task_id,
        "title": title,
        "status": run.status,
        "current_phase": run.current_phase,
        "progress_percent": run.progress_percent,
        "started_at": stamp(run.started_at),
        "completed_at": stamp(run.completed_at),
        "error_message": run.error_message,
        "artifacts": run.artifacts,
    }))
}

pub async fn tail_task_log(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    input: TaskRunLookup,
) -> DbResult<Value> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, false).await?;
    let (run, _title): (super::tasks::TaskRunRow, String) = {
        let task: (Uuid, String) = sqlx::query_as("SELECT t.id,t.title FROM tasks t JOIN task_runs r ON r.task_id=t.id WHERE r.id=$1 AND t.workspace_id=$2 FOR SHARE OF t")
            .bind(input.run_id).bind(workspace_id).fetch_one(&mut *transaction).await?;
        let run = super::tasks::get_task_run(&mut *transaction, input.run_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        (run, task.1)
    };
    let logs = super::tasks::get_task_run_logs(&mut *transaction, run.id).await?;
    transaction.commit().await?;
    let after = input.after_log_id;
    let limit = input.limit.unwrap_or(50).clamp(1, 200) as usize;
    let offset = match after {
        Some(id) => {
            logs.iter()
                .position(|log| log.id == id)
                .ok_or_else(|| invalid("Log cursor not found in this run"))?
                + 1
        }
        None => 0,
    };
    let selected: Vec<_> = logs.into_iter().skip(offset).take(limit + 1).collect();
    let has_more = selected.len() > limit;
    let lines: Vec<Value> = selected
        .into_iter()
        .take(limit)
        .map(|log| {
            json!({
                "id": log.id,
                "phase": log.phase,
                "agent_type": log.agent_type,
                "log_level": log.log_level,
                "message": log.message,
                "metadata": log.metadata,
                "created_at": stamp(log.created_at),
            })
        })
        .collect();
    Ok(json!({
        "run_id": run.id,
        "status": run.status,
        "logs": lines,
        "has_more": has_more,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::reminders;
    use crate::db::reminders::Delivered;
    use chrono::{Duration, Utc};

    /// The key every test that dispatches a reminder takes first.
    ///
    /// `deliver_next` claims the oldest due reminder in the whole database
    /// rather than the oldest in one workspace — it is a dispatcher, and every
    /// server instance shares it — and `claim_turn` takes the oldest firing
    /// that still owes a turn the same way. Two tests that both have one
    /// therefore steal each other's.
    ///
    /// In the database rather than in this process, because CI runs these
    /// under `cargo nextest`, which gives every test a process of its own: a
    /// `Mutex` here would serialise nothing there, and the tests would pass
    /// locally and race in CI. Postgres releases a session lock when the
    /// connection holding it goes, which covers a test that panics as well as
    /// one that returns.
    ///
    /// Not a fixture concern: the rows are already isolated by workspace. It is
    /// the claim that is global, which is the behaviour under test.
    const DISPATCH: i64 = 0x7A6F_6E65_5245_4D44;

    /// Held for the length of a dispatching test. Its connection is its own
    /// rather than the pool's, because a pooled connection is returned to the
    /// pool still holding the lock.
    async fn dispatching() -> sqlx::PgConnection {
        use sqlx::Connection;
        let mut connection = sqlx::PgConnection::connect(
            &std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
        )
        .await
        .expect("a connection of its own to hold the dispatch lock");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(DISPATCH)
            .execute(&mut connection)
            .await
            .expect("the dispatch lock is takeable");
        connection
    }

    /// Long enough that a claimed turn stays claimed for the length of a test,
    /// which is what a real lease is for.
    const LEASE: std::time::Duration = std::time::Duration::from_secs(3600);

    /// And a lease nothing can be inside, which is a worker that claimed a turn
    /// and never came back — without a test that has to wait out a real one.
    const ABANDONED: std::time::Duration = std::time::Duration::ZERO;

    async fn fixture() -> (PgPool, Uuid, Uuid, Uuid, Uuid) {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL required"))
            .await
            .unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let user = Uuid::new_v4();
        let chat = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Actions test', $1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Actions test', $1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO users (id, email, password_hash, display_name) VALUES ($1, $1::text, 'test-only', 'Alice')").bind(user).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(workspace)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO chats (id, workspace_id, title, model_name) VALUES ($1, $2, 'Test', 'test')").bind(chat).bind(workspace).execute(&pool).await.unwrap();
        (pool, organization, workspace, user, chat)
    }

    /// A single connection announcing itself to `pg_stat_activity` by name.
    async fn named_pool(application: &str) -> PgPool {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL required"))
            .await
            .unwrap();
        sqlx::query_scalar::<_, String>("SELECT set_config('application_name', $1, false)")
            .bind(application)
            .fetch_one(&pool)
            .await
            .unwrap();
        pool
    }

    /// Return once the named connection is waiting on a row lock.
    async fn wait_until_blocked(pool: &PgPool, application: &str) {
        for _ in 0..1000 {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name = $1 AND wait_event_type = 'Lock')",
            )
            .bind(application)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("{application} never waited on the row it was meant to block on");
    }

    async fn cleanup(pool: &PgPool, organization: Uuid, user: Uuid) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user)
            .execute(pool)
            .await
            .unwrap();
    }

    #[test]
    fn sparse_assignment_and_timezone_arguments() {
        let id = Uuid::new_v4();
        let omitted: Update = serde_json::from_value(json!({"task_id":id})).unwrap();
        let clear: Update =
            serde_json::from_value(json!({"task_id":id,"assignee_id":null})).unwrap();
        assert_eq!(omitted.assignee_id, None);
        assert_eq!(clear.assignee_id, Some(None));
        assert!(serde_json::from_value::<Update>(json!({"task_id":id,"status":"queued"})).is_err());
        assert!(
            serde_json::from_value::<reminders::Reminder>(
                json!({"content":"check","due_at":"2026-09-11T09:00:00"})
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn actions_enforce_workspace_roles_sparse_updates_and_mentions() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let created = create_task(
            &pool,
            workspace,
            user,
            Task {
                title: "Ship".into(),
                description: "Keep".into(),
                assignee_id: Some(user),
            },
        )
        .await
        .unwrap();
        let task_id = serde_json::from_value::<Uuid>(created["id"].clone()).unwrap();
        let update: Update =
            serde_json::from_value(json!({"task_id":task_id,"status":"complete"})).unwrap();
        let complete = update_task(&pool, workspace, user, update).await.unwrap();
        assert_eq!(complete["description"], "Keep");
        assert_eq!(complete["assignee_id"], json!(user));
        assert_eq!(
            list_tasks(&pool, workspace, user, Some("complete"))
                .await
                .unwrap()[0]["assignee_id"],
            json!(user)
        );
        assert!(!complete["completed_at"].is_null());
        let update: Update = serde_json::from_value(
            json!({"task_id":task_id,"status":"created","assignee_id":null}),
        )
        .unwrap();
        let reopened = update_task(&pool, workspace, user, update).await.unwrap();
        assert!(reopened["assignee_id"].is_null());
        assert!(reopened["completed_at"].is_null());
        let (_, other_organization, other, other_user, other_chat) = fixture().await;
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(other)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        let update: Update =
            serde_json::from_value(json!({"task_id":task_id,"title":"stolen"})).unwrap();
        assert!(update_task(&pool, other, user, update).await.is_err());
        let assignment: Update =
            serde_json::from_value(json!({"task_id":task_id,"assignee_id":other_user})).unwrap();
        assert!(
            update_task(&pool, workspace, user, assignment)
                .await
                .is_err()
        );
        assert!(
            send_message(
                &pool,
                workspace,
                user,
                chat_id,
                Message {
                    chat_id: other_chat,
                    content: "fail".into(),
                    mentions: vec![],
                    reason: None,
                }
            )
            .await
            .is_err()
        );
        assert!(
            send_message(
                &pool,
                workspace,
                user,
                chat_id,
                Message {
                    chat_id,
                    content: "fail".into(),
                    mentions: vec![other_user],
                    reason: None,
                }
            )
            .await
            .is_err()
        );
        let message = send_message(
            &pool,
            workspace,
            user,
            chat_id,
            Message {
                chat_id,
                content: "Hello".into(),
                mentions: vec![user],
                reason: None,
            },
        )
        .await
        .unwrap();
        assert!(message["content"].as_str().unwrap().contains("@Alice"));
        assert_eq!(message["metadata"]["actor_id"], json!(user));
        assert_eq!(
            list_members(&pool, workspace, user).await.unwrap()[0]["name"],
            "Alice"
        );
        assert_eq!(
            list_chats(&pool, workspace, user).await.unwrap()[0]["id"],
            json!(chat_id)
        );
        sqlx::query("UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1")
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        assert!(list_members(&pool, workspace, user).await.is_ok());
        assert!(
            create_task(
                &pool,
                workspace,
                user,
                Task {
                    title: "No".into(),
                    description: String::new(),
                    assignee_id: None
                }
            )
            .await
            .is_err()
        );
        sqlx::query("UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1")
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        assert!(list_chats(&pool, workspace, user).await.is_err());
        cleanup(&pool, other_organization, other_user).await;
        cleanup(&pool, organization, user).await;
    }

    #[tokio::test]
    async fn a_message_sends_with_or_without_a_reason() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        for reason in [
            Some("The user asked for the team to be told.".to_string()),
            None,
        ] {
            let sent = send_message(
                &pool,
                workspace,
                user,
                chat_id,
                Message {
                    chat_id,
                    content: "Shipped".into(),
                    mentions: vec![],
                    reason,
                },
            )
            .await
            .unwrap();
            assert_eq!(
                sent["content"], "Shipped",
                "a reason never edits the message"
            );
        }
        cleanup(&pool, organization, user).await;
    }

    /// A rule the schedule module refuses never reaches a row, and each
    /// refusal names what was wrong: the caller is a model turning a person's
    /// sentence into a schedule, and a refusal it cannot act on costs the
    /// person another turn.
    #[tokio::test]
    async fn a_schedule_this_build_will_not_keep_is_refused_at_the_create() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let automation =
            |rrule: Option<&str>, prompt: Option<&str>, mode: Option<&str>| reminders::Reminder {
                content: "Check the build".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: rrule.map(str::to_string),
                prompt: prompt.map(str::to_string),
                timing_mode: mode.map(str::to_string),
            };

        for (rrule, prompt, mode, expected) in [
            // Faster than the ceiling, counted after the BY clauses split it.
            (
                Some(
                    "FREQ=DAILY;BYHOUR=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;\
                     BYMINUTE=0,30",
                ),
                None,
                None,
                "once an hour",
            ),
            // A clause this build does not implement, refused and not dropped.
            (
                Some("FREQ=MONTHLY;BYSETPOS=-1;BYDAY=FR"),
                None,
                None,
                "BYSETPOS",
            ),
            // A clause the frequency's arithmetic never reads, which would
            // otherwise fire on days nobody chose.
            (Some("FREQ=DAILY;BYDAY=MO,FR"), None, None, "does not use"),
            // A mode that is not a mode at all.
            (None, None, Some("whenever"), "not a timing mode"),
            // The mode the worker still cannot dispatch: accepting it would run
            // it under exact_schedule's contract, which is not what it asked
            // for.
            (None, None, Some("flexible_schedule"), "window"),
            // And a watch missing either half of its comparison. One firing has
            // nothing to compare against, and fixed content has nothing to
            // compare, so neither is stored and quietly downgraded.
            (
                None,
                Some("Check it"),
                Some("condition_watch"),
                "both halves",
            ),
            (
                Some("FREQ=DAILY"),
                None,
                Some("condition_watch"),
                "both halves",
            ),
            (None, None, Some("condition_watch"), "both halves"),
        ] {
            let refused = reminders::create(
                &pool,
                workspace,
                user,
                chat_id,
                automation(rrule, prompt, mode),
            )
            .await
            .expect_err(&format!("{rrule:?}/{prompt:?}/{mode:?} must be refused"));
            let sqlx::Error::Protocol(why) = &refused else {
                panic!("{rrule:?}/{mode:?} was refused by the wrong error: {refused:?}");
            };
            assert!(why.contains(expected), "{rrule:?}/{mode:?}: {why}");
        }

        let stored: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM reminders WHERE workspace_id = $1")
                .bind(workspace)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, 0, "a refused create must leave no row behind");
        cleanup(&pool, organization, user).await;
    }

    /// `create` trims a prompt and stores NULL for an empty one, so the
    /// constraint has to mean the same thing by "carries a prompt" — it is the
    /// boundary a write that never went through `create` still has to cross.
    /// A watch holding a prompt of spaces would fire for ever with nothing to
    /// ask, which is the mode doing less than its name, quietly.
    #[tokio::test]
    async fn a_watch_missing_either_half_is_refused_by_the_database_too() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        for (rrule, prompt) in [
            (Some("FREQ=DAILY"), Some("   ")),
            (Some("  \t "), Some("Check the release branch")),
            (Some("FREQ=DAILY"), None),
            (None, Some("Check the release branch")),
        ] {
            let refused = sqlx::query(
                "INSERT INTO reminders (workspace_id, created_by, chat_id, content, due_at, \
                 rrule, prompt, timing_mode) VALUES ($1, $2, $3, 'Release branch', \
                 NOW() + INTERVAL '1 hour', $4, $5, 'condition_watch')",
            )
            .bind(workspace)
            .bind(user)
            .bind(chat_id)
            .bind(rrule)
            .bind(prompt)
            .execute(&pool)
            .await;
            let Err(sqlx::Error::Database(why)) = refused else {
                panic!("{rrule:?}/{prompt:?} must be refused by the database");
            };
            assert_eq!(
                why.constraint(),
                Some("reminders_watch_compares_check"),
                "{rrule:?}/{prompt:?} was refused by the wrong constraint: {why}"
            );
        }
        cleanup(&pool, organization, user).await;
    }

    /// Two firings of one schedule can be owed at once — the server was down,
    /// and the schedule moved on twice. Handing both out together would read
    /// one baseline into both and write the two answers back in whatever order
    /// they finished, which is exactly the comparison a watch promises not to
    /// get wrong. They go out one at a time, and schedules do not block each
    /// other.
    #[tokio::test]
    async fn a_schedule_runs_one_firing_at_a_time_and_does_not_hold_up_another() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let watch = |name: &str| reminders::Reminder {
            content: name.into(),
            due_at: Utc::now() + Duration::hours(1),
            rrule: Some("FREQ=DAILY".into()),
            prompt: Some("Check whether the release branch is green".into()),
            timing_mode: Some("condition_watch".into()),
        };
        let mut owed = Vec::new();
        for name in ["Release branch", "Dependency PRs"] {
            let created = reminders::create(&pool, workspace, user, chat_id, watch(name))
                .await
                .unwrap();
            let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
            // Two firings of this one, written down the way a claim writes them.
            for _ in 0..2 {
                sqlx::query(
                    "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, \
                     created_by, prompt) VALUES ($1, $2, $3, $4, 'Check it')",
                )
                .bind(id)
                .bind(workspace)
                .bind(chat_id)
                .bind(user)
                .execute(&pool)
                .await
                .unwrap();
            }
            owed.push(id);
        }

        // One from each schedule, and then nothing: the second firing of each
        // is waiting on the first, not on the other schedule.
        let first = reminders::claim_turn(&pool, LEASE)
            .await
            .unwrap()
            .expect("the oldest firing is claimable");
        let second = reminders::claim_turn(&pool, LEASE)
            .await
            .unwrap()
            .expect("the other schedule is not held up by the first");
        assert_ne!(
            first.reminder_id, second.reminder_id,
            "a schedule with a firing in flight must not be handed its next one"
        );
        assert!(
            reminders::claim_turn(&pool, LEASE).await.unwrap().is_none(),
            "both schedules now have a firing in flight, so neither offers its second"
        );

        // And once a firing is done, the one behind it is offered — to the same
        // schedule, so the next comparison is against what the first recorded.
        reminders::finish_turn(&pool, first.id).await.unwrap();
        let next = reminders::claim_turn(&pool, LEASE)
            .await
            .unwrap()
            .expect("the firing behind a finished one is offered");
        assert_eq!(
            next.reminder_id, first.reminder_id,
            "the freed schedule is the one that gets its next firing"
        );

        for id in owed {
            sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }
        cleanup(&pool, organization, user).await;
    }

    /// The last attempt a firing gets is still a firing in flight. `attempts`
    /// reaches the bound on the third claim, and a schedule whose turn is out
    /// for the third time is no less busy than one out for the first — so the
    /// question "is another one already running" is answered by the claim
    /// alone, never by how many times it has been tried.
    #[tokio::test]
    async fn a_firing_on_its_last_attempt_still_holds_the_one_behind_it() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Release branch".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some("Check whether the release branch is green".into()),
                timing_mode: Some("condition_watch".into()),
            },
        )
        .await
        .unwrap();
        let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
        for _ in 0..2 {
            sqlx::query(
                "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, \
                 prompt) VALUES ($1, $2, $3, $4, 'Check it')",
            )
            .bind(reminder)
            .bind(workspace)
            .bind(chat_id)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        }

        // The older firing has been taken twice and its claim has gone stale,
        // so the next one it gets is its third and last.
        let older: Uuid = sqlx::query_scalar(
            "SELECT id FROM reminder_turns WHERE reminder_id = $1 ORDER BY created_at, id LIMIT 1",
        )
        .bind(reminder)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE reminder_turns SET attempts = 2, claimed_at = NOW() - INTERVAL '1 day' \
             WHERE id = $1",
        )
        .bind(older)
        .execute(&pool)
        .await
        .unwrap();

        let third = reminders::claim_turn(&pool, LEASE)
            .await
            .unwrap()
            .expect("a firing whose claim went stale is offered again");
        assert_eq!(third.id, older, "the stale claim is the one taken up again");
        let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM reminder_turns WHERE id = $1")
            .bind(older)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(attempts, 3, "the third claim reaches the attempt bound");
        assert!(
            reminders::claim_turn(&pool, LEASE).await.unwrap().is_none(),
            "a firing out for the last time still holds the one behind it; otherwise the two run \
             together and the watch compares both against the same reading"
        );

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    /// The baseline is a column and an explicit write, because the cheap
    /// version — asking the model to compare against the answer already sitting
    /// in the chat — is destroyed by compaction and says nothing when it is.
    /// This pins the column: only a watch is given one, a firing's own answer
    /// is what fills it, and a firing that answered nothing leaves it alone.
    #[tokio::test]
    async fn a_watch_keeps_its_own_reading_and_a_plain_reminder_is_given_none() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let watch = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Release branch".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some("Check whether the release branch is green".into()),
                timing_mode: Some("condition_watch".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(watch["timing_mode"], "condition_watch");
        assert!(
            watch["last_observation"].is_null(),
            "a watch starts with nothing to compare against: {watch}"
        );
        let watch_id: Uuid = serde_json::from_value(watch["id"].clone()).unwrap();

        let plain = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Standup".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some("Say what is on today".into()),
                timing_mode: None,
            },
        )
        .await
        .unwrap();
        let plain_id: Uuid = serde_json::from_value(plain["id"].clone()).unwrap();

        // The outer option is what tells the worker to compose a comparison at
        // all, and the inner one is what it has to compare against.
        assert_eq!(
            reminders::watch_baseline(&pool, watch_id).await.unwrap(),
            Some(None),
            "a watch's first firing has a baseline to fill and nothing yet in it"
        );
        assert_eq!(
            reminders::watch_baseline(&pool, plain_id).await.unwrap(),
            None,
            "a reminder that is not a watch is never handed a comparison"
        );

        // A firing that stored no answer leaves the baseline alone, so the next
        // one compares against the last reading that worked. Overwriting it
        // with nothing would report the world changed when all that happened is
        // that this firing did not run.
        reminders::record_observation(&pool, watch_id, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(
            reminders::watch_baseline(&pool, watch_id).await.unwrap(),
            Some(None),
            "a firing with no answer must not overwrite the baseline"
        );

        // One that did answer keeps it, capped. The reading is read back into
        // the next firing's prompt, where an unbounded one would crowd out the
        // context it is meant to be compared in.
        let long = "green. ".repeat(2000);
        let answered = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content) VALUES ($1, $2, 'assistant', $3)",
        )
        .bind(answered)
        .bind(chat_id)
        .bind(&long)
        .execute(&pool)
        .await
        .unwrap();
        reminders::record_observation(&pool, watch_id, answered)
            .await
            .unwrap();
        let kept = reminders::watch_baseline(&pool, watch_id)
            .await
            .unwrap()
            .flatten()
            .expect("a firing that answered fills the baseline");
        assert_eq!(
            kept.chars().count(),
            reminders::OBSERVATION_CAP,
            "a reading longer than the cap is kept up to it"
        );
        assert!(
            long.starts_with(&kept),
            "the cap keeps the leading characters, so two readings stay comparable"
        );

        cleanup(&pool, organization, user).await;
    }

    /// The difference this whole change exists for: a one-shot is finished when
    /// it fires, and an automation moves to its next occurrence and stays
    /// pending. Both deliver exactly one message per firing.
    #[tokio::test]
    async fn an_automation_moves_to_its_next_firing_where_a_one_shot_is_finished() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let one_shot = reminders::Reminder {
            content: "Once".into(),
            due_at: Utc::now() + Duration::hours(1),
            rrule: None,
            prompt: None,
            timing_mode: None,
        };
        let daily = reminders::Reminder {
            content: "Every morning".into(),
            due_at: Utc::now() + Duration::hours(1),
            rrule: Some("FREQ=DAILY".into()),
            // No prompt: this test is about a schedule moving on rather than
            // ending, and a prompt would replace the message it counts with a
            // turn. The prompt path has its own test.
            prompt: None,
            timing_mode: Some("exact_schedule".into()),
        };

        let one_shot = reminders::create(&pool, workspace, user, chat_id, one_shot)
            .await
            .unwrap();
        let daily = reminders::create(&pool, workspace, user, chat_id, daily)
            .await
            .unwrap();
        let one_shot_id: Uuid = serde_json::from_value(one_shot["id"].clone()).unwrap();
        let daily_id: Uuid = serde_json::from_value(daily["id"].clone()).unwrap();
        assert_eq!(daily["timing_mode"], "exact_schedule");
        assert!(
            !daily["anchor_at"].is_null(),
            "a recurring reminder keeps the first firing to measure from: {daily}"
        );
        assert!(
            !daily["expires_at"].is_null(),
            "a recurring reminder carries a lifetime: {daily}"
        );
        assert!(
            one_shot["anchor_at"].is_null() && one_shot["expires_at"].is_null(),
            "a one-shot gets neither: {one_shot}"
        );

        sqlx::query(
            // The anchor moves with the due date. A schedule that has reached its
            // first firing has that firing behind it; leaving the anchor in the
            // future would leave today's occurrence still ahead, which is right for
            // a schedule that has not fired yet and is not the case under test.
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second', anchor_at = \
             CASE WHEN anchor_at IS NULL THEN NULL ELSE NOW() - INTERVAL '1 second' END \
             WHERE workspace_id = $1",
        )
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
        assert_ne!(
            reminders::deliver_next(&pool).await.unwrap(),
            Delivered::Nothing
        );
        assert_ne!(
            reminders::deliver_next(&pool).await.unwrap(),
            Delivered::Nothing
        );
        assert!(
            reminders::deliver_next(&pool).await.unwrap() == Delivered::Nothing,
            "the rescheduled automation is not due again immediately"
        );

        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(messages, 2, "one message per firing, and no more");

        let (status, fired, due): (String, i32, chrono::DateTime<Utc>) =
            sqlx::query_as("SELECT status, fired_count, due_at FROM reminders WHERE id = $1")
                .bind(daily_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "pending", "an automation is not finished by firing");
        assert_eq!(fired, 1);
        assert!(
            due > Utc::now() + Duration::hours(20),
            "a daily rule moves about a day on, not to the next tick: {due}"
        );

        let (status, fired): (String, i32) =
            sqlx::query_as("SELECT status, fired_count FROM reminders WHERE id = $1")
                .bind(one_shot_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "delivered", "a one-shot is finished when it fires");
        assert_eq!(fired, 1);
        cleanup(&pool, organization, user).await;
    }

    /// `COUNT=1` means one firing. The row still carries the count from before
    /// the delivery in hand, so without adding it every finite rule delivers
    /// one more message than it was asked for.
    #[tokio::test]
    async fn a_finite_count_delivers_exactly_the_firings_it_named() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let _dispatch = dispatching().await;
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Once only".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=DAILY;COUNT=1".into()),
                prompt: None,
                timing_mode: None,
            },
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

        sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert_ne!(
            reminders::deliver_next(&pool).await.unwrap(),
            Delivered::Nothing
        );
        assert!(
            reminders::deliver_next(&pool).await.unwrap() == Delivered::Nothing,
            "COUNT=1 must not leave a second firing scheduled"
        );

        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(messages, 1, "COUNT=1 is one message, not two");

        let (status, fired): (String, i32) =
            sqlx::query_as("SELECT status, fired_count FROM reminders WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "expired");
        assert_eq!(fired, 1);
        cleanup(&pool, organization, user).await;
    }

    /// A prompt is a turn, not a message. The claim stores nothing in the chat
    /// and writes the turn down as still owed, because holding a row lock
    /// across a model call is exactly what this split exists to avoid — and
    /// because a handoff that only lived in memory would go with the process
    /// holding it, leaving a schedule that had moved past an occurrence nobody
    /// was ever given.
    #[tokio::test]
    async fn a_prompt_is_written_down_as_a_turn_and_stores_no_message_of_its_own() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let _dispatch = dispatching().await;
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "unused when a prompt is given".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some(
                    "Say what changed since yesterday. If nothing did, say nothing.".into(),
                ),
                timing_mode: None,
            },
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

        sqlx::query(
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second', \
             anchor_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

        let delivered = reminders::deliver_next(&pool).await.unwrap();
        assert_eq!(delivered, Delivered::Settled);
        let turn = reminders::claim_turn(&pool, LEASE)
            .await
            .unwrap()
            .expect("a prompt leaves a turn to run");
        assert_eq!(turn.chat_id, chat_id);
        assert_eq!(turn.workspace_id, workspace);
        assert_eq!(turn.user_id, user, "the turn runs as whoever asked for it");
        assert_eq!(turn.reminder_id, id);
        assert!(turn.prompt.starts_with("Say what changed"), "{turn:?}");
        assert!(
            reminders::claim_turn(&pool, LEASE).await.unwrap().is_none(),
            "a claimed turn is not offered to a second worker inside its lease"
        );

        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            messages, 0,
            "the content must not be posted beside the turn the prompt opens"
        );

        // The schedule still moved on, and still holds no message of its own.
        let (status, fired, message_id): (String, i32, Option<Uuid>) =
            sqlx::query_as("SELECT status, fired_count, message_id FROM reminders WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(fired, 1);
        assert_eq!(message_id, None);

        reminders::finish_turn(&pool, turn.id).await.unwrap();
        assert!(
            reminders::claim_turn(&pool, LEASE).await.unwrap().is_none(),
            "a turn that has run is not owed again"
        );
        cleanup(&pool, organization, user).await;
    }

    /// The gap this table exists for. A process that claims a firing and dies
    /// before the turn finishes leaves a schedule that has moved past an
    /// occurrence and a chat with nothing in it, so the firing is offered again
    /// once its claim goes stale — and given up on rather than offered for
    /// ever, because a firing that takes the server down every time it runs is
    /// a crash loop and not a delivery.
    #[tokio::test]
    async fn a_turn_whose_worker_died_is_offered_again_and_then_given_up_on() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Overnight check".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: None,
                prompt: Some("Say what changed overnight.".into()),
                timing_mode: None,
            },
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
        sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        reminders::deliver_next(&pool).await.unwrap();

        // Three claims, each abandoned: the lease of zero is a worker that
        // never came back, without a test that has to wait out a real one.
        for attempt in 1..=3 {
            let turn = reminders::claim_turn(&pool, ABANDONED)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("attempt {attempt} must be offered the turn"));
            assert_eq!(turn.reminder_id, id);
        }
        assert!(
            reminders::claim_turn(&pool, ABANDONED)
                .await
                .unwrap()
                .is_none(),
            "a turn that has used every attempt is dropped rather than offered a fourth time"
        );
        let owed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM reminder_turns WHERE reminder_id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            owed, 0,
            "the queue does not silt up with firings nothing will run"
        );
        cleanup(&pool, organization, user).await;
    }

    /// Stopping a standing job stops the firing it has already claimed as well.
    /// Being answered once more by something just cancelled reads as the cancel
    /// not having worked.
    #[tokio::test]
    async fn cancelling_a_schedule_drops_the_turn_it_has_not_run_yet() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Hourly check".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=HOURLY".into()),
                prompt: Some("Say what changed in the last hour.".into()),
                timing_mode: None,
            },
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
        sqlx::query(
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second', \
             anchor_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        reminders::deliver_next(&pool).await.unwrap();

        reminders::cancel(&pool, workspace, user, id).await.unwrap();
        assert!(
            reminders::claim_turn(&pool, LEASE).await.unwrap().is_none(),
            "a cancelled schedule owes nothing, including what it had already claimed"
        );
        cleanup(&pool, organization, user).await;
    }

    /// A schedule that has outlived its lifetime ends as `expired`, which is a
    /// different ending from a delivery and different again from a cancel, so a
    /// reader can tell a schedule that ran out from one somebody stopped.
    #[tokio::test]
    async fn an_automation_past_its_lifetime_expires_rather_than_firing_for_ever() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Every morning".into(),
                due_at: Utc::now() + Duration::hours(1),
                rrule: Some("FREQ=DAILY".into()),
                prompt: None,
                timing_mode: None,
            },
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

        sqlx::query(
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second', \
             expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        assert_ne!(
            reminders::deliver_next(&pool).await.unwrap(),
            Delivered::Nothing
        );

        let (status, completed): (String, Option<chrono::DateTime<Utc>>) =
            sqlx::query_as("SELECT status, completed_at FROM reminders WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "expired");
        assert!(
            completed.is_some(),
            "an ended schedule records when it ended"
        );
        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            messages, 0,
            "a schedule nobody renewed must not get one more message on its way out"
        );
        assert!(
            reminders::deliver_next(&pool).await.unwrap() == Delivered::Nothing,
            "an expired schedule is never claimed again"
        );
        cleanup(&pool, organization, user).await;
    }

    #[tokio::test]
    async fn reminders_cancel_revoke_and_deliver_once_across_workers() {
        let _dispatch = dispatching().await;
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let reminder = || reminders::Reminder {
            content: "Check release".into(),
            due_at: Utc::now() + Duration::hours(1),
            rrule: None,
            prompt: None,
            timing_mode: None,
        };
        assert!(
            reminders::create(&pool, workspace, user, Uuid::new_v4(), reminder())
                .await
                .is_err()
        );
        let first = reminders::create(&pool, workspace, user, chat_id, reminder())
            .await
            .unwrap();
        let first_id: Uuid = serde_json::from_value(first["id"].clone()).unwrap();
        assert!(
            reminders::cancel(&pool, Uuid::new_v4(), user, first_id)
                .await
                .is_err()
        );
        reminders::cancel(&pool, workspace, user, first_id)
            .await
            .unwrap();
        reminders::create(&pool, workspace, user, chat_id, reminder())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE workspace_id = $1",
        )
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
        let (first, second) = tokio::join!(
            reminders::deliver_next(&pool),
            reminders::deliver_next(&pool)
        );
        let claimed = |outcome: Delivered| usize::from(outcome != Delivered::Nothing);
        assert_eq!(claimed(first.unwrap()) + claimed(second.unwrap()), 1);
        assert_eq!(
            reminders::deliver_next(&pool).await.unwrap(),
            Delivered::Nothing
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        reminders::create(&pool, workspace, user, chat_id, reminder())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE workspace_id = $1",
        )
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1")
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        assert_ne!(
            reminders::deliver_next(&pool).await.unwrap(),
            Delivered::Nothing
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM reminders WHERE workspace_id = $1 AND status = 'pending'",
        )
        .bind(workspace)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pending, 0);
        cleanup(&pool, organization, user).await;
    }
    #[tokio::test]
    async fn admission_rolls_back_task_when_run_insert_fails() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let name = format!("admission_{}", workspace.simple());
        let function = format!(
            "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF EXISTS(SELECT 1 FROM tasks WHERE id=NEW.task_id AND workspace_id='{workspace}') THEN RAISE EXCEPTION 'injected admission failure'; END IF; RETURN NEW; END $$"
        );
        sqlx::query(sqlx::AssertSqlSafe(function))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {name} BEFORE INSERT ON task_runs FOR EACH ROW EXECUTE FUNCTION {name}()"))).execute(&pool).await.unwrap();
        let result = start_task(
            &pool,
            workspace,
            user,
            serde_json::from_value(
                json!({"title":"atomic admission","description":"Inject a run failure"}),
            )
            .unwrap(),
        )
        .await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TRIGGER {name} ON task_runs"
        )))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP FUNCTION {name}()")))
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected admission failure")
        );
        assert_eq!(count, 0, "failed run admission left a committed task");
    }

    #[tokio::test]
    async fn admission_rechecks_concurrent_revocation() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let mut revocation = pool.begin().await.unwrap();
        sqlx::query(
            "UPDATE workspace_members SET is_active=false WHERE workspace_id=$1 AND user_id=$2",
        )
        .bind(workspace)
        .bind(user)
        .execute(&mut *revocation)
        .await
        .unwrap();
        let connection = pool.clone();
        let creation = tokio::spawn(async move {
            super::super::tasks::create_task_as(
                &connection,
                workspace,
                &[],
                "revoked",
                "",
                None,
                None,
                true,
                None,
                Some(user),
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        revocation.commit().await.unwrap();
        let result = creation.await.unwrap();
        cleanup(&pool, organization, user).await;
        assert!(
            result.is_err(),
            "admission bypassed the concurrent membership revocation"
        );
    }

    /// The join table stores no ordinal, so the order rows come back in is the
    /// planner's choice. This asserted the order they were passed in, which held
    /// only when two random ids happened to be ascending -- a coin flip per run.
    /// The read is ordered by id now, so a task's projects do not reshuffle
    /// between reads, and duplicates still collapse.
    #[tokio::test]
    async fn admission_deduplicates_projects_into_a_stable_order() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
        ids.sort();
        let [lower, higher] = ids;
        let first = higher;
        let second = lower;
        for project in [first, second] {
            sqlx::query("INSERT INTO projects(id,workspace_id,name) VALUES($1,$2,'Admission')")
                .bind(project)
                .bind(workspace)
                .execute(&pool)
                .await
                .unwrap();
        }
        let result = super::super::tasks::create_task_as(
            &pool,
            workspace,
            &[second, first, second],
            "projects",
            "",
            None,
            None,
            true,
            None,
            Some(user),
        )
        .await;
        let result = match result {
            Ok(task) => super::super::tasks::update_task(
                &pool,
                task.id,
                None,
                None,
                None,
                None,
                None,
                Some(&[first, second, first]),
            )
            .await
            .map(|updated| (task, updated)),
            Err(error) => Err(error),
        };
        cleanup(&pool, organization, user).await;
        let (created, updated) = result.expect("duplicate project IDs must be accepted");
        // Passed in as [second, first, second] and [first, second, first]; the
        // duplicates collapse and what comes back is ordered by id either way.
        assert_eq!(created.project_ids, vec![lower, higher]);
        assert_eq!(updated.unwrap().project_ids, vec![lower, higher]);
    }
    #[tokio::test]
    async fn admission_cursor_uses_log_order_and_rejects_foreign_ids() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let task = super::super::tasks::create_task_as(
            &pool,
            workspace,
            &[],
            "cursor",
            "",
            None,
            None,
            true,
            None,
            Some(user),
        )
        .await
        .unwrap();
        let run = super::super::tasks::create_task_run_as(&pool, task.id, Some(user))
            .await
            .unwrap();
        let high = Uuid::from_u128(u128::MAX - 1);
        let low = Uuid::from_u128(1);
        for (id, offset) in [(high, 0), (low, 1)] {
            sqlx::query("INSERT INTO task_run_logs(id,task_run_id,phase,agent_type,log_level,message,created_at) VALUES($1,$2,'test','test','info','line',TIMESTAMP '2026-01-01' + $3 * INTERVAL '1 second')").bind(id).bind(run.id).bind(offset).execute(&pool).await.unwrap();
        }
        let page = tail_task_log(
            &pool,
            workspace,
            user,
            TaskRunLookup {
                run_id: run.id,
                after_log_id: Some(high),
                limit: Some(1),
            },
        )
        .await
        .unwrap();
        let unknown = tail_task_log(
            &pool,
            workspace,
            user,
            TaskRunLookup {
                run_id: run.id,
                after_log_id: Some(Uuid::new_v4()),
                limit: None,
            },
        )
        .await;
        let other = super::super::tasks::create_task_as(
            &pool,
            workspace,
            &[],
            "other",
            "",
            None,
            None,
            true,
            None,
            Some(user),
        )
        .await
        .unwrap();
        let other_run = super::super::tasks::create_task_run_as(&pool, other.id, Some(user))
            .await
            .unwrap();
        let foreign = tail_task_log(
            &pool,
            workspace,
            user,
            TaskRunLookup {
                run_id: other_run.id,
                after_log_id: Some(high),
                limit: None,
            },
        )
        .await;
        cleanup(&pool, organization, user).await;
        assert_eq!(
            page["logs"][0]["id"],
            low.to_string(),
            "UUID value must not determine chronological pagination"
        );
        assert!(unknown.is_err(), "unknown cursor was accepted");
        assert!(foreign.is_err(), "another run's cursor was accepted");
    }

    #[tokio::test]
    async fn admission_revoked_actor_cannot_mutate_existing_tasks() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let task = super::super::tasks::start_task_authorized(
            &pool,
            user,
            super::super::tasks::Create {
                workspace_id: workspace,
                project_ids: &[],
                title: "protected",
                description: "",
                acceptance_criteria: None,
                priority: None,
                is_agentic: true,
                require_plan_approval: false,
                source_id: None,
                created_by: Some(user),
            },
        )
        .await
        .unwrap();
        let super::super::tasks::Mutation::Applied((task, _)) = task else {
            panic!("the fixture actor is a member and must be admitted");
        };
        sqlx::query(
            "UPDATE workspace_members SET is_active=false WHERE workspace_id=$1 AND user_id=$2",
        )
        .bind(workspace)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        let updated = super::super::tasks::update_task_authorized(
            &pool,
            user,
            super::super::tasks::Patch {
                id: task.id,
                title: Some("changed"),
                description: None,
                acceptance_criteria: None,
                status: None,
                priority: None,
                project_ids: None,
                require_plan_approval: None,
            },
        )
        .await;
        let queued = super::super::tasks::queue_task_authorized(&pool, user, task.id).await;
        let admitted = super::super::tasks::create_task_run_as(&pool, task.id, Some(user)).await;
        let deleted = super::super::tasks::delete_task_authorized(&pool, user, task.id).await;
        let row = super::super::tasks::get_task(&pool, task.id).await.unwrap();
        cleanup(&pool, organization, user).await;
        // A revoked member is told the task is gone rather than refused, so the
        // reply cannot be used to confirm it exists.
        assert!(
            matches!(updated, Ok(super::super::tasks::Mutation::NotFound)),
            "a revoked actor must not update"
        );
        assert!(
            matches!(queued, Ok(super::super::tasks::Mutation::NotFound)),
            "a revoked actor must not queue"
        );
        assert!(admitted.is_err(), "a revoked actor must not admit a run");
        assert!(
            matches!(deleted, Ok(super::super::tasks::Mutation::NotFound)),
            "a revoked actor must not delete"
        );
        assert_eq!(row.unwrap().title, "protected");
    }

    #[tokio::test]
    async fn admission_update_deduplicates_and_rejects_foreign_projects() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let project = Uuid::new_v4();
        let foreign_workspace = Uuid::new_v4();
        let foreign_project = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Foreign',$1::text)",
        )
        .bind(foreign_workspace)
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO projects(id,workspace_id,name) VALUES($1,$2,'Foreign')")
            .bind(foreign_project)
            .bind(foreign_workspace)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects(id,workspace_id,name) VALUES($1,$2,'Project')")
            .bind(project)
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        let task = super::super::tasks::create_task_as(
            &pool,
            workspace,
            &[],
            "projects",
            "",
            None,
            None,
            true,
            None,
            Some(user),
        )
        .await
        .unwrap();
        let update = super::super::tasks::update_task(
            &pool,
            task.id,
            None,
            None,
            None,
            None,
            None,
            Some(&[project, project]),
        )
        .await;
        let foreign = super::super::tasks::update_task(
            &pool,
            task.id,
            None,
            None,
            None,
            None,
            None,
            Some(&[foreign_project]),
        )
        .await;
        let associations = super::super::tasks::get_task_project_ids(&pool, task.id)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
        assert_eq!(update.unwrap().unwrap().project_ids, vec![project]);
        assert!(
            matches!(&foreign, Err(sqlx::Error::Protocol(message))
                if message == "Project is not available in this workspace"),
            "a foreign project must be named, not reported as a missing row"
        );
        assert_eq!(
            associations,
            vec![project],
            "invalid update must preserve associations"
        );
    }
    #[tokio::test]
    async fn admission_reads_hold_membership_until_scoped_snapshot() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let task = super::super::tasks::create_task_as(
            &pool,
            workspace,
            &[],
            "snapshot",
            "",
            None,
            None,
            true,
            None,
            Some(user),
        )
        .await
        .unwrap();
        let run = super::super::tasks::create_task_run_as(&pool, task.id, Some(user))
            .await
            .unwrap();
        for tail in [false, true] {
            sqlx::query(
                "UPDATE workspace_members SET is_active=true WHERE workspace_id=$1 AND user_id=$2",
            )
            .bind(workspace)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
            let mut blocker = pool.begin().await.unwrap();
            sqlx::query("SELECT id FROM tasks WHERE id=$1 FOR UPDATE")
                .bind(task.id)
                .fetch_one(&mut *blocker)
                .await
                .unwrap();
            let reader_name = format!("actions-reader-{}", Uuid::new_v4().simple());
            let connection = named_pool(&reader_name).await;
            let reader = tokio::spawn(async move {
                if tail {
                    tail_task_log(
                        &connection,
                        workspace,
                        user,
                        TaskRunLookup {
                            run_id: run.id,
                            after_log_id: None,
                            limit: None,
                        },
                    )
                    .await
                } else {
                    get_task_run(&connection, workspace, user, run.id).await
                }
            });
            wait_until_blocked(&pool, &reader_name).await;
            let revocation_name = format!("actions-revocation-{}", Uuid::new_v4().simple());
            let connection = named_pool(&revocation_name).await;
            let revocation = tokio::spawn(async move {
                sqlx::query("UPDATE workspace_members SET is_active=false WHERE workspace_id=$1 AND user_id=$2").bind(workspace).bind(user).execute(&connection).await
            });
            wait_until_blocked(&pool, &revocation_name).await;
            let held = !reader.is_finished() && !revocation.is_finished();
            blocker.commit().await.unwrap();
            reader.await.unwrap().unwrap();
            revocation.await.unwrap().unwrap();
            if !held {
                cleanup(&pool, organization, user).await;
                panic!("scoped read released authorization before its snapshot completed");
            }
        }
        cleanup(&pool, organization, user).await;
    }
}
