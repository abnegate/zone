use serde::Serialize;

use super::totals_response::TotalsResponse;
use crate::db::chat_token_usage::ChatTotals;

/// One chat's place in a workspace rollup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatTotalsResponse {
    pub chat_id: String,
    pub title: String,
    pub totals: TotalsResponse,
}

impl From<ChatTotals> for ChatTotalsResponse {
    fn from(totals: ChatTotals) -> Self {
        Self {
            chat_id: totals.chat_id.to_string(),
            title: totals.title,
            totals: totals.totals.into(),
        }
    }
}
