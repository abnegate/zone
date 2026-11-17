use serde::Serialize;

use crate::db::chat_token_usage::TurnRow;

/// One assistant turn's spend.
///
/// The three billed counts are null, not zero, when the provider reported
/// nothing: several coding-agent backends do, and a zero would read as a free
/// turn. The breakdown is always present because zone estimates it itself, and
/// `estimated` says whether that estimate came from a tokenizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnResponse {
    pub message_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub rounds: i32,
    pub tool_calls: i32,
    pub context_tokens: i64,
    pub instructions_tokens: i64,
    pub conversation_tokens: i64,
    pub tools_tokens: i64,
    pub results_tokens: i64,
    pub summary_tokens: i64,
    pub attachments_tokens: Option<i64>,
    pub overhead_tokens: i64,
    pub estimated: bool,
    pub recorded_at: String,
}

impl From<TurnRow> for TurnResponse {
    fn from(row: TurnRow) -> Self {
        Self {
            message_id: row.message_id.to_string(),
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
