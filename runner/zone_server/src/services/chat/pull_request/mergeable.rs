use abnegate_vcs::pull_request::MergeableState;
use serde::Serialize;

/// What stands between a pull request and a merge, as GitHub sums it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mergeable {
    Clean,
    Blocked,
    Behind,
    Conflicted,
    Unstable,
    Draft,
    Unknown,
}

impl From<MergeableState> for Mergeable {
    fn from(state: MergeableState) -> Self {
        match state {
            MergeableState::Clean | MergeableState::HasHooks => Self::Clean,
            MergeableState::Blocked => Self::Blocked,
            MergeableState::Behind => Self::Behind,
            MergeableState::Dirty => Self::Conflicted,
            MergeableState::Unstable => Self::Unstable,
            MergeableState::Draft => Self::Draft,
            _ => Self::Unknown,
        }
    }
}
