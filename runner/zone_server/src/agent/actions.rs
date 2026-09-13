//! Authenticated workspace mutations and durable reminders.
use super::tools::WorkspaceScope;
use crate::db::{actions, reminders};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use zone_core::tools::{
    REASON_PARAM, Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult, excerpt,
    reason_property,
};

use super::PREVIEW_BODY_CHARS;

/// Named because the waiting section counts on them: a description that still
/// mandates a poll is read at the moment a runner starts, which is closer to
/// the decision than any prompt section gets.
pub(crate) const START_TASK_DESCRIPTION: &str = "Create an agentic coding task and start the background runner immediately. Returns task_id and run_id. Does not wait for completion — wait for it with wait_for, then read get_task_run and tail_task_log. Use only when the user asked to run work in the background.";

pub(crate) const TAIL_TASK_LOG_DESCRIPTION: &str = "Fetch new runner log lines since a previous log ID. Read a run's progress with it once; to find out when the run finishes, wait for it with wait_for rather than calling this again.";

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
        if scope.chat_id.is_none()
            && matches!(
                action,
                Action::SendMessage
                    | Action::CreateReminder
                    | Action::ListReminders
                    | Action::CancelReminder
                    | Action::StartTask
            )
        {
            continue;
        }
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
            Action::StartTask => START_TASK_DESCRIPTION,
            Action::GetTaskRun => {
                "Get status, phase, progress and error for a runner task in this workspace."
            }
            Action::TailTaskLog => TAIL_TASK_LOG_DESCRIPTION,
        }
    }

    fn tier(&self) -> Tier {
        match self.action {
            // Both speak to somebody: one now, one on a schedule the user is
            // not watching when it fires.
            Action::SendMessage | Action::CreateReminder => Tier::Outward,
            Action::CreateTask
            | Action::UpdateTask
            | Action::CancelReminder
            | Action::StartTask => Tier::Write,
            Action::ListTasks
            | Action::ListMembers
            | Action::ListChats
            | Action::ListReminders
            | Action::GetTaskRun
            | Action::TailTaskLog => Tier::Read,
        }
    }

    fn preview(&self, params: &Value) -> Option<String> {
        let content = excerpt(params["content"].as_str()?, PREVIEW_BODY_CHARS);
        match self.action {
            Action::SendMessage => {
                let mentioned = params["mentions"].as_array().map_or(0, Vec::len);
                let mentions = match mentioned {
                    0 => String::new(),
                    named => format!(", mentioning {named}"),
                };
                Some(format!(
                    "Post to chat {}{mentions}: \"{content}\"",
                    params["chat_id"].as_str().unwrap_or("unnamed")
                ))
            }
            Action::CreateReminder => Some(format!(
                "Message this chat at {}: \"{content}\"",
                params["due_at"].as_str().unwrap_or("an unstated time")
            )),
            _ => None,
        }
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
                json!({"chat_id":identifier,"content":{"type":"string","minLength":1},"mentions":{"type":"array","items":identifier},REASON_PARAM: reason_property()}),
                json!(["chat_id", "content", REASON_PARAM]),
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
                    scope
                        .chat_id
                        .ok_or_else(|| actions::invalid("This action requires a chat"))?,
                    decode(params)?,
                )
                .await
            }
            Action::CreateReminder => {
                reminders::create(
                    pool,
                    scope.workspace_id,
                    scope.user_id,
                    scope
                        .chat_id
                        .ok_or_else(|| actions::invalid("This action requires a chat"))?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use zone_core::tools::REASON_DESCRIPTION;

    const WHY: &str = "The user asked for the team to be told.";

    fn tool(action: Action) -> WorkspaceAction {
        WorkspaceAction {
            scope: WorkspaceScope {
                state: AppState::for_tests(),
                workspace_id: Uuid::new_v4(),
                chat_id: Some(Uuid::new_v4()),
                user_id: Uuid::new_v4(),
            },
            action,
        }
    }

    #[tokio::test]
    async fn send_message_asks_for_a_reason_the_message_accepts() {
        let schema = tool(Action::SendMessage).parameters_schema();
        assert_eq!(
            schema["properties"][REASON_PARAM]["description"],
            REASON_DESCRIPTION
        );
        assert!(
            schema["required"]
                .as_array()
                .expect("required is a list")
                .contains(&json!(REASON_PARAM)),
            "reason must be advertised as required: {schema}"
        );
        let message: actions::Message = serde_json::from_value(json!({
            "chat_id": Uuid::new_v4(),
            "content": "Shipped",
            "mentions": [],
            REASON_PARAM: WHY,
        }))
        .expect("deny_unknown_fields must accept every property the schema advertises");
        assert_eq!(message.reason.as_deref(), Some(WHY));
    }

    #[test]
    fn send_message_without_a_reason_still_decodes() {
        let message: actions::Message =
            serde_json::from_value(json!({"chat_id": Uuid::new_v4(), "content": "Shipped"}))
                .expect("a missing reason must never fail the call");
        assert!(message.reason.is_none(), "an absent reason stays absent");
    }

    #[tokio::test]
    async fn only_send_message_asks_for_a_reason() {
        for action in [
            Action::ListTasks,
            Action::CreateTask,
            Action::UpdateTask,
            Action::ListMembers,
            Action::ListChats,
            Action::CreateReminder,
            Action::ListReminders,
            Action::CancelReminder,
            Action::StartTask,
            Action::GetTaskRun,
            Action::TailTaskLog,
        ] {
            let tool = tool(action);
            let schema = tool.parameters_schema();
            assert!(
                schema["properties"].get(REASON_PARAM).is_none(),
                "{} must not ask for a reason: {schema}",
                tool.name()
            );
        }
    }
}
