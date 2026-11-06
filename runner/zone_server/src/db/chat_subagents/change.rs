use uuid::Uuid;
use zone_core::llm::SubagentStatus;

/// What one update says about a chat's subagent. Every `None` leaves the
/// stored value as it is, or the column's default when the row is new.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubagentChange<'a> {
    pub chat_id: Uuid,
    pub message_id: Option<Uuid>,
    pub external_id: &'a str,
    pub description: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub model: Option<&'a str>,
    pub background: Option<bool>,
    pub status: Option<SubagentStatus>,
    pub tool_uses: Option<i64>,
    pub duration_ms: Option<i64>,
    pub tokens: Option<i64>,
    pub last_tool: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub error: Option<&'a str>,
}
