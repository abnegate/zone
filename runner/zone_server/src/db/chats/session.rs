use uuid::Uuid;
use zone_core::llm::AgentKind;

/// The coding agent CLI session a chat's turns resume, and the login they run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSession {
    /// The login the chat's turns run on, until it is signed out.
    pub login: Option<Uuid>,
    /// The id the CLI knows the session by.
    pub id: String,
    pub agent: AgentKind,
    /// The last `chat_entries.position` the session has seen.
    pub entry: i64,
    /// The hex SHA-256 of the stable system prompt the session last saw.
    pub prompt: Option<String>,
}
