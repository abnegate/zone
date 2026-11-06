use abnegate_vcs::ReviewThreadRecord;
use serde::Serialize;

/// How many of a pull request's review threads are still open, and how many
/// someone resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Threads {
    pub open: u32,
    pub resolved: u32,
}

impl From<&[ReviewThreadRecord]> for Threads {
    fn from(records: &[ReviewThreadRecord]) -> Self {
        records.iter().fold(Self::default(), |threads, record| {
            if record.resolved {
                Self {
                    resolved: threads.resolved + 1,
                    ..threads
                }
            } else {
                Self {
                    open: threads.open + 1,
                    ..threads
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, resolved: bool) -> ReviewThreadRecord {
        ReviewThreadRecord::new(id, Vec::new()).with_resolved(resolved)
    }

    #[test]
    fn open_and_resolved_threads_are_counted_apart() {
        let records = [
            thread("PRRT_1", false),
            thread("PRRT_2", true),
            thread("PRRT_3", false),
            thread("PRRT_4", true),
            thread("PRRT_5", true),
        ];
        assert_eq!(
            Threads::from(records.as_slice()),
            Threads {
                open: 2,
                resolved: 3,
            }
        );
    }

    #[test]
    fn a_pull_request_without_threads_counts_none() {
        assert_eq!(Threads::from([].as_slice()), Threads::default());
    }

    #[test]
    fn an_outdated_thread_still_counts_by_whether_it_was_resolved() {
        let records = [thread("PRRT_1", false).with_outdated(true)];
        assert_eq!(
            Threads::from(records.as_slice()),
            Threads {
                open: 1,
                resolved: 0,
            }
        );
    }
}
