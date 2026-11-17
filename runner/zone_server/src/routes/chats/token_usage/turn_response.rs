use serde::Serialize;

use crate::db::chat_token_usage::TurnRow;

/// One model's share of one assistant turn. A turn handed over to another
/// model lists one share per model under the same `turn_id`, and only the
/// model it ended on carries the context breakdown. The billed counts are
/// null, not zero, when the provider reported nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnResponse {
    pub turn_id: String,
    /// Null once the message is deleted.
    pub message_id: Option<String>,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub rounds: i32,
    pub tool_calls: i32,
    pub context_tokens: Option<i64>,
    pub instructions_tokens: Option<i64>,
    pub conversation_tokens: Option<i64>,
    pub tools_tokens: Option<i64>,
    pub results_tokens: Option<i64>,
    pub summary_tokens: Option<i64>,
    pub attachments_tokens: Option<i64>,
    pub overhead_tokens: Option<i64>,
    pub estimated: Option<bool>,
    pub recorded_at: String,
}

impl From<TurnRow> for TurnResponse {
    fn from(row: TurnRow) -> Self {
        Self {
            turn_id: row.turn_id.to_string(),
            message_id: row.message_id.map(|id| id.to_string()),
            model: row.model,
            reasoning_effort: row.reasoning_effort,
            prompt_tokens: row.prompt_tokens,
            completion_tokens: row.completion_tokens,
            total_tokens: row.total_tokens,
            rounds: row.rounds,
            tool_calls: row.tool_calls,
            context_tokens: row.context_tokens,
            instructions_tokens: row.instructions_tokens,
            conversation_tokens: row.conversation_tokens,
            tools_tokens: row.tools_tokens,
            results_tokens: row.results_tokens,
            summary_tokens: row.summary_tokens,
            attachments_tokens: row.attachments_tokens,
            overhead_tokens: row.overhead_tokens,
            estimated: row.estimated,
            recorded_at: row.recorded_at.to_rfc3339(),
        }
    }
}
