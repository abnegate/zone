use crate::services::github::readiness::BlockerReport;

pub const HEAD_MOVED: &str = "head_moved";
pub const NOT_READY: &str = "not_ready";
pub const CONFLICT: &str = "conflict";
pub const PROTECTED: &str = "protected";
pub const MERGED: &str = "merged";
pub const CLOSED: &str = "closed";
const HEAD_MOVED_MESSAGE: &str =
    "The pull request's head moved since it was read. Review the new head first.";
const NOT_READY_MESSAGE: &str = "The pull request is not ready to merge.";
const CONFLICT_MESSAGE: &str = "The pull request no longer merges cleanly into its base.";
const PROTECTED_MESSAGE: &str = "Branch protection refused the merge.";
const MERGED_MESSAGE: &str = "The pull request was already merged.";
const CLOSED_MESSAGE: &str = "The pull request is closed. Reopen it to merge it.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    HeadMoved { current_head: String },
    NotReady { blockers: Vec<BlockerReport> },
    Conflict,
    Protected,
    Merged,
    Closed,
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::HeadMoved { .. } => HEAD_MOVED,
            Self::NotReady { .. } => NOT_READY,
            Self::Conflict => CONFLICT,
            Self::Protected => PROTECTED,
            Self::Merged => MERGED,
            Self::Closed => CLOSED,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::HeadMoved { .. } => HEAD_MOVED_MESSAGE,
            Self::NotReady { .. } => NOT_READY_MESSAGE,
            Self::Conflict => CONFLICT_MESSAGE,
            Self::Protected => PROTECTED_MESSAGE,
            Self::Merged => MERGED_MESSAGE,
            Self::Closed => CLOSED_MESSAGE,
        }
    }

    pub fn outdates(&self) -> bool {
        matches!(
            self,
            Self::HeadMoved { .. } | Self::Conflict | Self::Merged | Self::Closed
        )
    }
}
