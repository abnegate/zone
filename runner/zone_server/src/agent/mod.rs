//! Agentic chat: letting a conversation call workspace tools before answering.
//!
//! Plain chat sends the conversation to the model and streams back whatever it
//! says. An agentic chat instead offers the model tools and runs a reason/act
//! loop until it has an answer. The tool trace is streamed to the client as it
//! happens and stored on the assistant message so reloading the conversation
//! still shows the work.
//!
//! Agent chats always offer workspace tools and server filesystem and shell tools.
//! Docker deployments execute these tools inside the server container.

pub mod actions;
pub mod approval;
pub mod audio;
pub mod citations;
pub mod documents;
pub mod identifier;
pub mod images;
pub mod integrations;
pub mod monitoring;
pub mod prompt;
pub mod readiness;
pub mod receipts;
pub mod releases;
pub mod runner;
pub mod tools;
pub mod verification;
pub mod web;

pub use approval::{ApprovalGate, ApprovalPolicy};
pub use citations::{Citation, CitationKind, CitationOutcome};
pub use prompt::{Environment, Surface, Vcs, Verbosity};
pub use receipts::{ActionReceipt, ActionTarget};
pub use runner::{
    AgentEvent, AgentRun, LoopBudget, MAX_ITERATIONS, MAX_TOOL_CALLS, run, run_with_context,
};
pub use tools::{ChatTools, ToolProfile, WorkspaceScope};

use serde::{Deserialize, Serialize};
use zone_core::tools::REASON_PARAM;

/// How much of a title an approval preview quotes.
pub(crate) const PREVIEW_TITLE_CHARS: usize = 80;

/// How much of a body an approval preview quotes: enough to recognise the
/// message being sent without reprinting it.
pub(crate) const PREVIEW_BODY_CHARS: usize = 200;

/// The model's stated reason for a call, read out of the call's own arguments.
///
/// This is the model's prose, not an observation, so every consumer of it has
/// to present it as a claim. Absent, blank, or non-string reasons all read as
/// no reason at all, which the console says out loud rather than leaving blank.
pub fn reason(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments.trim())
        .ok()?
        .get(REASON_PARAM)?
        .as_str()
        .map(str::trim)
        .filter(|stated| !stated.is_empty())
        .map(str::to_string)
}

/// A completed tool call, as streamed to the client and stored on the message.
///
/// This is the wire and storage shape both: the console renders it live from
/// the websocket and again from `messages.metadata` after a reload, so the two
/// have to agree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub success: bool,
    pub detail: String,
    pub duration_ms: u64,
    /// Model thinking that immediately preceded this call, shown in the trace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Why the model said it was making this call, for side-effecting tools.
    /// Model-authored: the console labels it as stated, never as observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What the call will do, rendered by the server from its arguments while
    /// it waits to be allowed. Kept on the record so a reader who reloads
    /// mid-decision is still deciding on the action rather than on raw JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_record_round_trips_through_metadata() {
        let record = ToolCallRecord {
            id: "call_1".to_string(),
            name: "search_knowledge".to_string(),
            arguments: r#"{"query":"deploys"}"#.to_string(),
            success: true,
            detail: "3 passages".to_string(),
            duration_ms: 42,
            reasoning: Some("Search workspace docs first.".into()),
            reason: Some("The user asked what changed in the deploy.".into()),
            preview: None,
        };

        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["name"], "search_knowledge");
        assert_eq!(json["success"], true);
        assert_eq!(json["reasoning"], "Search workspace docs first.");
        assert_eq!(json["reason"], "The user asked what changed in the deploy.");

        let parsed: ToolCallRecord = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, record);
    }

    #[test]
    fn tool_call_record_stored_without_a_reason_still_parses() {
        let stored = serde_json::json!({
            "id": "call_1",
            "name": "run_shell",
            "arguments": r#"{"command":"ls"}"#,
            "success": true,
            "detail": "ok",
            "duration_ms": 7
        });

        let parsed: ToolCallRecord = serde_json::from_value(stored).unwrap();
        assert_eq!(parsed.reason, None);
        assert_eq!(parsed.reasoning, None);
        assert!(
            serde_json::to_value(&parsed)
                .unwrap()
                .get("reason")
                .is_none(),
            "an absent reason must not be written back as null"
        );
    }

    #[test]
    fn a_stated_reason_is_read_from_the_call_arguments() {
        assert_eq!(
            reason(r#"{"command":"ls","reason":"List the checkout before patching."}"#),
            Some("List the checkout before patching.".to_string())
        );
        assert_eq!(
            reason(r#"  {"reason":"  Trimmed to the model's own words.  "}  "#),
            Some("Trimmed to the model's own words.".to_string())
        );
    }

    #[test]
    fn a_missing_blank_or_unusable_reason_reads_as_none() {
        for arguments in [
            r#"{"command":"ls"}"#,
            r#"{"reason":""}"#,
            r#"{"reason":"   "}"#,
            r#"{"reason":42}"#,
            r#"{"reason":null}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(reason(arguments), None, "{arguments}");
        }
    }

    #[test]
    fn action_receipt_round_trips_through_metadata() {
        let receipt = ActionReceipt {
            id: "call_1".to_string(),
            action: "create_task".to_string(),
            target_type: ActionTarget::Task,
            target_id: "task-1".to_string(),
            target_label: "Ship the billing export".to_string(),
            actor_id: "user-1".to_string(),
            actor_name: "Alice".to_string(),
            occurred_at: "2026-09-05T10:47:00.000Z".to_string(),
            success: true,
            outcome: "Task created".to_string(),
            href: "/tasks?id=task-1".to_string(),
            reason: Some("The user asked me to track the export.".into()),
        };

        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["action"], "create_task");
        assert_eq!(json["target_type"], "task");
        assert_eq!(json["href"], "/tasks?id=task-1");
        assert_eq!(json["reason"], "The user asked me to track the export.");

        let parsed: ActionReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, receipt);
    }
}
