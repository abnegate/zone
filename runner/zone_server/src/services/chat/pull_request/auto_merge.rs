use serde::Serialize;

/// Whether GitHub merges the pull request on its own once it can, and what
/// stops turning that on when nothing can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoMerge {
    pub enabled: bool,
    pub unavailable: Option<String>,
}

impl AutoMerge {
    pub const OFF_FOR_REPOSITORY: &str = "Auto-merge is turned off for this repository.";
    pub const NOT_PERMITTED: &str = "This source's credential cannot change auto-merge.";
    pub const DRAFT: &str = "Mark it ready for review first.";
    pub const SETTLED: &str = "It is no longer open.";
    pub const READY: &str = "It can be merged now, so there is nothing to wait for.";
    pub const UNREAD: &str = "GitHub's auto-merge settings could not be read.";

    pub fn unavailable(reason: &str) -> Self {
        Self {
            enabled: false,
            unavailable: Some(reason.to_string()),
        }
    }
}
