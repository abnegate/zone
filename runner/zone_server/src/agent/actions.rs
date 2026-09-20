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

/// What `create_reminder` offers and what it refuses, in the tool's own words.
///
/// Long because the schema alone cannot say the four things a caller gets
/// wrong: that a rule outside the subset is refused rather than quietly
/// dropped, that hourly is a ceiling and a faster condition wants an event
/// rather than a schedule, that a prompt turns content into the schedule's
/// name rather than its message, and that a repeat is something to offer
/// rather than something to impose on a request that was made once.
const CREATE_REMINDER_DESCRIPTION: &str = "Schedule a durable reminder delivered to this chat. \
     due_at is the first firing: a future RFC3339 time with an explicit timezone offset, and the \
     exact time the person named. Clarify an ambiguous date or timezone rather than guessing, and \
     do not claim this will run a task. \
     Add rrule to repeat it, as an RFC 5545 rule in this subset: FREQ (HOURLY, DAILY, WEEKLY, \
     MONTHLY), INTERVAL, BYDAY, BYHOUR, BYMINUTE, BYMONTHDAY, UNTIL, COUNT. A clause outside that \
     list is refused rather than dropped. FREQ=HOURLY is the finest cadence there is; MINUTELY \
     and SECONDLY are refused, not rounded up. Once an hour is the ceiling, measured at the \
     shortest gap the rule produces rather than its average — BYHOUR=0,1 with BYMINUTE=0,30 \
     fires four times a day and three of those gaps are half an hour. A condition that changes \
     faster than the ceiling wants wait_for on the event itself, not a schedule. A repeating \
     reminder stops after seven days unless it is asked for again. \
     Without a prompt, each firing delivers content as it is written. With one, each firing runs \
     the prompt as a turn of your own in this chat and what you say is the delivery, and content \
     is not sent at all — it stays as the schedule's name, which is what list_reminders shows and \
     what the person reads when deciding whether to cancel it, so make it a short description of \
     the standing job rather than a message. Use a prompt when the useful answer has to be worked \
     out at the time, and content alone when it is the same words every time. A prompt is an \
     instruction to your future self, which will have this chat and these tools and no memory of \
     writing it, so say what to check and what to report. \
     End it with the rule that if nothing changed, it should say nothing: a schedule that reports \
     every firing whether or not anything happened teaches the person to ignore it. \
     Set timing_mode to condition_watch when the change is the point rather than the time. Each \
     firing is handed what the last one answered and asked what differs, so the prompt only has \
     to say what to look at — the comparison is supplied, and so is the rule for an unchanged \
     firing, which for a watch is one short line rather than silence. \
     A watch needs both an rrule and a prompt and is refused without them: one firing has nothing \
     to compare against, and fixed content has nothing to compare. Two limits to state when you \
     offer one. It sees only the state at each firing, so a condition that appears and disappears \
     between two firings is never noticed — for something that raises an event of its own, use \
     wait_for on the event rather than a watch. And a watch cannot stay silent: running the turn \
     is how it reports at all, so an unchanged firing still answers here, in one short line. That \
     is the one place a watch departs from the say-nothing rule above, so a watch's prompt should \
     not repeat that rule. \
     Offer a repeat when somebody plainly wants the same thing again; never turn a request made \
     once into a standing one they did not ask for.";

/// Named because the waiting section counts on them: a description that still
/// mandates a poll is read at the moment a runner starts, which is closer to
/// the decision than any prompt section gets.
pub(crate) const START_TASK_DESCRIPTION: &str = "Create an agentic coding task and start the background runner immediately. Returns task_id and run_id. Does not wait for completion — wait for it with wait_for, then read get_task_run and tail_task_log. Use only when the user asked to run work in the background.";

/// The two surfaces this description is read from. `wait_for` takes
/// kind=task_run from a chat and refuses it from inside a run, so the guidance
/// names which half is which rather than sending both to the same call: a run
/// that follows an unscoped offer learns it was wrong from the refusal.
pub(crate) const FROM_CHAT: &str = "from a chat";
pub(crate) const FROM_RUN: &str = "from inside a run";

