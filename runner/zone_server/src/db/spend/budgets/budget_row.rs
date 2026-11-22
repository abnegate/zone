use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::spend::{Budget, Microdollars, State, Target};

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub target: Target,
    pub budget: Budget,
    pub workspace_name: Option<String>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub spent: Microdollars,
    pub updated_at: DateTime<Utc>,
}

impl BudgetRow {
    pub fn state(&self) -> State {
        self.budget.state(self.spent)
    }
}
