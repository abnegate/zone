//! Authenticated workspace mutations and durable reminders.
use super::tools::WorkspaceScope;
use crate::db::{actions, reminders};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

#[derive(Clone, Copy)]
enum Action {
    ListTasks,
    CreateTask,
    UpdateTask,
    ListMembers,
    ListChats,
    SendMessage,
    CreateReminder,
    ListReminders,
    CancelReminder,
    StartTask,
    GetTaskRun,
    TailTaskLog,
}

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    for action in [
        Action::ListTasks,
        Action::CreateTask,
        Action::UpdateTask,
        Action::ListMembers,
        Action::ListChats,
        Action::SendMessage,
        Action::CreateReminder,
        Action::ListReminders,
        Action::CancelReminder,
        Action::StartTask,
        Action::GetTaskRun,
        Action::TailTaskLog,
    ] {
        registry.register(Arc::new(WorkspaceAction {
            scope: scope.clone(),
            action,
        }));
    }
}

struct WorkspaceAction {
    scope: WorkspaceScope,
    action: Action,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Filter {
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    reminder_id: Uuid,
}

fn decode<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, sqlx::Error> {
    serde_json::from_value(params)
        .map_err(|error| actions::invalid(&format!("Invalid arguments: {error}")))
}

#[async_trait]
impl Tool for WorkspaceAction {
    fn name(&self) -> &str {
        match self.action {
            Action::ListTasks => "list_tasks",
            Action::CreateTask => "create_task",
            Action::UpdateTask => "update_task",
            Action::ListMembers => "list_members",
            Action::ListChats => "list_chats",
            Action::SendMessage => "send_message",
            Action::CreateReminder => "create_reminder",
            Action::ListReminders => "list_reminders",
            Action::CancelReminder => "cancel_reminder",
            Action::StartTask => "start_task",
            Action::GetTaskRun => "get_task_run",
            Action::TailTaskLog => "tail_task_log",
        }
    }
    fn description(&self) -> &str {
        match self.action {
            Action::ListTasks => {
                "List workspace tasks with IDs, assignees, actual status, and timestamps. Optionally filter by status."
            }
            Action::CreateTask => {
                "Create a manual workspace task. Assign using a user ID from list_members. Does not start an agent runner."
            }
            Action::UpdateTask => {
                "Update a manual task, assign/unassign it, or mark complete. Omitted fields stay unchanged; assignee_id null unassigns. Runner-managed tasks cannot be edited here."
            }
            Action::ListMembers => {
                "List active workspace members with user IDs and names for assignment or mentions."
            }
            Action::ListChats => "List workspace chats with IDs for sending a message.",
            Action::SendMessage => {
                "Send a message to a workspace chat on the user's explicit request. Mentions record the intended member IDs in the message; they do not send email or push notifications."
            }
            Action::CreateReminder => {
                "Schedule a durable one-time reminder delivered to this chat. Require a future RFC3339 due_at with timezone offset. Clarify ambiguous dates or timezones; do not claim to run a task automatically."
            }
            Action::ListReminders => {
                "List the current user's workspace reminders, including pending, delivered, and cancelled reminders."
            }
            Action::CancelReminder => "Cancel one of the current user's pending reminders.",
            Action::StartTask => {
                "Create an agentic coding task and start the background runner immediately. Returns task_id and run_id. Does not wait for completion — poll get_task_run and tail_task_log. Use only when the user asked to run work in the background."
            }
            Action::GetTaskRun => {
                "Get status, phase, progress and error for a runner task in this workspace."
            }
            Action::TailTaskLog => {
                "Fetch new runner log lines since a previous log ID. Use to monitor start_task progress."
            }
        }
    }

