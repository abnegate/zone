use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::spend::Scope;

use crate::db::spend::budgets::BudgetRow;

#[derive(Debug, Serialize)]
pub struct AuditedBudget {
    pub scope: Scope,
    pub workspace_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub limit_microdollars: i64,
    pub threshold_percent: u8,
    pub hard: bool,
}

impl From<&BudgetRow> for AuditedBudget {
    fn from(row: &BudgetRow) -> Self {
        Self {
            scope: row.target.scope,
            workspace_id: row.target.workspace,
            user_id: row.target.user,
            limit_microdollars: row.budget.limit.0,
            threshold_percent: row.budget.threshold_percent,
            hard: row.budget.hard,
        }
    }
}
