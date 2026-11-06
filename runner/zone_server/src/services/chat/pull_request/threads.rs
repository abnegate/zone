use abnegate_vcs::ReviewThreadCount;
use serde::Serialize;

/// How many of a pull request's review threads are still open, and how many
/// someone resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Threads {
    pub open: u32,
    pub resolved: u32,
}

impl Threads {
    /// The counts GitHub's threads add up to, or nothing when the read
    /// stopped short of every thread, since one it never reached may be the
    /// thread still open.
    pub fn counted(count: ReviewThreadCount) -> Option<Self> {
        count.complete.then_some(Self {
            open: count.open,
            resolved: count.resolved,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_resolved_threads_are_carried_apart() {
        assert_eq!(
            Threads::counted(ReviewThreadCount::new(2, 3, true)),
            Some(Threads {
                open: 2,
                resolved: 3,
            })
        );
    }

    #[test]
    fn a_pull_request_without_threads_counts_zero() {
        assert_eq!(
            Threads::counted(ReviewThreadCount::new(0, 0, true)),
            Some(Threads::default())
        );
    }

    #[test]
    fn a_count_that_stopped_short_is_unknown() {
        assert_eq!(
            Threads::counted(ReviewThreadCount::new(0, 1000, false)),
            None
        );
    }
}
