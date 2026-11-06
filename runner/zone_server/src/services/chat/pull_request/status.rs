use chrono::{DateTime, Utc};
use serde::Serialize;

use super::auto_merge::AutoMerge;
use super::checks::Checks;
use super::mergeable::Mergeable;
use super::state::State;
use super::threads::Threads;

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
    /// Why it cannot be moved between draft and ready for review, if it cannot.
    pub draft_unavailable: Option<String>,
    /// Its review threads while it is open, unless GitHub would not list them all.
    pub threads: Option<Threads>,
    pub observed_at: DateTime<Utc>,
}

impl Status {
    pub fn open(&self) -> bool {
        self.state == State::Open
    }
}
