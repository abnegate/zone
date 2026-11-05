use abnegate_vcs::ChecksOutcome;
use serde::Serialize;

/// What the checks on a pull request's head commit add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Checks {
    Passing,
    Failing,
    Pending,
    None,
    Unknown,
}

impl From<&ChecksOutcome> for Checks {
    fn from(outcome: &ChecksOutcome) -> Self {
        match outcome {
            ChecksOutcome::Success => Self::Passing,
            ChecksOutcome::Failure(_) => Self::Failing,
            ChecksOutcome::Absent => Self::None,
            _ => Self::Pending,
        }
    }
}
