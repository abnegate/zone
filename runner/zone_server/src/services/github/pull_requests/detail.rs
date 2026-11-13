use serde::Serialize;

use super::BotComment;
use super::PullRequest;
use super::Thread;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    pub pull_request: PullRequest,
    pub threads: Vec<Thread>,
    pub comments: Vec<BotComment>,
}
