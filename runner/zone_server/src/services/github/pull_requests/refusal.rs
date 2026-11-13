use crate::services::github::readiness::BlockerReport;

pub const HEAD_MOVED: &str = "head_moved";
pub const NOT_READY: &str = "not_ready";
pub const CONFLICT: &str = "conflict";
pub const PROTECTED: &str = "protected";

const HEAD_MOVED_MESSAGE: &str =
    "The pull request's head moved since it was read. Review the new head first.";
const NOT_READY_MESSAGE: &str = "The pull request is not ready to merge.";
const CONFLICT_MESSAGE: &str = "The pull request no longer merges cleanly into its base.";
const PROTECTED_MESSAGE: &str = "Branch protection refused the merge.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    HeadMoved { current_head: String },
    NotReady { blockers: Vec<BlockerReport> },
    Conflict,
    Protected,
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::HeadMoved { .. } => HEAD_MOVED,
            Self::NotReady { .. } => NOT_READY,
            Self::Conflict => CONFLICT,
            Self::Protected => PROTECTED,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::HeadMoved { .. } => HEAD_MOVED_MESSAGE,
            Self::NotReady { .. } => NOT_READY_MESSAGE,
            Self::Conflict => CONFLICT_MESSAGE,
            Self::Protected => PROTECTED_MESSAGE,
        }
    }
}
