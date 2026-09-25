//! Why Claude's token endpoint refused a grant, as its error response says.

use serde::Deserialize;

use super::reason::Reason;

#[derive(Deserialize)]
pub(super) struct Failure {
    error: Option<Reason>,
    error_description: Option<String>,
}

impl Failure {
    pub(super) fn description(self) -> Option<String> {
        [self.error_description, self.error.map(Reason::text)]
            .into_iter()
            .flatten()
            .find(|text| !text.trim().is_empty())
    }
}
