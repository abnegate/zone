use serde::Serialize;

use crate::db::chat_token_usage::Totals;

/// What a set of turns added up to. `reported_turns` counts only the turns a
/// provider gave a figure for, the denominator of a per-turn average.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TotalsResponse {
    pub turns: i64,
    pub reported_turns: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub rounds: i64,
    pub tool_calls: i64,
    /// The largest context any one turn ended on, not a sum.
    pub peak_context_tokens: i64,
    pub instructions_tokens: i64,
    pub conversation_tokens: i64,
    pub tools_tokens: i64,
    pub results_tokens: i64,
    pub summary_tokens: i64,
    pub attachments_tokens: i64,
    pub overhead_tokens: i64,
    pub first_at: Option<String>,
    pub last_at: Option<String>,
}

impl From<Totals> for TotalsResponse {
    fn from(totals: Totals) -> Self {
        Self {
            turns: totals.turns,
            reported_turns: totals.reported_turns,
            prompt_tokens: totals.prompt_tokens,
            completion_tokens: totals.completion_tokens,
            total_tokens: totals.total_tokens,
            rounds: totals.rounds,
            tool_calls: totals.tool_calls,
            peak_context_tokens: totals.peak_context_tokens,
            instructions_tokens: totals.instructions_tokens,
            conversation_tokens: totals.conversation_tokens,
            tools_tokens: totals.tools_tokens,
            results_tokens: totals.results_tokens,
            summary_tokens: totals.summary_tokens,
            attachments_tokens: totals.attachments_tokens,
            overhead_tokens: totals.overhead_tokens,
            first_at: totals.first_at.map(|at| at.to_rfc3339()),
            last_at: totals.last_at.map(|at| at.to_rfc3339()),
        }
    }
}
