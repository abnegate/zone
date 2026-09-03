//! Agentic chat: letting a conversation call workspace tools before answering.
//!
//! Plain chat sends the conversation to the model and streams back whatever it
//! says. An agentic chat additionally offers the model a set of read-only tools
//! scoped to the chat's workspace, and runs a reason/act loop until it has an
//! answer. The tool trace is streamed to the client as it happens and stored on
//! the assistant message so reloading the conversation still shows the work.

pub mod runner;
pub mod tools;

pub use runner::{AgentEvent, AgentRun, MAX_ITERATIONS, MAX_TOOL_CALLS, run};
pub use tools::{ChatTool, ChatToolRegistry, ToolContext};

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
}

/// Instructions prepended to an agentic turn.
///
/// The tool schemas already tell the model what it *can* call; this covers when
/// it *should*, which is the part local models get wrong most often.
pub fn system_prompt(registry: &ChatToolRegistry) -> String {
    format!(
        "You are Zone's assistant, answering inside one of the user's workspaces.\n\n\
         You can call these tools to look things up: {}.\n\n\
         How to work:\n\
         - Anything about this workspace's own documents, sources, projects or tasks must come \
         from a tool call, never from memory. Search first, answer second.\n\
         - General knowledge questions need no tools; answer them directly.\n\
         - Prefer one well-phrased search over several near-identical ones, and stop searching \
         once you can answer.\n\
         - Name the documents you drew on so the user can check them.\n\
         - If the tools return nothing useful, say so plainly instead of guessing. A wrong answer \
         about the user's own data is worse than an admission that you could not find it.",
        registry.names().join(", ")
    )
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
        };

        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["name"], "search_knowledge");
        assert_eq!(json["success"], true);

        let parsed: ToolCallRecord = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, record);
    }
}
