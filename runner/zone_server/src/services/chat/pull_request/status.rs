use chrono::{DateTime, Utc};
use serde::Serialize;

use super::auto_merge::AutoMerge;
use super::checks::Checks;
use super::merge_method::MergeMethod;
use super::mergeable::Mergeable;
use super::state::State;

/// A pull request as GitHub described it when last asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub title: String,
    pub state: State,
    pub draft: bool,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub checks: Checks,
    pub mergeable: Mergeable,
    pub auto_merge: AutoMerge,
    pub observed_at: DateTime<Utc>,
    /// GitHub's GraphQL id for it, which every change is addressed by.
    #[serde(skip)]
    pub node_id: String,
    /// How auto-merge would merge it: the first way the repository allows.
    #[serde(skip)]
    pub merge_method: Option<MergeMethod>,
}

impl Status {
    pub fn open(&self) -> bool {
        self.state == State::Open
    }
}
