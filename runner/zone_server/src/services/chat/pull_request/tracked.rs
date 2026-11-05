use crate::db::chat_pull_requests::PullRequestRow;

use super::status::Status;

/// One of a chat's pull requests, with what GitHub last said of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub row: PullRequestRow,
    /// `None` when no source can read it, or GitHub could not be asked.
    pub status: Option<Status>,
    /// Whether one of the workspace's sources holds a credential for it.
    pub credentialed: bool,
}
