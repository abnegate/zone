use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::entitlement::Feature;
use crate::unknown_plan::UnknownPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Team,
    Enterprise,
}

impl Plan {
    pub const ALL: [Self; 2] = [Self::Team, Self::Enterprise];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Enterprise => "enterprise",
        }
    }

    pub fn includes(self, feature: Feature) -> bool {
        self >= feature.plan()
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Plan {
    type Err = UnknownPlan;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|plan| plan.as_str() == value)
            .ok_or_else(|| UnknownPlan(value.to_owned()))
    }
}

#[cfg(test)]
mod tests;
