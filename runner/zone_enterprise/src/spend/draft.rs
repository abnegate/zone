use serde::Deserialize;
use uuid::Uuid;

use crate::spend::budget::{Budget, DEFAULT_THRESHOLD};
use crate::spend::error::Error;
use crate::spend::scope::Scope;
use crate::spend::target::Target;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub scope: Scope,
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    #[serde(default)]
    pub user_id: Option<Uuid>,
    pub limit_microdollars: i64,
    #[serde(default = "default_threshold")]
    pub threshold_percent: i64,
    #[serde(default)]
    pub hard: bool,
}

impl Draft {
    pub fn validate(&self) -> Result<(Target, Budget), Error> {
        let budget = Budget::checked(self.limit_microdollars, self.threshold_percent, self.hard)?;
        let target = Target::new(self.scope, self.workspace_id, self.user_id)?;
        Ok((target, budget))
    }
}

fn default_threshold() -> i64 {
    i64::from(DEFAULT_THRESHOLD)
}
