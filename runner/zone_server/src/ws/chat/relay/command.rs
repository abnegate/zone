use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A client's change to a chat's turn or queue, already validated by the
/// replica its socket is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Send {
        id: Uuid,
        content: String,
        metadata: Option<serde_json::Value>,
    },
    Cancel,
    Interrupt {
        id: Uuid,
    },
    EditQueued {
        id: Uuid,
        content: String,
    },
    RemoveQueued {
        id: Uuid,
    },
    ApproveTool {
        tool_call_id: String,
        approved: bool,
    },
}
