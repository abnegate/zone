use serde::Deserialize;

use crate::spend::budget::Budget;
use crate::spend::error::Error;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    #[serde(default)]
    pub limit_microdollars: Option<i64>,
    #[serde(default)]
    pub threshold_percent: Option<i64>,
    #[serde(default)]
    pub hard: Option<bool>,
}

impl Patch {
    pub fn applied(&self, budget: &Budget) -> Result<Budget, Error> {
        Budget::checked(
            self.limit_microdollars.unwrap_or(budget.limit.0),
            self.threshold_percent
                .unwrap_or(i64::from(budget.threshold_percent)),
            self.hard.unwrap_or(budget.hard),
        )
    }
}
