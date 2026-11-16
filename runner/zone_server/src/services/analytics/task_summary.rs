use serde::{Deserialize, Serialize};

use super::Tally;
use crate::db::analytics::{CountRow, TaskActivityRow};
use crate::db::tasks::{Origin, STATUSES};

/// Where a workspace's tasks stand now, and what moved inside the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub statuses: Vec<Tally>,
    pub origins: Vec<Tally>,
    pub created: usize,
    pub shipped: usize,
    pub reopened: usize,
}

impl TaskSummary {
    pub fn new(statuses: &[CountRow], origins: &[CountRow], activity: TaskActivityRow) -> Self {
        Self {
            statuses: Tally::fixed(STATUSES, statuses),
            origins: Tally::fixed(Origin::ALL.map(Origin::as_str), origins),
            created: count(activity.created),
            shipped: count(activity.shipped),
            reopened: count(activity.reopened),
        }
    }
}

fn count(value: i64) -> usize {
    usize::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tasks::{BLOCKED, COMPLETE};

    fn row(name: &str, count: i64) -> CountRow {
        CountRow {
            name: name.to_string(),
            count,
        }
    }

    fn quiet() -> TaskActivityRow {
        TaskActivityRow {
            created: 0,
            shipped: 0,
            reopened: 0,
        }
    }

    #[test]
    fn every_status_is_listed_in_order_including_zeros() {
        let summary = TaskSummary::new(&[row(COMPLETE, 4), row(BLOCKED, 1)], &[], quiet());

        let names: Vec<&str> = summary
            .statuses
            .iter()
            .map(|tally| tally.name.as_str())
            .collect();
        assert_eq!(names, STATUSES);
        assert_eq!(
            summary
                .statuses
                .iter()
                .map(|tally| tally.count)
                .sum::<usize>(),
            5
        );
        assert_eq!(
            summary
                .statuses
                .iter()
                .find(|tally| tally.name == COMPLETE)
                .map(|tally| tally.count),
            Some(4)
        );
    }

    #[test]
    fn every_origin_is_listed_in_order_including_zeros() {
        let summary = TaskSummary::new(&[], &[row(Origin::Automation.as_str(), 2)], quiet());
        assert_eq!(
            summary.origins,
            vec![
                Tally::new(Origin::Member.as_str(), 0),
                Tally::new(Origin::Reminder.as_str(), 0),
                Tally::new(Origin::Automation.as_str(), 2),
            ]
        );
    }

    #[test]
    fn activity_carries_through() {
        let summary = TaskSummary::new(
            &[],
            &[],
            TaskActivityRow {
                created: 7,
                shipped: 3,
                reopened: 1,
            },
        );
        assert_eq!(
            (summary.created, summary.shipped, summary.reopened),
            (7, 3, 1)
        );
    }
}
