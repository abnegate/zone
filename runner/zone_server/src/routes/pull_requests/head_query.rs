use serde::Deserialize;

use crate::services::github::readiness::CommitSha;

pub const INVALID_HEAD: &str = "head must be the pull request's 40 character head commit.";

#[derive(Debug, Deserialize)]
pub struct HeadQuery {
    pub head: String,
}

impl HeadQuery {
    pub fn head(&self) -> Option<CommitSha> {
        CommitSha::parse(&self.head)
    }
}
