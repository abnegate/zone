use serde::Deserialize;
use zone_enterprise::fleet::{Error, Labels};

use super::refusal::Refusal;
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::state::State;

/// Sets the fleet's minimum version, or clears it when `minimum_version` is absent or null.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateFleetPolicyRequest {
    pub minimum_version: Option<String>,
    #[serde(default)]
    pub confirm: bool,
}

impl UpdateFleetPolicyRequest {
    pub fn minimum(&self) -> Result<Option<Minimum>, Error> {
        self.minimum_version
            .as_deref()
            .map(Minimum::parse)
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRunnerRequest {
    pub state: String,
    #[serde(default)]
    pub confirm: bool,
}

impl UpdateRunnerRequest {
    pub fn requested(&self) -> Result<State, Refusal> {
        State::parse(&self.state).ok_or_else(|| Refusal::State(self.state.clone()))
    }
}

/// Replaces the labels a scope requires; an empty list requires none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRunnerLabelsRequest {
    pub labels: Vec<String>,
}

impl UpdateRunnerLabelsRequest {
    pub fn required(&self) -> Result<Labels, Error> {
        Labels::from_strings(&self.labels)
    }
}
