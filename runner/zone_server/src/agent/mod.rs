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
        };

        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["name"], "search_knowledge");
        assert_eq!(json["success"], true);
        assert_eq!(json["reasoning"], "Search workspace docs first.");

        let parsed: ToolCallRecord = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, record);
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
        };

        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["action"], "create_task");
        assert_eq!(json["target_type"], "task");
        assert_eq!(json["href"], "/tasks?id=task-1");

        let parsed: ActionReceipt = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, receipt);
    }
}
