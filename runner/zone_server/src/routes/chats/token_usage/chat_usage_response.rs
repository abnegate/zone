use serde::Serialize;

use super::totals_response::TotalsResponse;
use super::turn_response::TurnResponse;

/// What one chat has spent, and the shares of its turns behind it, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatUsageResponse {
    pub chat_id: String,
    pub totals: TotalsResponse,
    pub turns: Vec<TurnResponse>,
    /// Whether `turns` was cut short by the limit. The totals cover every
    /// recorded turn either way.
    pub truncated: bool,
}
