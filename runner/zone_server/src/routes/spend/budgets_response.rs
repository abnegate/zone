use serde::Serialize;
use zone_enterprise::{Edition, Status};

use super::budget_response::BudgetResponse;

#[derive(Debug, Serialize)]
pub struct BudgetsResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    pub month: String,
    pub budgets: Vec<BudgetResponse>,
}