pub(crate) const TAIL_TASK_LOG_DESCRIPTION: &str = "Fetch new runner log lines since a previous log ID. Read a run's progress with it once rather than calling it again: from a chat, find out when the run finishes by waiting for it with wait_for kind=task_run; from inside a run, finish and let whoever started it coordinate.";

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
            Action::CreateReminder => CREATE_REMINDER_DESCRIPTION,
            Action::ListReminders => {
                "List the current user's workspace reminders: pending, delivered, cancelled, and \
                 expired — a repeating one that ran out of rule or outlived its week, which is a \
                 different ending from one somebody cancelled. A pending row with an rrule is \
                 still repeating, and its due_at is the next firing rather than the first."
            }
            Action::CancelReminder => {
                "Cancel one of the current user's pending reminders. Cancelling a repeating one \
                 stops the whole schedule, not just its next firing."
            }
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
                json!({"task_id":identifier,"title":{"type":"string","minLength":1},"description":{"type":"string"},"status":{"type":"string","enum":["created","in_progress","review","complete","blocked"]},"priority":{"type":"integer","minimum":1,"maximum":5,"description":"1 is the most urgent, 5 the least."},"assignee_id":{"type":["string","null"],"format":"uuid"}}),
                json!(["task_id"]),
            ),
            Action::SendMessage => (
                json!({"chat_id":identifier,"content":{"type":"string","minLength":1},"mentions":{"type":"array","items":identifier},REASON_PARAM: reason_property()}),
                json!(["chat_id", "content", REASON_PARAM]),
            ),
            Action::CreateReminder => (
                json!({
                    "content":{"type":"string","minLength":1,"description":"The words each firing delivers. With a prompt they are not delivered at all and this is the schedule's name instead, so keep it short enough to recognise in a list."},
                    "due_at":{"type":"string","format":"date-time","description":"RFC3339 with explicit timezone offset, e.g. 2026-09-20T19:30:00+12:00. The first firing, and the exact time the person named. It must still be in the future when the call is made: a turn can take minutes, so leave room rather than naming the next minute."},
                    "rrule":{"type":"string","description":"RFC 5545 rule to repeat it, e.g. FREQ=WEEKLY;BYDAY=MO;BYHOUR=9. FREQ may be HOURLY, DAILY, WEEKLY or MONTHLY; FREQ=HOURLY is the finest cadence and MINUTELY or SECONDLY is refused. Omit for a single reminder."},
                    "prompt":{"type":"string","description":"An instruction to your future self, run as a turn at each firing instead of delivering content. End it with the rule that if nothing changed, say nothing — except under condition_watch, which supplies its own rule and answers an unchanged firing in one short line."},
                    "timing_mode":{"type":"string","enum":["exact_schedule","condition_watch"],"description":"exact_schedule, the default, fires at the time named. condition_watch fires on the rrule and reports what differs from the last firing, and requires both rrule and prompt. It cannot see a change that appears and disappears between two firings."}
                }),
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

    #[tokio::test]
    async fn update_task_offers_the_priority_a_task_has() {
        let schema = tool(Action::UpdateTask).parameters_schema();
        let priority = &schema["properties"]["priority"];
        assert_eq!(priority["type"], "integer");
        assert_eq!(priority["minimum"], 1);
        assert_eq!(priority["maximum"], 5);
        let update: actions::Update =
            serde_json::from_value(json!({"task_id": Uuid::new_v4(), "priority": 2}))
                .expect("deny_unknown_fields must accept every property the schema advertises");
        assert_eq!(update.priority, Some(2));
    }

    #[tokio::test]
    async fn create_reminder_states_the_hourly_floor_where_the_rule_is_asked_for() {
        let schema = tool(Action::CreateReminder).parameters_schema();
        let rrule = schema["properties"]["rrule"]["description"]
            .as_str()
            .expect("rrule is described");
        assert!(rrule.contains("HOURLY"), "{rrule}");
        assert!(rrule.contains("MINUTELY"), "{rrule}");
        assert!(
            CREATE_REMINDER_DESCRIPTION.contains("MINUTELY"),
            "the tool description names what is refused"
        );
        let due_at = schema["properties"]["due_at"]["description"]
            .as_str()
            .expect("due_at is described");
        assert!(
            due_at.contains("+12:00"),
            "an example offset is shown: {due_at}"
        );
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
