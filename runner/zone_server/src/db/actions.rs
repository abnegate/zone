//! Transactional workspace actions with execution-time membership checks.
use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::DbResult;
use super::tasks::{self, Origin};
use super::workspace_members::{self, WorkspaceRole};

/// The last thing a model reads before it decides what to do while a runner
/// runs, which is why the waiting section pins this one too.
pub(crate) const RUNNER_STARTED: &str = "Runner started. Wait for it with wait_for, then read get_task_run and tail_task_log; do not claim the work finished.";

/// Where a started task tells the model [`RUNNER_STARTED`].
pub(crate) const RUNNER_MESSAGE: &str = "message";

static UPDATES: Lazy<broadcast::Sender<(Uuid, Value)>> = Lazy::new(|| broadcast::channel(256).0);
pub fn subscribe() -> broadcast::Receiver<(Uuid, Value)> {
    UPDATES.subscribe()
}
pub fn publish(chat_id: Uuid, message: Value) {
    let _ = UPDATES.send((chat_id, message));
}

pub(crate) fn patch<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub title: Option<String>,
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
    pub priority: Option<i32>,
    #[serde(default, deserialize_with = "patch")]
    pub assignee_id: Option<Option<Uuid>>,
}

/// `queued` stays out: only a run queues a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Created,
    InProgress,
    Review,
    Complete,
    Blocked,
    Paused,
    Cancelled,
}

impl Status {
    pub const ALL: [Self; 7] = [
        Self::Created,
        Self::InProgress,
        Self::Review,
        Self::Complete,
        Self::Blocked,
        Self::Paused,
        Self::Cancelled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => tasks::CREATED,
            Self::InProgress => tasks::IN_PROGRESS,
            Self::Review => tasks::REVIEW,
            Self::Complete => tasks::COMPLETE,
            Self::Blocked => tasks::BLOCKED,
            Self::Paused => tasks::PAUSED,
            Self::Cancelled => tasks::CANCELLED,
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
    chat_id: Option<Uuid>,
    origin: Origin,
    input: Task,
) -> DbResult<Value> {
    let (title, title_pending) = super::tasks::stored_title(
        super::tasks::given_title(input.title.as_deref()),
        &input.description,
    );
    if title_pending && input.description.trim().is_empty() {
        return Err(invalid(super::tasks::UNTITLED_AND_UNDESCRIBED));
    }
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    if let Some(assignee) = input.assignee_id {
        authorize(&mut transaction, workspace_id, assignee, false).await?;
    }
    let result = sqlx::query_scalar("INSERT INTO tasks (workspace_id, title, description, assignee_id, created_by, title_pending, chat_id, origin) VALUES ($1, $2, $3, $4, $5, $6, (SELECT id FROM chats WHERE id = $7 AND workspace_id = $1), $8) RETURNING to_jsonb(tasks.*)")
        .bind(workspace_id).bind(title).bind(input.description).bind(input.assignee_id).bind(user_id).bind(title_pending).bind(chat_id).bind(origin.as_str()).fetch_one(&mut *transaction).await?;
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
    if input
        .priority
        .is_some_and(|priority| !tasks::PRIORITIES.contains(&priority))
    {
        return Err(invalid(tasks::PRIORITY_OUT_OF_RANGE));
    }
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    if let Some(Some(assignee)) = input.assignee_id {
        authorize(&mut transaction, workspace_id, assignee, false).await?;
    }
    let status = input.status.map(Status::as_str);
    if let Some(status) = status
        && run_owns_status(&mut transaction, workspace_id, input.task_id, status).await?
    {
        return Err(invalid(tasks::ACTIVE_RUN));
    }
    let result = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "UPDATE tasks SET title = COALESCE($3, title), title_pending = title_pending AND $3::text IS NULL, description = COALESCE($4, description), status = COALESCE($5, status), assignee_id = CASE WHEN $6 THEN $7 ELSE assignee_id END, priority = COALESCE($8, priority), started_at = CASE WHEN $5 = 'in_progress' THEN COALESCE(started_at, NOW()) ELSE started_at END, {}, updated_at = NOW() WHERE id = $1 AND workspace_id = $2 AND NOT is_agentic RETURNING to_jsonb(tasks.*)",
        tasks::completion_stamps("$5")
    )))
        .bind(input.task_id).bind(workspace_id).bind(input.title).bind(input.description).bind(status).bind(input.assignee_id.is_some()).bind(input.assignee_id.flatten()).bind(input.priority).fetch_optional(&mut *transaction).await?;
    transaction.commit().await?;
    result.ok_or_else(|| invalid("Task not found or is managed by the task runner"))
}

