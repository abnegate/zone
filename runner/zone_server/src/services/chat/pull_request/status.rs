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

    /// Whether this pull request could be merged as it stands: open and out of
    /// draft, every check passing, nothing in the way of the merge, and no
    /// review thread left unresolved.
    ///
    /// Every part of that has to have been read. Checks that could not be
    /// reached, or threads GitHub would not list in full, leave this false,
    /// because unread is not the same as clean.
    pub fn ready_to_merge(&self) -> bool {
        self.open()
            && !self.draft
            && self.checks == Checks::Passing
            && self.mergeable == Mergeable::Clean
            && self.threads.is_some_and(|threads| threads.open == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Status {
        Status {
            title: "Add widgets".to_string(),
            state: State::Open,
            draft: false,
            additions: 10,
            deletions: 2,
            changed_files: 3,
            checks: Checks::Passing,
            mergeable: Mergeable::Clean,
            auto_merge: AutoMerge::unavailable(AutoMerge::UNREAD),
            draft_unavailable: None,
            threads: Some(Threads {
                open: 0,
                resolved: 4,
            }),
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn passing_checks_a_clean_merge_and_no_open_threads_is_ready() {
        assert!(ready().ready_to_merge());
    }

    #[test]
    fn anything_short_of_all_three_is_not_ready() {
        for (reason, status) in [
            (
                "a draft",
                Status {
                    draft: true,
                    ..ready()
                },
            ),
            (
                "checks still running",
                Status {
                    checks: Checks::Pending,
                    ..ready()
                },
            ),
            (
                "checks failing",
                Status {
                    checks: Checks::Failing,
                    ..ready()
                },
            ),
            (
                "no checks at all",
                Status {
                    checks: Checks::None,
                    ..ready()
                },
            ),
            (
                "merge conflicts",
                Status {
                    mergeable: Mergeable::Conflicted,
                    ..ready()
                },
            ),
            (
                "a blocked merge",
                Status {
                    mergeable: Mergeable::Blocked,
                    ..ready()
                },
            ),
            (
                "an open review thread",
                Status {
                    threads: Some(Threads {
                        open: 1,
                        resolved: 0,
                    }),
                    ..ready()
                },
            ),
            (
                "threads that could not be listed in full",
                Status {
                    threads: None,
                    ..ready()
                },
            ),
            (
                "already merged",
                Status {
                    state: State::Merged,
                    ..ready()
                },
            ),
        ] {
            assert!(!status.ready_to_merge(), "{reason} read as ready to merge");
        }
    }
}
