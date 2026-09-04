//! Durable receipts for sandboxed workspace writes.
//!
//! Read tools stay in the quiet tool trace. When the agent creates or changes a
//! task, document, message, or reminder, the console also needs a first-class
//! record: what happened, to which item, by whom, when, and whether it stuck.
//! That record is built here from the tool name, arguments, and result — never
//! from model prose — then stored on the assistant message.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zone_core::tools::ToolResult;

/// Longest target label we keep on a receipt.
const LABEL_CHARS: usize = 80;

/// Longest outcome line we keep on a receipt.
const OUTCOME_CHARS: usize = 240;

/// Workspace item a write tool can change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionTarget {
    Task,
    Document,
    Message,
    Reminder,
}

/// A completed workspace write, as streamed to the client and stored on the
/// assistant message. Field names are the console contract: the live
/// websocket frame and `messages.metadata.action_receipts` must agree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionReceipt {
    pub id: String,
    pub action: String,
    pub target_type: ActionTarget,
    pub target_id: String,
    pub target_label: String,
    pub actor_id: String,
    pub actor_name: String,
    pub occurred_at: String,
    pub success: bool,
    pub outcome: String,
    pub href: String,
}

/// Tools that mutate a workspace item and should mint a receipt.
pub fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "create_task"
            | "update_task"
            | "create_document"
            | "update_document"
            | "send_message"
            | "create_reminder"
            | "cancel_reminder"
    )
}

/// Build a receipt from a finished write, or `None` for any other tool.
pub fn from_write(
    id: &str,
    name: &str,
    arguments: &str,
    result: &ToolResult,
    actor_id: Uuid,
    actor_name: &str,
    occurred_at: DateTime<Utc>,
) -> Option<ActionReceipt> {
    let target_type = target_type(name)?;
    let args = parse_object(arguments);
    let output = result
        .output
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());

    let target_id = target_id(name, &args, output.as_ref());
    let chat_id = first_text(&[
        output
            .as_ref()
            .and_then(|value| text_field(value, "chat_id")),
        text_field(&args, "chat_id"),
    ]);
    let target_label = clip(&target_label(name, &args, output.as_ref()), LABEL_CHARS);
    let outcome = clip(&outcome(name, result), OUTCOME_CHARS);
    let href = href(target_type, &target_id, &chat_id);

    Some(ActionReceipt {
        id: id.to_string(),
        action: name.to_string(),
        target_type,
        target_id,
        target_label,
        actor_id: actor_id.to_string(),
        actor_name: actor_name.trim().to_string(),
        occurred_at: occurred_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        success: result.success,
        outcome,
        href,
    })
}

fn target_type(name: &str) -> Option<ActionTarget> {
    Some(match name {
        "create_task" | "update_task" => ActionTarget::Task,
        "create_document" | "update_document" => ActionTarget::Document,
        "send_message" => ActionTarget::Message,
        "create_reminder" | "cancel_reminder" => ActionTarget::Reminder,
        _ => return None,
    })
}

fn target_id(name: &str, args: &Value, output: Option<&Value>) -> String {
    let from_output = output.and_then(|value| text_field(value, "id"));
    let from_args = match name {
        "update_task" => text_field(args, "task_id"),
        "update_document" => text_field(args, "id"),
        "cancel_reminder" => text_field(args, "reminder_id"),
        _ => None,
    };
    first_text(&[from_output, from_args]).unwrap_or_default()
}

fn target_label(name: &str, args: &Value, output: Option<&Value>) -> String {
    let title = first_text(&[
        output.and_then(|value| text_field(value, "title")),
        text_field(args, "title"),
    ]);
    let content = first_text(&[
        output.and_then(|value| text_field(value, "content")),
        text_field(args, "content"),
    ]);
    match name {
        "create_task" | "update_task" | "create_document" | "update_document" => title
            .or(content)
            .unwrap_or_else(|| fallback_label(name).to_string()),
        "send_message" | "create_reminder" | "cancel_reminder" => content
            .or(title)
            .unwrap_or_else(|| fallback_label(name).to_string()),
        _ => fallback_label(name).to_string(),
    }
}

