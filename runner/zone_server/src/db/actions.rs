//! Transactional workspace actions with execution-time membership checks.
use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use tokio::sync::broadcast;
use uuid::Uuid;

static UPDATES: Lazy<broadcast::Sender<(Uuid, Value)>> = Lazy::new(|| broadcast::channel(256).0);
pub fn subscribe() -> broadcast::Receiver<(Uuid, Value)> {
    UPDATES.subscribe()
}
pub fn publish(chat_id: Uuid, message: Value) {
    let _ = UPDATES.send((chat_id, message));
}

use super::DbResult;

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
    let role: Option<String> = sqlx::query_scalar("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND is_active FOR SHARE")
        .bind(workspace_id).bind(user_id).fetch_optional(connection).await?;
    if !matches!(role.as_deref(), Some("owner" | "admin" | "member"))
        && (write || role.as_deref() != Some("viewer"))
    {
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
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    if let Some(source_id) = input.source_id {
        let active: Option<Option<bool>> =
            sqlx::query_scalar("SELECT is_active FROM sources WHERE id = $1 AND workspace_id = $2")
                .bind(source_id)
                .bind(workspace_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if !matches!(active, Some(None) | Some(Some(true))) {
            return Err(invalid("Source not found in this workspace or inactive"));
        }
    }
    transaction.commit().await?;

    let task = super::tasks::create_task(
        pool,
        workspace_id,
        &input.project_ids,
        input.title.trim(),
        input.description.trim(),
        input.acceptance_criteria.as_deref(),
        input.priority,
        true,
        input.source_id,
    )
    .await?;
    let run = super::tasks::create_task_run(pool, task.id).await?;
    Ok(json!({
        "task_id": task.id,
        "run_id": run.id,
        "title": task.title,
        "is_agentic": true,
        "status": run.status,
        "message": "Runner started. Poll get_task_run and tail_task_log; do not claim the work finished."
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
    transaction.commit().await?;
    let run = super::tasks::get_task_run(pool, run_id)
        .await?
        .ok_or_else(|| invalid("Task run not found"))?;
    let task = super::tasks::get_task(pool, run.task_id)
        .await?
        .ok_or_else(|| invalid("Task run not found"))?;
    if task.workspace_id != workspace_id {
        return Err(invalid("Task run not found"));
    }
    Ok(json!({
        "id": run.id,
        "task_id": run.task_id,
        "title": task.title,
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
    transaction.commit().await?;
    let run = super::tasks::get_task_run(pool, input.run_id)
        .await?
        .ok_or_else(|| invalid("Task run not found"))?;
    let task = super::tasks::get_task(pool, run.task_id)
        .await?
        .ok_or_else(|| invalid("Task run not found"))?;
    if task.workspace_id != workspace_id {
        return Err(invalid("Task run not found"));
    }
    let logs = super::tasks::get_task_run_logs(pool, run.id).await?;
    let after = input.after_log_id;
    let limit = input.limit.unwrap_or(50).clamp(1, 200) as usize;
    let selected: Vec<_> = logs
        .into_iter()
        .filter(|log| after.is_none_or(|id| log.id > id))
        .take(limit + 1)
        .collect();
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
    use chrono::{Duration, Utc};

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
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
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
                    mentions: vec![]
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
                    mentions: vec![other_user]
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
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
    async fn reminders_cancel_revoke_and_deliver_once_across_workers() {
        let (pool, organization, workspace, user, chat_id) = fixture().await;
        let reminder = || reminders::Reminder {
            content: "Check release".into(),
            due_at: Utc::now() + Duration::hours(1),
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
        assert_eq!(
            usize::from(first.unwrap()) + usize::from(second.unwrap()),
            1
        );
        assert!(!reminders::deliver_next(&pool).await.unwrap());
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
        assert!(reminders::deliver_next(&pool).await.unwrap());
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
}
