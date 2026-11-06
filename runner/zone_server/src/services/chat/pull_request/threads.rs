use abnegate_vcs::ReviewThreadCount;
use serde::Serialize;

/// How many of a pull request's review threads are still open, and how many
/// someone resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Threads {
    pub open: u32,
    pub resolved: u32,
}

impl From<ReviewThreadCount> for Threads {
    fn from(count: ReviewThreadCount) -> Self {
        Self {
            open: count.open,
            resolved: count.resolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_resolved_threads_are_carried_apart() {
        assert_eq!(
            Threads::from(ReviewThreadCount::new(2, 3)),
            Threads {
                open: 2,
                resolved: 3,
            }
        );
    }

    #[test]
    fn a_pull_request_without_threads_counts_none() {
        assert_eq!(
            Threads::from(ReviewThreadCount::default()),
            Threads::default()
        );
    }
}
