use crate::db::chat_pull_requests::PullRequestRow;

use super::connection::Connection;
use super::status::Status;

/// One of a chat's pull requests, with what GitHub last said of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub row: PullRequestRow,
    /// `None` when no source can read it, or GitHub could not be asked.
    pub status: Option<Status>,
    pub connection: Connection,
}