fn fallback_label(name: &str) -> &'static str {
    match name {
        "create_task" | "update_task" => "Task",
        "create_document" | "update_document" => "Document",
        "send_message" => "Message",
        "create_reminder" | "cancel_reminder" => "Reminder",
        _ => "Workspace item",
    }
}

fn outcome(name: &str, result: &ToolResult) -> String {
    if !result.success {
        return result
            .error
            .as_deref()
            .or(result.output.as_deref())
            .unwrap_or("The write did not complete.")
            .lines()
            .next()
            .unwrap_or("The write did not complete.")
            .trim()
            .to_string();
    }
    match name {
        "create_task" => "Task created".to_string(),
        "update_task" => "Task updated".to_string(),
        "create_document" => "Document created".to_string(),
        "update_document" => "Document updated".to_string(),
        "send_message" => "Message sent".to_string(),
        "create_reminder" => "Reminder scheduled".to_string(),
        "cancel_reminder" => "Reminder cancelled".to_string(),
        _ => "Write completed".to_string(),
    }
}

fn href(target_type: ActionTarget, target_id: &str, chat_id: &Option<String>) -> String {
    match target_type {
        ActionTarget::Task if !target_id.is_empty() => format!("/tasks?id={target_id}"),
        ActionTarget::Document if !target_id.is_empty() => format!("/wiki?id={target_id}"),
        ActionTarget::Message => match (chat_id.as_deref(), target_id) {
            (Some(chat), id) if !id.is_empty() => format!("/chats?id={chat}&message={id}"),
            (Some(chat), _) => format!("/chats?id={chat}"),
            _ => String::new(),
        },
        ActionTarget::Reminder => match chat_id.as_deref() {
            Some(chat) if !chat.is_empty() => format!("/chats?id={chat}"),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn parse_object(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Object(serde_json::Map::new());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Null) => Value::Object(serde_json::Map::new()),
        Ok(value) => value,
        Err(_) => Value::Object(serde_json::Map::new()),
    }
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn first_text(candidates: &[Option<String>]) -> Option<String> {
    candidates
        .iter()
        .flatten()
        .find(|text| !text.is_empty())
        .cloned()
}

fn clip(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}…", &text[..byte_idx]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn actor() -> Uuid {
        Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
    }

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-05T10:47:00.000Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn receipt(name: &str, arguments: &str, result: ToolResult) -> ActionReceipt {
        from_write("call_1", name, arguments, &result, actor(), "Alice", at()).unwrap()
    }

    #[test]
    fn read_tools_do_not_mint_receipts() {
        assert!(
            from_write(
                "call_1",
                "list_tasks",
                "{}",
                &ToolResult::success("[]"),
                actor(),
                "Alice",
                at()
            )
            .is_none()
        );
        assert!(!is_write_tool("list_tasks"));
        assert!(!is_write_tool("write_file"));
        assert!(is_write_tool("create_task"));
    }

    #[test]
    fn create_task_receipt_links_the_new_task() {
        let built = receipt(
            "create_task",
            r#"{"title":"Ship the billing export"}"#,
            ToolResult::success(
                json!({"id":"22222222-2222-2222-2222-222222222222","title":"Ship the billing export"})
                    .to_string(),
            ),
        );

        assert_eq!(built.action, "create_task");
        assert_eq!(built.target_type, ActionTarget::Task);
        assert_eq!(built.target_id, "22222222-2222-2222-2222-222222222222");
        assert_eq!(built.target_label, "Ship the billing export");
        assert_eq!(built.actor_name, "Alice");
        assert_eq!(built.occurred_at, "2026-09-05T10:47:00.000Z");
        assert!(built.success);
        assert_eq!(built.outcome, "Task created");
        assert_eq!(built.href, "/tasks?id=22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn update_task_uses_the_argument_id_when_output_is_missing() {
        let built = receipt(
            "update_task",
            r#"{"task_id":"22222222-2222-2222-2222-222222222222","status":"complete"}"#,
            ToolResult::success("ok"),
        );
        assert_eq!(built.target_id, "22222222-2222-2222-2222-222222222222");
        assert_eq!(built.outcome, "Task updated");
        assert_eq!(built.href, "/tasks?id=22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn document_receipts_take_title_from_arguments() {
        let created = receipt(
            "create_document",
            r#"{"title":"Runbook","content":"Restart the worker"}"#,
            ToolResult::success(json!({"id":"doc-1","created":true}).to_string()),
        );
        assert_eq!(created.target_type, ActionTarget::Document);
        assert_eq!(created.target_label, "Runbook");
        assert_eq!(created.href, "/wiki?id=doc-1");

        let updated = receipt(
            "update_document",
            r#"{"id":"doc-1","title":"Runbook v2"}"#,
            ToolResult::success(json!({"id":"doc-1","updated":true}).to_string()),
        );
        assert_eq!(updated.outcome, "Document updated");
        assert_eq!(updated.href, "/wiki?id=doc-1");
    }

    #[test]
    fn send_message_links_the_destination_chat() {
        let built = receipt(
            "send_message",
            r#"{"chat_id":"chat-9","content":"Standup is at ten"}"#,
            ToolResult::success(
                json!({"id":"msg-3","chat_id":"chat-9","content":"Standup is at ten"}).to_string(),
            ),
        );
        assert_eq!(built.target_type, ActionTarget::Message);
        assert_eq!(built.target_id, "msg-3");
        assert_eq!(built.target_label, "Standup is at ten");
        assert_eq!(built.href, "/chats?id=chat-9&message=msg-3");
    }

    #[test]
    fn reminder_receipts_link_the_destination_chat() {
        let created = receipt(
            "create_reminder",
            r#"{"content":"Follow up Friday","due_at":"2026-09-06T10:00:00Z"}"#,
            ToolResult::success(
                json!({"id":"rem-1","chat_id":"chat-9","content":"Follow up Friday"}).to_string(),
            ),
        );
        assert_eq!(created.target_type, ActionTarget::Reminder);
        assert_eq!(created.href, "/chats?id=chat-9");
        assert_eq!(created.outcome, "Reminder scheduled");

        let cancelled = receipt(
            "cancel_reminder",
            r#"{"reminder_id":"rem-1"}"#,
            ToolResult::success(
                json!({"id":"rem-1","chat_id":"chat-9","status":"cancelled"}).to_string(),
            ),
        );
        assert_eq!(cancelled.target_id, "rem-1");
        assert_eq!(cancelled.outcome, "Reminder cancelled");
    }

    #[test]
    fn failed_create_keeps_the_label_and_omits_the_link() {
        let built = receipt(
            "create_task",
            r#"{"title":"Ship the billing export"}"#,
            ToolResult::error("Workspace access denied"),
        );
        assert!(!built.success);
        assert_eq!(built.target_id, "");
        assert_eq!(built.target_label, "Ship the billing export");
        assert_eq!(built.outcome, "Workspace access denied");
        assert!(built.href.is_empty());
    }

    #[test]
    fn failed_update_still_links_the_known_target() {
        let built = receipt(
            "update_task",
            r#"{"task_id":"22222222-2222-2222-2222-222222222222","title":"Renamed"}"#,
            ToolResult::error("Task not found or is managed by the task runner"),
        );
        assert!(!built.success);
        assert_eq!(built.href, "/tasks?id=22222222-2222-2222-2222-222222222222");
        assert_eq!(built.target_label, "Renamed");
    }

    #[test]
    fn receipt_round_trips_through_metadata() {
        let built = receipt(
            "create_task",
            r#"{"title":"Ship"}"#,
            ToolResult::success(json!({"id":"task-1","title":"Ship"}).to_string()),
        );
        let json = serde_json::to_value(&built).unwrap();
        assert_eq!(json["target_type"], "task");
        assert_eq!(json["actor_id"], actor().to_string());
        let parsed: ActionReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, built);
    }
}
