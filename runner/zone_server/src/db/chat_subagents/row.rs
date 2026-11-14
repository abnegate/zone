use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SubagentRow {
    pub id: Uuid,
    pub chat_id: Uuid,
    /// The turn that started it, which is also its assistant message's id.
    pub message_id: Option<Uuid>,
    /// The agent's own name for it: Claude's Agent call id, or codex's thread id.
    pub external_id: String,
    pub description: String,
    pub kind: Option<String>,
    pub model: Option<String>,
    pub background: bool,
    /// One of [`abnegate_agent_cli::SubagentStatus`]'s names; the table's check
    /// refuses any other.
    pub status: String,
    pub tool_uses: Option<i64>,
    pub duration_ms: Option<i64>,
    pub tokens: Option<i64>,
    pub last_tool: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub step_count: i64,
    /// Counts the row's writes, so a reader holding two copies keeps the later.
    pub revision: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}
