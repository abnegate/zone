use super::announcement::Announcement;
use super::budget_row::BudgetRow;

/// A budget past its threshold or limit whose notice hasn't gone out this month.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub row: BudgetRow,
    pub announcement: Announcement,
}
