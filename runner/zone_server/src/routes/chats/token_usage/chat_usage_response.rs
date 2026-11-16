use serde::Serialize;

use super::totals_response::TotalsResponse;
use super::turn_response::TurnResponse;

/// What one chat has spent, and the turns behind it, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatUsageResponse {
    pub chat_id: String,
    pub totals: TotalsResponse,
    pub turns: Vec<TurnResponse>,
    /// Whether `turns` was cut short by the limit. The totals cover every
    /// recorded turn either way, so a client must not re-add the listed turns
    /// and expect the total.
    pub truncated: bool,
}
