use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::spend::{Scope, State};

use crate::db::spend::budgets::BudgetRow;

#[derive(Debug, Serialize)]
pub struct BudgetResponse {
    pub id: Uuid,
    pub scope: Scope,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<String>,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub limit_microdollars: i64,
    pub threshold_percent: u8,
    pub hard: bool,
    /// This month's, in UTC.
    pub spent_microdollars: i64,
    pub state: State,
    pub updated_at: DateTime<Utc>,
}

impl From<BudgetRow> for BudgetResponse {
    fn from(row: BudgetRow) -> Self {
        let state = row.state();
        Self {
            id: row.id,
            scope: row.target.scope,
            workspace_id: row.target.workspace,
            workspace_name: row.workspace_name,
            user_id: row.target.user,
            user_name: row.user_name,
            user_email: row.user_email,
            limit_microdollars: row.budget.limit.0,
            threshold_percent: row.budget.threshold_percent,
            hard: row.budget.hard,
            spent_microdollars: row.spent.0,
            state,
            updated_at: row.updated_at,
        }
    }
}
