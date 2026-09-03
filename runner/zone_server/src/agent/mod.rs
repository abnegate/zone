//! Agentic chat: letting a conversation call workspace tools before answering.
//!
//! Plain chat sends the conversation to the model and streams back whatever it
//! says. An agentic chat instead offers the model tools and runs a reason/act
//! loop until it has an answer. The tool trace is streamed to the client as it
//! happens and stored on the assistant message so reloading the conversation
//! still shows the work.
//!
//! A chat picks one of two tool sets. Sandboxed, the default, is read-only and
//! scoped to the chat's workspace. Unsandboxed adds [`host`]: a real shell and
//! real file access, with no allow-list and no path containment.

pub mod runner;
pub mod tools;

pub use runner::{AgentEvent, AgentRun, MAX_ITERATIONS, MAX_TOOL_CALLS, run};
pub use tools::{ChatTools, WorkspaceScope, host_tools_allowed};

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
pub fn system_prompt(tools: &ChatTools) -> String {
    let mut prompt = format!(
        "You are Zone's assistant, answering inside one of the user's workspaces.\n\n\
         You can call these tools: {}.\n\n\
         How to work:\n\
         - Anything about this workspace's own documents, sources, projects or tasks must come \
         from a tool call, never from memory. Search first, answer second.\n\
         - General knowledge questions need no tools; answer them directly.\n\
         - Prefer one well-phrased search over several near-identical ones, and stop searching \
         once you can answer.\n\
         - Name the documents you drew on so the user can check them.\n\
         - If the tools return nothing useful, say so plainly instead of guessing. A wrong answer \
         about the user's own data is worse than an admission that you could not find it.",
        tools.names().join(", ")
    );

    if !tools.is_sandboxed() {
        // The user turned the sandbox off deliberately, so this is about
        // working carefully rather than refusing. The model cannot see the
        // consequences of a command, and the person reading only sees the
        // trace afterwards.
        prompt.push_str(
            "\n\nThis chat is running without a sandbox. run_shell, read_file, write_file and \
             list_directory act on the real machine this server runs on, as the user that runs \
             it, and nothing you do through them is undone when the turn ends.\n\
             - Look before you change: read a file before rewriting it, and check what a \
             directory holds before writing into it.\n\
             - Keep each command narrow and inspectable, and prefer a dry run where one exists.\n\
             - Do not delete, move or overwrite anything the user did not ask you to, and do not \
             touch anything outside what the request is about.\n\
             - Say what you changed on disk in your reply. The user sees the tool trace, but the \
             consequences are yours to explain.",
        );
    }

    prompt
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
