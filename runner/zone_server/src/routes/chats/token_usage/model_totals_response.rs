use serde::Serialize;

use super::totals_response::TotalsResponse;
use crate::db::chat_token_usage::ModelTotals;

/// One model's place in a workspace rollup. Comparing two models over the same
/// window is the cheapest token experiment there is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelTotalsResponse {
    pub model: String,
    pub totals: TotalsResponse,
}

impl From<ModelTotals> for ModelTotalsResponse {
    fn from(totals: ModelTotals) -> Self {
        Self {
            model: totals.model,
            totals: totals.totals.into(),
        }
    }
}