    fn mutating(&self) -> bool {
        matches!(
            self.action,
            Action::CreateTask
                | Action::UpdateTask
                | Action::SendMessage
                | Action::CreateReminder
                | Action::CancelReminder
                | Action::StartTask
        )
    }
    fn parameters_schema(&self) -> Value {
        let identifier = json!({"type":"string","format":"uuid"});
        let (properties, required) = match self.action {
            Action::ListTasks => (
                json!({"status":{"type":"string","enum":["created","queued","in_progress","review","complete","blocked"]}}),
                json!([]),
            ),
            Action::CreateTask => (
                json!({"title":{"type":"string","minLength":1},"description":{"type":"string"},"assignee_id":{"type":["string","null"],"format":"uuid"}}),
                json!(["title"]),
            ),
            Action::UpdateTask => (
                json!({"task_id":identifier,"title":{"type":"string","minLength":1},"description":{"type":"string"},"status":{"type":"string","enum":["created","in_progress","review","complete","blocked"]},"assignee_id":{"type":["string","null"],"format":"uuid"}}),
                json!(["task_id"]),
            ),
            Action::SendMessage => (
                json!({"chat_id":identifier,"content":{"type":"string","minLength":1},"mentions":{"type":"array","items":identifier}}),
                json!(["chat_id", "content"]),
            ),
            Action::CreateReminder => (
                json!({"content":{"type":"string","minLength":1},"due_at":{"type":"string","format":"date-time","description":"RFC3339 with explicit timezone offset"}}),
                json!(["content", "due_at"]),
            ),
            Action::CancelReminder => (json!({"reminder_id":identifier}), json!(["reminder_id"])),
            Action::StartTask => (
                json!({
                    "title":{"type":"string","minLength":1},
                    "description":{"type":"string","minLength":1},
                    "acceptance_criteria":{"type":"string"},
                    "project_ids":{"type":"array","items":identifier},
                    "source_id":{"type":"string","format":"uuid"},
                    "priority":{"type":"integer","minimum":1,"maximum":5}
                }),
                json!(["title", "description"]),
            ),
            Action::GetTaskRun => (json!({"run_id":identifier}), json!(["run_id"])),
            Action::TailTaskLog => (
                json!({
                    "run_id":identifier,
                    "after_log_id":{"type":"string","format":"uuid"},
                    "limit":{"type":"integer","minimum":1,"maximum":200}
                }),
                json!(["run_id"]),
            ),
            _ => (json!({}), json!([])),
        };
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let result = self.run(params).await;
        Ok(match result {
            Ok(value) => ToolResult::success(value.to_string()),
            Err(sqlx::Error::Protocol(message)) => ToolResult::error(message),
            Err(error) => {
                tracing::warn!(%error, action = self.name(), "Workspace action failed");
                ToolResult::error("Workspace action failed; no success is confirmed.")
            }
        })
    }
}

impl WorkspaceAction {
    async fn run(&self, params: Value) -> Result<Value, sqlx::Error> {
        let scope = &self.scope;
        let pool = scope.state.db();
        match self.action {
            Action::ListTasks => {
                actions::list_tasks(
                    pool,
                    scope.workspace_id,
                    scope.user_id,
                    decode::<Filter>(params)?.status.as_deref(),
                )
                .await
            }
            Action::CreateTask => {
                actions::create_task(pool, scope.workspace_id, scope.user_id, decode(params)?).await
            }
            Action::UpdateTask => {
                actions::update_task(pool, scope.workspace_id, scope.user_id, decode(params)?).await
            }
            Action::ListMembers => {
                actions::list_members(pool, scope.workspace_id, scope.user_id).await
            }
            Action::ListChats => actions::list_chats(pool, scope.workspace_id, scope.user_id).await,
            Action::SendMessage => {
                actions::send_message(
                    pool,
                    scope.workspace_id,
                    scope.user_id,
                    scope.chat_id,
                    decode(params)?,
                )
                .await
            }
            Action::CreateReminder => {
                reminders::create(
                    pool,
                    scope.workspace_id,
                    scope.user_id,
                    scope.chat_id,
                    decode(params)?,
                )
                .await
            }
            Action::ListReminders => reminders::list(pool, scope.workspace_id, scope.user_id).await,
            Action::CancelReminder => {
                reminders::cancel(
                    pool,
                    scope.workspace_id,
                    scope.user_id,
                    decode::<Cancel>(params)?.reminder_id,
                )
                .await
            }
            Action::StartTask => {
                let started =
                    actions::start_task(pool, scope.workspace_id, scope.user_id, decode(params)?)
                        .await?;
                if let (Some(run_id), Some(task_id)) = (
                    started
                        .get("run_id")
                        .and_then(Value::as_str)
                        .and_then(|id| Uuid::parse_str(id).ok()),
                    started
                        .get("task_id")
                        .and_then(Value::as_str)
                        .and_then(|id| Uuid::parse_str(id).ok()),
                ) {
                    let state = scope.state.clone();
                    tokio::spawn(async move {
                        crate::workers::task::execute_task_run(&state, run_id, task_id).await;
                    });
                }
                Ok(started)
            }
            Action::GetTaskRun => {
                actions::get_task_run(
                    pool,
                    scope.workspace_id,
                    scope.user_id,
                    decode::<RunLookup>(params)?.run_id,
                )
                .await
            }
            Action::TailTaskLog => {
                actions::tail_task_log(pool, scope.workspace_id, scope.user_id, decode(params)?)
                    .await
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunLookup {
    run_id: Uuid,
}
