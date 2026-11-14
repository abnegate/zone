use abnegate_agent_cli::AgentKind;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

use super::Reason;

/// What a chat is told of a turn that moved to another login, live and in the assistant
/// message's metadata alike.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Notice {
    /// The label of the login the turn left.
    pub from: String,
    /// The label of the login the turn continues on.
    pub to: String,
    /// The agent the turn left.
    #[serde(serialize_with = "named")]
    pub from_agent: AgentKind,
    /// The agent the turn continues on.
    #[serde(serialize_with = "named")]
    pub agent: AgentKind,
    pub reason: Reason,
    /// When the limit the turn left on resets, when that is known.
    pub resets_at: Option<DateTime<Utc>>,
    /// Whether the agent's session file moved with the turn, so it resumed rather than replayed.
    pub carried: bool,
    /// How many Unicode scalar values of the answer were written before the switch.
    pub at: usize,
    /// How many tool calls the turn had made before the switch, which orders a
    /// call at the same offset on the right side of the divider.
    #[serde(default)]
    pub calls: usize,
    /// What the login being left thought after its last call, which belongs above the divider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

fn named<S: Serializer>(agent: &AgentKind, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(agent.as_str())
}