/// Whether a live run owns the status that `status` would change. Locks the
/// task row so a run that ends after this check writes its status after this
/// transaction, not underneath it.
async fn run_owns_status(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    task_id: Uuid,
    status: &str,
) -> DbResult<bool> {
    let held: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT status, active_run_id FROM tasks WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
    )
    .bind(task_id)
    .bind(workspace_id)
    .fetch_optional(connection)
    .await?;
    Ok(held.is_some_and(|(current, run)| run.is_some() && current != status))
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
    if status.is_some_and(|status| tasks::Status::parse(status).is_none()) {
        return Err(invalid(tasks::UNKNOWN_STATUS));
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
    pub title: Option<String>,
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
    chat_id: Option<Uuid>,
    origin: Origin,
    input: StartTask,
) -> DbResult<Value> {
    if input.description.trim().is_empty() {
        return Err(invalid("Description must not be blank"));
    }
    if input
        .priority
        .is_some_and(|priority| !tasks::PRIORITIES.contains(&priority))
    {
        return Err(invalid(tasks::PRIORITY_OUT_OF_RANGE));
    }
    let (task, run) = match super::tasks::start_task_authorized(
        pool,
        user_id,
        super::tasks::Create {
            workspace_id,
            project_ids: &input.project_ids,
            title: super::tasks::given_title(input.title.as_deref()),
            description: input.description.trim(),
            acceptance_criteria: input.acceptance_criteria.as_deref(),
            priority: input.priority,
            is_agentic: true,
            require_plan_approval: false,
            source_id: input.source_id,
            created_by: Some(user_id),
            chat_id,
            origin,
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
            return Err(invalid(tasks::ACTIVE_RUN));
        }
        Err(super::tasks::MutationError::Database(error)) => return Err(error),
    };
    Ok(json!({
        "task_id": task.id,
        "run_id": run.id,
        "title": task.title,
        "title_pending": task.title_pending,
        "is_agentic": true,
        "status": run.status,
        RUNNER_MESSAGE: RUNNER_STARTED
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
    use crate::db::reminders::Sweep;
    use chrono::Duration;
    use chrono::Utc;
    use std::collections::BTreeSet;

    /// Long enough that a claimed turn stays claimed for the length of a test,
    /// which is what a real lease is for.
    const LEASE: std::time::Duration = std::time::Duration::from_secs(3600);

    /// And a lease nothing can be inside, which is a worker that claimed a turn
    /// and never came back — without a test that has to wait out a real one.
    const ABANDONED: std::time::Duration = std::time::Duration::ZERO;

    fn database() -> String {
        std::env::var("TEST_DATABASE_URL").expect("a disposable, migrated TEST_DATABASE_URL")
    }

    async fn fixture() -> (PgPool, Uuid, Uuid, Uuid, Uuid) {
        let pool = PgPool::connect(&database()).await.unwrap();
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
            .connect(&database())
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

    const UNKNOWN: &str = "archived";

    #[test]
    fn an_agent_may_pause_or_cancel_a_task() {
        let id = Uuid::new_v4();
        for (value, expected) in [
            (tasks::PAUSED, Status::Paused),
            (tasks::CANCELLED, Status::Cancelled),
        ] {
            let update: Update =
                serde_json::from_value(json!({"task_id":id,"status":value})).unwrap();
            assert_eq!(update.status, Some(expected), "{value}");
            assert_eq!(expected.as_str(), value);
        }
        assert!(serde_json::from_value::<Update>(json!({"task_id":id,"status":UNKNOWN})).is_err());
    }

    #[test]
    fn an_agent_may_set_every_status_but_queued() {
        let mut settable: BTreeSet<&str> = Status::ALL.map(Status::as_str).into();
        assert!(
            settable.insert(tasks::QUEUED),
            "an agent must not queue a task"
        );
        assert_eq!(settable, BTreeSet::from(tasks::STATUSES));
    }

    /// serde names every variant when it refuses one, and that is the only
    /// list of them the compiler keeps: a variant added without a place in
    /// `ALL` is one the tool schema never offers.
    #[test]
    fn all_lists_every_status_an_agent_can_send() {
        let refusal = serde_json::from_value::<Status>(json!(UNKNOWN))
            .unwrap_err()
            .to_string();
        let (_, variants) = refusal
            .split_once("expected one of ")
            .unwrap_or_else(|| panic!("serde lists the variants: {refusal}"));
        let named: BTreeSet<&str> = variants
            .split(", ")
            .map(|name| name.trim_matches('`'))
            .collect();

        assert_eq!(
            named,
            BTreeSet::from(Status::ALL.map(Status::as_str)),
            "{refusal}"
        );
    }

    #[tokio::test]
    async fn listing_filters_on_paused_and_cancelled_and_refuses_an_unknown_status() {
        let (pool, organization, workspace, user, _chat) = fixture().await;
        let mut seeded = Vec::new();
        for status in [tasks::PAUSED, tasks::CANCELLED, tasks::CREATED] {
            let created = create_task(
                &pool,
                workspace,
                user,
                None,
                Origin::Member,
                Task {
                    title: Some(status.into()),
                    description: "Listed by status".into(),
                    assignee_id: None,
                },
            )
            .await
            .unwrap();
            let id = created["id"].clone();
            let moved: Update =
                serde_json::from_value(json!({"task_id":id,"status":status})).unwrap();
            update_task(&pool, workspace, user, moved).await.unwrap();
            seeded.push((status, id));
        }

        for (status, id) in &seeded {
            let listed = list_tasks(&pool, workspace, user, Some(*status))
                .await
                .unwrap();
            let listed: Vec<&Value> = listed
                .as_array()
                .expect("a list of tasks")
                .iter()
                .map(|task| &task["id"])
                .collect();
            assert_eq!(listed, vec![id], "only the {status} task is listed");
        }
        let everything = list_tasks(&pool, workspace, user, None).await.unwrap();
        assert_eq!(everything.as_array().map(Vec::len), Some(seeded.len()));
        let unknown = list_tasks(&pool, workspace, user, Some(UNKNOWN)).await;
        assert!(
            matches!(&unknown, Err(sqlx::Error::Protocol(message)) if message == tasks::UNKNOWN_STATUS),
            "{unknown:?}"
        );
        cleanup(&pool, organization, user).await;
    }

    #[tokio::test]
    async fn an_agent_cannot_change_the_status_of_a_task_with_a_live_run() {
        let (pool, organization, workspace, user, _chat) = fixture().await;
        let created = create_task(
            &pool,
            workspace,
            user,
            None,
            Origin::Member,
            Task {
                title: Some("Manual".into()),
                description: "Run by hand".into(),
                assignee_id: None,
            },
        )
        .await
        .unwrap();
        let task_id = serde_json::from_value::<Uuid>(created["id"].clone()).unwrap();
        let run = tasks::create_task_run(&pool, task_id).await.unwrap();
        let pause: Update =
            serde_json::from_value(json!({"task_id":task_id,"status":tasks::PAUSED})).unwrap();
        let refused = update_task(&pool, workspace, user, pause).await;
        assert!(
            matches!(&refused, Err(sqlx::Error::Protocol(message)) if message == tasks::ACTIVE_RUN),
            "{refused:?}"
        );
        let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_ne!(status, tasks::PAUSED, "the run still owns the status");
        let rename: Update =
            serde_json::from_value(json!({"task_id":task_id,"title":"Renamed"})).unwrap();
        let renamed = update_task(&pool, workspace, user, rename).await.unwrap();
        assert_eq!(
            renamed["title"], "Renamed",
            "an update that leaves status alone lands"
        );
        assert_eq!(renamed["status"], json!(status));

        assert!(tasks::start_task_run(&pool, run.id).await.unwrap());
        let restated: Update = serde_json::from_value(
            json!({"task_id":task_id,"status":tasks::IN_PROGRESS,"title":"Restated"}),
        )
        .unwrap();
        let restated = update_task(&pool, workspace, user, restated)
            .await
            .expect("a status the task already has changes nothing a run owns");
        assert_eq!(restated["title"], "Restated");
        assert_eq!(restated["status"], tasks::IN_PROGRESS);
        cleanup(&pool, organization, user).await;
    }

    #[tokio::test]
    async fn actions_enforce_workspace_roles_sparse_updates_and_mentions() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let created = create_task(
            &pool,
            workspace,
            user,
            None,
            Origin::Member,
            Task {
                title: Some("Ship".into()),
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
        assert!(!complete["shipped_at"].is_null());
        assert!(complete["reopened_at"].is_null());
        let update: Update = serde_json::from_value(
            json!({"task_id":task_id,"status":"created","assignee_id":null}),
        )
        .unwrap();
        let reopened = update_task(&pool, workspace, user, update).await.unwrap();
        assert!(reopened["assignee_id"].is_null());
        assert!(reopened["completed_at"].is_null());
        assert_eq!(
            reopened["shipped_at"], complete["shipped_at"],
            "the move back out stays visible as a reopen"
        );
        assert!(!reopened["reopened_at"].is_null(), "and is dated");
        let raised: Update =
            serde_json::from_value(json!({"task_id":task_id,"priority":1})).unwrap();
        let raised = update_task(&pool, workspace, user, raised).await.unwrap();
        assert_eq!(
            raised["reopened_at"], reopened["reopened_at"],
            "a later write leaves the date of the move alone"
        );
        assert_eq!(raised["priority"], json!(1));
        assert_eq!(
            raised["status"],
            json!("created"),
            "priority alone changes nothing else"
        );
        let out_of_range: Update =
            serde_json::from_value(json!({"task_id":task_id,"priority":9})).unwrap();
        assert!(
            update_task(&pool, workspace, user, out_of_range)
                .await
                .is_err()
        );
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
                None,
                Origin::Member,
                Task {
                    title: Some("No".into()),
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
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: rrule.map(str::to_string),
                prompt: prompt.map(str::to_string),
                timing_mode: mode.map(str::to_string),
                time_zone: None,
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
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let watch = |name: &str| reminders::Reminder {
            content: name.into(),
            due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
            rrule: Some("FREQ=DAILY".into()),
            prompt: Some("Check whether the release branch is green".into()),
            timing_mode: Some("condition_watch".into()),
            time_zone: None,
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
        let first = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the oldest firing is claimable");
        let second = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the other schedule is not held up by the first");
        assert_ne!(
            first.reminder_id, second.reminder_id,
            "a schedule with a firing in flight must not be handed its next one"
        );
        assert!(
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
            "both schedules now have a firing in flight, so neither offers its second"
        );

        // And once a firing is done, the one behind it is offered — to the same
        // schedule, so the next comparison is against what the first recorded.
        reminders::finish_turn(&pool, first.id, first.attempts)
            .await
            .unwrap();
        let next = reminders::claim_turn(&pool, sweep, LEASE)
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
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Release branch".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some("Check whether the release branch is green".into()),
                timing_mode: Some("condition_watch".into()),
                time_zone: None,
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

        let third = reminders::claim_turn(&pool, sweep, LEASE)
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
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
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
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some("Check whether the release branch is green".into()),
                timing_mode: Some("condition_watch".into()),
                time_zone: None,
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
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some("Say what is on today".into()),
                timing_mode: None,
                time_zone: None,
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
        sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(watch_id)
            .execute(&pool)
            .await
            .unwrap();
        reminders::deliver_next(&pool, Sweep::Workspace(workspace))
            .await
            .unwrap();
        let turn = reminders::claim_turn(&pool, Sweep::Workspace(workspace), LEASE)
            .await
            .unwrap()
            .expect("the watch's firing is claimable");
        assert_eq!(
            reminders::record_observation(&pool, &turn, Uuid::new_v4(), LEASE)
                .await
                .unwrap(),
            reminders::Observation::Unanswered
        );
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
        assert_eq!(
            reminders::record_observation(&pool, &turn, answered, LEASE)
                .await
                .unwrap(),
            reminders::Observation::Kept
        );
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

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(watch_id)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    /// The difference this whole change exists for: a one-shot is finished when
    /// it fires, and an automation moves to its next occurrence and stays
    /// pending. Both deliver exactly one message per firing.
    #[tokio::test]
    async fn an_automation_moves_to_its_next_firing_where_a_one_shot_is_finished() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let one_shot = reminders::Reminder {
            content: "Once".into(),
            due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: None,
            timing_mode: None,
            time_zone: None,
        };
        let daily = reminders::Reminder {
            content: "Every morning".into(),
            due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
            rrule: Some("FREQ=DAILY".into()),
            // No prompt: this test is about a schedule moving on rather than
            // ending, and a prompt would replace the message it counts with a
            // turn. The prompt path has its own test.
            prompt: None,
            timing_mode: Some("exact_schedule".into()),
            time_zone: None,
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
            reminders::deliver_next(&pool, sweep).await.unwrap(),
            Delivered::Nothing
        );
        assert_ne!(
            reminders::deliver_next(&pool, sweep).await.unwrap(),
            Delivered::Nothing
        );
        assert!(
            reminders::deliver_next(&pool, sweep).await.unwrap() == Delivered::Nothing,
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
        let sweep = Sweep::Workspace(workspace);
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Once only".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=DAILY;COUNT=1".into()),
                prompt: None,
                timing_mode: None,
                time_zone: None,
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
            reminders::deliver_next(&pool, sweep).await.unwrap(),
            Delivered::Nothing
        );
        assert!(
            reminders::deliver_next(&pool, sweep).await.unwrap() == Delivered::Nothing,
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
        let sweep = Sweep::Workspace(workspace);
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "unused when a prompt is given".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=DAILY".into()),
                prompt: Some(
                    "Say what changed since yesterday. If nothing did, say nothing.".into(),
                ),
                timing_mode: None,
                time_zone: None,
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

        let delivered = reminders::deliver_next(&pool, sweep).await.unwrap();
        assert_eq!(delivered, Delivered::Settled);
        let turn = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("a prompt leaves a turn to run");
        assert_eq!(turn.chat_id, chat_id);
        assert_eq!(turn.workspace_id, workspace);
        assert_eq!(turn.user_id, user, "the turn runs as whoever asked for it");
        assert_eq!(turn.reminder_id, id);
        assert!(turn.prompt.starts_with("Say what changed"), "{turn:?}");
        assert!(
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
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

        reminders::finish_turn(&pool, turn.id, turn.attempts)
            .await
            .unwrap();
        assert!(
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
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
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Overnight check".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: None,
                prompt: Some("Say what changed overnight.".into()),
                timing_mode: None,
                time_zone: None,
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
        reminders::deliver_next(&pool, sweep).await.unwrap();

        // Three claims, each abandoned: the lease of zero is a worker that
        // never came back, without a test that has to wait out a real one.
        for attempt in 1..=3 {
            let turn = reminders::claim_turn(&pool, sweep, ABANDONED)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("attempt {attempt} must be offered the turn"));
            assert_eq!(turn.reminder_id, id);
        }
        assert!(
            reminders::claim_turn(&pool, sweep, ABANDONED)
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

    /// One owed firing of a one-shot prompt reminder, written down the way a
    /// claim writes it.
    async fn owed_turn(pool: &PgPool, workspace: Uuid, user: Uuid, chat_id: Uuid) -> Uuid {
        let created = reminders::create(
            pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Long investigation".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: None,
                prompt: Some("Work out why the nightly build is slow.".into()),
                timing_mode: None,
                time_zone: None,
            },
        )
        .await
        .unwrap();
        let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
        sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        reminders::deliver_next(pool, Sweep::Workspace(workspace))
            .await
            .unwrap();
        id
    }

    /// Ages a claim past any lease a test uses, the way a worker that went
    /// quiet would.
    async fn backdate(pool: &PgPool, turn: Uuid) {
        age(pool, turn, LEASE * 2).await;
    }

    async fn age(pool: &PgPool, turn: Uuid, by: std::time::Duration) {
        sqlx::query(
            "UPDATE reminder_turns SET claimed_at = NOW() - make_interval(secs => $2) \
             WHERE id = $1",
        )
        .bind(turn)
        .bind(by.as_secs_f64())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn claimed_at(pool: &PgPool, turn: Uuid) -> chrono::DateTime<Utc> {
        sqlx::query_scalar("SELECT claimed_at FROM reminder_turns WHERE id = $1")
            .bind(turn)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// A second firing of `reminder`, owed behind the first.
    async fn sibling_turn(pool: &PgPool, reminder: Uuid) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, prompt) \
             SELECT reminder_id, workspace_id, chat_id, created_by, prompt FROM reminder_turns \
             WHERE reminder_id = $1 RETURNING id",
        )
        .bind(reminder)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// A claim whose lease lapsed is one `claim_turn` treats as gone, and the
    /// schedule's next firing may already have started on the strength of it.
    /// Renewing the lapsed claim then would put two of one schedule's firings
    /// in flight, which is what running them one at a time rules out.
    #[tokio::test]
    async fn a_lapsed_claim_is_not_revived_once_a_sibling_firing_is_running() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;
        let first = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the owed firing is claimable");
        let sibling = sibling_turn(&pool, reminder).await;
        backdate(&pool, first.id).await;

        let mut renewal_in_flight = pool.begin().await.unwrap();
        sqlx::query("SELECT 1 FROM reminder_turns WHERE id = $1 FOR UPDATE")
            .bind(first.id)
            .execute(&mut *renewal_in_flight)
            .await
            .unwrap();
        let second = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("a lapsed claim does not hold its sibling back");
        assert_eq!(second.id, sibling, "the sibling firing is the one taken");
        renewal_in_flight.rollback().await.unwrap();
        let before = claimed_at(&pool, first.id).await;

        assert!(
            !reminders::renew_turn(&pool, first.id, first.attempts, LEASE)
                .await
                .unwrap(),
            "a lapsed claim is not the worker's to renew"
        );
        assert_eq!(
            claimed_at(&pool, first.id).await,
            before,
            "the lapsed claim stays lapsed while its sibling runs"
        );

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    /// A renewal that waits behind a claim judges its lease when it runs, not
    /// when it started waiting: the claim may have found it lapsed in between
    /// and started the schedule's next firing.
    #[tokio::test]
    async fn a_renewal_waiting_on_a_claim_judges_its_lease_after_the_claim() {
        const SHORT: std::time::Duration = std::time::Duration::from_secs(1);
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;
        let first = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the owed firing is claimable");
        let sibling = sibling_turn(&pool, reminder).await;

        let mut claiming = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(reminders::CLAIM_LOCK)
            .execute(&mut *claiming)
            .await
            .unwrap();
        let renewal = tokio::spawn({
            let pool = pool.clone();
            async move { reminders::renew_turn(&pool, first.id, first.attempts, SHORT).await }
        });
        tokio::time::sleep(SHORT * 2).await;
        sqlx::query(
            "UPDATE reminder_turns SET attempts = 1, claimed_at = clock_timestamp() WHERE id = $1",
        )
        .bind(sibling)
        .execute(&mut *claiming)
        .await
        .unwrap();
        claiming.commit().await.unwrap();

        assert!(
            !renewal.await.unwrap().unwrap(),
            "a claim that lapsed while its renewal waited is not revived"
        );

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    /// A claim that waited on another worker's longer than its lease is stamped
    /// when it is taken, not when it began waiting, so it does not start out
    /// lapsed and refused to the worker that took it.
    ///
    /// Compared against the moment the lock was let go rather than by renewing
    /// it: other tests take the same lock, and a renewal queued behind one of
    /// them could outwait a short lease whatever the claim was stamped with.
    #[tokio::test]
    async fn a_claim_that_waited_on_the_lock_starts_its_lease_when_taken() {
        const SHORT: std::time::Duration = std::time::Duration::from_secs(1);
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;

        let mut holding = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(reminders::CLAIM_LOCK)
            .execute(&mut *holding)
            .await
            .unwrap();
        let claim = tokio::spawn({
            let pool = pool.clone();
            async move { reminders::claim_turn(&pool, sweep, SHORT).await }
        });
        tokio::time::sleep(SHORT * 2).await;
        let released: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *holding)
            .await
            .unwrap();
        holding.commit().await.unwrap();
        let turn = claim
            .await
            .unwrap()
            .unwrap()
            .expect("the owed firing is claimed once the lock is free");
        let claimed = claimed_at(&pool, turn.id).await;

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;

        assert!(
            claimed >= released,
            "a claim that waited {SHORT:?} and more on the lock is stamped when it was taken \
             ({claimed}), not before the lock was let go ({released})"
        );
    }

    /// A turn has no length limit, so the lease is a heartbeat rather than a
    /// guess at how long a turn takes: a claim that keeps being renewed is
    /// never offered to a second worker however old it is, and one that stops
    /// being renewed is offered again once its lease runs out.
    #[tokio::test]
    async fn a_renewed_claim_is_not_offered_again_after_its_lease() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;
        let turn = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the owed firing is claimable");
        assert_eq!(turn.attempts, 1, "the claim names which attempt it is");

        age(&pool, turn.id, LEASE - std::time::Duration::from_secs(60)).await;
        assert!(
            reminders::renew_turn(&pool, turn.id, turn.attempts, LEASE)
                .await
                .unwrap(),
            "the worker holding the claim renews it"
        );
        assert!(
            claimed_at(&pool, turn.id).await > Utc::now() - Duration::minutes(1),
            "a renewal starts the lease over"
        );
        assert!(
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
            "a turn still running and renewing its claim is not handed to a second worker"
        );

        backdate(&pool, turn.id).await;
        let again = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("a claim nobody renewed lapses and the firing is offered again");
        assert_eq!(again.id, turn.id);
        assert_eq!(again.attempts, 2, "the new claim is the next attempt");

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    /// A worker whose claim lapsed and was taken over must not keep the new
    /// claim alive: it is not the one running the turn, and if the worker that
    /// is dies, its renewals would hold the firing for ever.
    #[tokio::test]
    async fn a_superseded_claim_is_not_renewed() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;
        let first = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the owed firing is claimable");
        backdate(&pool, first.id).await;
        let second = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the lapsed claim is taken over");
        assert_eq!(second.id, first.id);
        age(&pool, second.id, LEASE - std::time::Duration::from_secs(60)).await;
        let before = claimed_at(&pool, first.id).await;

        assert!(
            !reminders::renew_turn(&pool, first.id, first.attempts, LEASE)
                .await
                .unwrap(),
            "the first worker's claim is gone, so its renewal says so"
        );
        assert_eq!(
            claimed_at(&pool, first.id).await,
            before,
            "a stale renewal leaves the newer claim's lease alone"
        );
        assert!(
            reminders::renew_turn(&pool, second.id, second.attempts, LEASE)
                .await
                .unwrap(),
            "the newer claim is still inside its lease, so only the attempt turned the stale renewal away"
        );

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    /// A turn still running when its claim lapsed may already have a successor:
    /// `claim_turn` started the schedule's next firing against the baseline as
    /// it stood. The late reading must not replace that baseline, or the
    /// successor's comparison and its own reading are both undone.
    #[tokio::test]
    async fn a_superseded_turn_does_not_overwrite_the_baseline_its_successor_reads() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;
        sqlx::query(
            "UPDATE reminders SET timing_mode = 'condition_watch', rrule = 'FREQ=DAILY' \
             WHERE id = $1",
        )
        .bind(reminder)
        .execute(&pool)
        .await
        .unwrap();
        let first = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the owed firing is claimable");
        let sibling = sibling_turn(&pool, reminder).await;
        backdate(&pool, first.id).await;
        let mut first_still_running = pool.begin().await.unwrap();
        sqlx::query("SELECT 1 FROM reminder_turns WHERE id = $1 FOR UPDATE")
            .bind(first.id)
            .execute(&mut *first_still_running)
            .await
            .unwrap();
        let second = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the next firing starts once the first one's claim lapsed");
        first_still_running.rollback().await.unwrap();
        assert_eq!(second.id, sibling, "the sibling firing is the one taken");
        let stale = assistant_message(&pool, chat_id, "The build is red.").await;
        let current = assistant_message(&pool, chat_id, "The build is green.").await;

        let late = reminders::record_observation(&pool, &first, stale, LEASE)
            .await
            .unwrap();
        let baseline_after_late = reminders::watch_baseline(&pool, reminder).await.unwrap();
        let kept = reminders::record_observation(&pool, &second, current, LEASE)
            .await
            .unwrap();
        let baseline_after_kept = reminders::watch_baseline(&pool, reminder).await.unwrap();

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;

        assert_eq!(late, reminders::Observation::Superseded);
        assert_eq!(
            baseline_after_late,
            Some(None),
            "a turn that lost its claim leaves the baseline its successor reads"
        );
        assert_eq!(kept, reminders::Observation::Kept);
        assert_eq!(
            baseline_after_kept,
            Some(Some("The build is green.".to_owned())),
            "the turn holding the claim records its reading"
        );
    }

    async fn assistant_message(pool: &PgPool, chat_id: Uuid, content: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO messages (id, chat_id, role, content) \
             VALUES (gen_random_uuid(), $1, 'assistant', $2) RETURNING id",
        )
        .bind(chat_id)
        .bind(content)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// A worker whose claim lapsed and was taken over finishes only its own
    /// claim: the newer one stays for the worker running it to finish.
    #[tokio::test]
    async fn a_superseded_claim_does_not_finish_the_newer_one() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = owed_turn(&pool, workspace, user, chat_id).await;
        let first = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the owed firing is claimable");
        backdate(&pool, first.id).await;
        let second = reminders::claim_turn(&pool, sweep, LEASE)
            .await
            .unwrap()
            .expect("the lapsed claim is taken over");
        let remaining = |pool: PgPool, id: Uuid| async move {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reminder_turns WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
        };

        reminders::finish_turn(&pool, first.id, first.attempts)
            .await
            .unwrap();
        assert_eq!(
            remaining(pool.clone(), second.id).await,
            1,
            "a superseded worker finished the claim another worker is running"
        );

        reminders::finish_turn(&pool, second.id, second.attempts)
            .await
            .unwrap();
        assert_eq!(
            remaining(pool.clone(), second.id).await,
            0,
            "the worker holding the claim finishes it"
        );

        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder)
            .execute(&pool)
            .await
            .unwrap();
        cleanup(&pool, organization, user).await;
    }

    #[tokio::test]
    async fn a_workspace_sweep_leaves_every_other_workspace_s_firings_alone() {
        let (pool, organization, workspace, user, _) = fixture().await;
        let (_, other_organization, other_workspace, other_user, other_chat) = fixture().await;
        let reminder = |prompt: Option<&str>| reminders::Reminder {
            content: "Elsewhere".into(),
            due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: prompt.map(Into::into),
            timing_mode: None,
            time_zone: None,
        };
        reminders::create(
            &pool,
            other_workspace,
            other_user,
            other_chat,
            reminder(None),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE workspace_id = $1",
        )
        .bind(other_workspace)
        .execute(&pool)
        .await
        .unwrap();
        let created = reminders::create(
            &pool,
            other_workspace,
            other_user,
            other_chat,
            reminder(Some("Say what changed.")),
        )
        .await
        .unwrap();
        let watch: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
        for (attempts, claimed) in [(0, None), (3, Some(Utc::now() - Duration::days(1)))] {
            sqlx::query(
                "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, \
                 prompt, attempts, claimed_at) VALUES ($1, $2, $3, $4, 'Check it', $5, $6)",
            )
            .bind(watch)
            .bind(other_workspace)
            .bind(other_chat)
            .bind(other_user)
            .bind(attempts)
            .bind(claimed)
            .execute(&pool)
            .await
            .unwrap();
        }

        let sweep = Sweep::Workspace(workspace);
        assert_eq!(
            reminders::deliver_next(&pool, sweep).await.unwrap(),
            Delivered::Nothing,
            "another workspace's due reminder is not this sweep's to deliver"
        );
        assert!(
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
            "another workspace's owed turn is not this sweep's to claim"
        );
        let owed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM reminder_turns WHERE workspace_id = $1")
                .bind(other_workspace)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            owed, 2,
            "another workspace's spent turn is not this sweep's to give up on"
        );

        let delivered = || async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
                .bind(other_chat)
                .fetch_one(&pool)
                .await
                .unwrap()
        };
        assert_eq!(
            delivered().await,
            0,
            "nothing reached the other workspace's chat"
        );

        let elsewhere = Sweep::Workspace(other_workspace);
        assert_eq!(
            reminders::deliver_next(&pool, elsewhere).await.unwrap(),
            Delivered::Settled,
            "the reminder is still there for its own workspace's sweep"
        );
        assert_eq!(delivered().await, 1);
        assert_eq!(
            reminders::claim_turn(&pool, elsewhere, LEASE)
                .await
                .unwrap()
                .map(|turn| turn.reminder_id),
            Some(watch),
            "the turn is still there for its own workspace's sweep"
        );
        cleanup(&pool, organization, user).await;
        cleanup(&pool, other_organization, other_user).await;
    }

    /// Stopping a standing job stops the firing it has already claimed as well.
    /// Being answered once more by something just cancelled reads as the cancel
    /// not having worked.
    #[tokio::test]
    async fn cancelling_a_schedule_drops_the_turn_it_has_not_run_yet() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Hourly check".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=HOURLY".into()),
                prompt: Some("Say what changed in the last hour.".into()),
                timing_mode: None,
                time_zone: None,
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
        reminders::deliver_next(&pool, sweep).await.unwrap();

        reminders::cancel(&pool, workspace, user, id).await.unwrap();
        assert!(
            reminders::claim_turn(&pool, sweep, LEASE)
                .await
                .unwrap()
                .is_none(),
            "a cancelled schedule owes nothing, including what it had already claimed"
        );
        cleanup(&pool, organization, user).await;
    }

    /// A schedule that has outlived its lifetime ends as `expired`, which is a
    /// different ending from a delivery and different again from a cancel, so a
    /// reader can tell a schedule that ran out from one somebody stopped.
    #[tokio::test]
    async fn an_automation_past_its_lifetime_expires_rather_than_firing_for_ever() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let created = reminders::create(
            &pool,
            workspace,
            user,
            chat_id,
            reminders::Reminder {
                content: "Every morning".into(),
                due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
                rrule: Some("FREQ=DAILY".into()),
                prompt: None,
                timing_mode: None,
                time_zone: None,
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
            reminders::deliver_next(&pool, sweep).await.unwrap(),
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
            reminders::deliver_next(&pool, sweep).await.unwrap() == Delivered::Nothing,
            "an expired schedule is never claimed again"
        );
        cleanup(&pool, organization, user).await;
    }

    #[tokio::test]
    async fn reminders_cancel_revoke_and_deliver_once_across_workers() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let sweep = Sweep::Workspace(workspace);
        let reminder = || reminders::Reminder {
            content: "Check release".into(),
            due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: None,
            timing_mode: None,
            time_zone: None,
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
            reminders::deliver_next(&pool, sweep),
            reminders::deliver_next(&pool, sweep)
        );
        let claimed = |outcome: Delivered| usize::from(outcome != Delivered::Nothing);
        assert_eq!(claimed(first.unwrap()) + claimed(second.unwrap()), 1);
        assert_eq!(
            reminders::deliver_next(&pool, sweep).await.unwrap(),
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
            reminders::deliver_next(&pool, sweep).await.unwrap(),
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
            None,
            Origin::Member,
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
                title: Some("protected"),
                description: "",
                acceptance_criteria: None,
                priority: None,
                is_agentic: true,
                require_plan_approval: false,
                source_id: None,
                created_by: Some(user),
                chat_id: None,
                origin: super::super::tasks::Origin::Member,
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
                is_agentic: None,
                source_id: None,
                model_name: None,
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
