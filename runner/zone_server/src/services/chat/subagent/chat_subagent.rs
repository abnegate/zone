use abnegate_agent_cli::SubagentStatus;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::chat_subagents::SubagentRow;

/// One of a chat's subagents as its readers see it, over REST and the chat's
/// socket alike. Every unknown field is sent as `null` rather than left out,
/// so a reader can tell a cleared value from one it was never sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatSubagent {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub message_id: Option<Uuid>,
    pub external_id: String,
    pub description: String,
    pub kind: Option<String>,
    pub model: Option<String>,
    pub background: bool,
    pub status: SubagentStatus,
    pub tool_uses: Option<i64>,
    pub duration_ms: Option<i64>,
    pub tokens: Option<i64>,
    pub last_tool: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
    /// How many steps it has taken; the steps themselves are read on their own.
    pub step_count: i64,
    /// Counts the row's writes: of two copies, the higher is the later.
    pub revision: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl From<SubagentRow> for ChatSubagent {
    fn from(row: SubagentRow) -> Self {
        Self {
            id: row.id,
            chat_id: row.chat_id,
            message_id: row.message_id,
            external_id: row.external_id,
            description: row.description,
            kind: row.kind,
            model: row.model,
            background: row.background,
            status: SubagentStatus::named(&row.status).unwrap_or(SubagentStatus::Stopped),
            tool_uses: row.tool_uses,
            duration_ms: row.duration_ms,
            tokens: row.tokens,
            last_tool: row.last_tool,
            summary: row.summary,
            error: row.error,
            step_count: row.step_count,
            revision: row.revision,
            started_at: row.started_at,
            finished_at: row.finished_at,
            updated_at: row.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row() -> SubagentRow {
        let started = DateTime::parse_from_rfc3339("2026-11-06T09:00:00Z")
            .expect("a time")
            .with_timezone(&Utc);
        SubagentRow {
            id: Uuid::nil(),
            chat_id: Uuid::max(),
            message_id: None,
            external_id: "toolu_01Agent".to_string(),
            description: "Ask for a review".to_string(),
            kind: Some("general-purpose".to_string()),
            model: None,
            background: true,
            status: SubagentStatus::Running.as_str().to_string(),
            tool_uses: Some(12),
            duration_ms: None,
            tokens: None,
            last_tool: None,
            summary: None,
            error: None,
            step_count: 3,
            revision: 7,
            started_at: started,
            finished_at: None,
            updated_at: started,
        }
    }

    #[test]
    fn a_subagent_is_sent_with_every_field_and_unknowns_as_null() {
        let sent = serde_json::to_value(ChatSubagent::from(row())).expect("a subagent");

        assert_eq!(
            sent,
            json!({
                "id": Uuid::nil(),
                "chat_id": Uuid::max(),
                "message_id": null,
                "external_id": "toolu_01Agent",
                "description": "Ask for a review",
                "kind": "general-purpose",
                "model": null,
                "background": true,
                "status": "running",
                "tool_uses": 12,
                "duration_ms": null,
                "tokens": null,
                "last_tool": null,
                "summary": null,
                "error": null,
                "step_count": 3,
                "revision": 7,
                "started_at": "2026-11-06T09:00:00Z",
                "finished_at": null,
                "updated_at": "2026-11-06T09:00:00Z",
            })
        );
    }

    #[test]
    fn every_stored_status_is_sent_as_itself() {
        for &status in SubagentStatus::ALL {
            let stored = SubagentRow {
                status: status.as_str().to_string(),
                ..row()
            };
            assert_eq!(ChatSubagent::from(stored).status, status);
        }
    }
}
