//! Deciding which queued task should run next.
//!
//! `ORDER BY priority DESC, queued_at ASC` answers the question with one
//! column. This answers it with five, and keeps the components so the choice
//! can be explained. Ordering is total: equal scores fall back to blast radius
//! and then to identifier, so the same input always produces the same run
//! order.

use super::change::{Change, Origin};
use super::prioritiser::{Prioritiser, Verdict};
use super::ratio::Ratio;
use super::score::Score;
use super::signals::{Level, Severity, Signals};

/// A queued task in the terms this module understands, so a caller holding a
/// database row does not have to know how a `Change` is shaped.
#[derive(Debug, Clone, Default)]
pub struct QueuedTask {
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub priority: Option<i32>,
    pub paths: Vec<String>,
    pub labels: Vec<String>,
    pub attempts: u64,
    pub dependents: u64,
    pub failures: u64,
    pub level: Level,
    pub clustered: bool,
}

impl QueuedTask {
    pub fn into_change(self) -> Change {
        let signals = Signals {
            severity: self.priority.map(Severity::from_rank).unwrap_or_default(),
            occurrences: self.attempts,
            dependents: self.dependents,
            escalation: Ratio::of(self.failures, self.attempts),
            unhandled: self.failures > 0,
            level: self.level,
            clustered: self.clustered,
        };
        Change {
            identifier: self.identifier,
            origin: Origin::Task,
            title: self.title,
            description: self.description,
            paths: self.paths,
            symbols: Vec::new(),
            labels: self.labels,
            signals,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub change: Change,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Queue {
    pub ready: Vec<Position>,
    pub suppressed: Vec<Position>,
}

impl Queue {
    pub fn next(&self) -> Option<&Position> {
        self.ready.first()
    }

    pub fn identifiers(&self) -> Vec<&str> {
        self.ready
            .iter()
            .map(|position| position.change.identifier.as_str())
            .collect()
    }
}

pub fn order(prioritiser: &Prioritiser, changes: Vec<Change>) -> Queue {
    let mut ready = Vec::new();
    let mut suppressed = Vec::new();

    for change in changes {
        let verdict = prioritiser.assess(&change);
        let position = Position { change, verdict };
        if position.verdict.suppressed() {
            suppressed.push(position);
        } else {
            ready.push(position);
        }
    }

    ready.sort_by(|left, right| {
        Score::rank(&left.verdict.score, &right.verdict.score)
            .then(right.verdict.blast_radius.cmp(&left.verdict.blast_radius))
            .then(left.change.identifier.cmp(&right.change.identifier))
    });

    Queue { ready, suppressed }
}

#[cfg(test)]
mod tests {
    use super::{QueuedTask, order};
    use crate::services::prioritisation::change::{Change, Origin};
    use crate::services::prioritisation::configuration::Configuration;
    use crate::services::prioritisation::prioritiser::Prioritiser;
    use crate::services::prioritisation::ratio::Ratio;
    use crate::services::prioritisation::signals::{Level, Severity, Signals};
    use crate::services::prioritisation::suppression::{Field, Rule};

    fn task(identifier: &str, severity: Severity, paths: &[&str]) -> Change {
        Change {
            paths: paths.iter().map(|path| (*path).to_string()).collect(),
            signals: Signals {
                severity,
                ..Signals::default()
            },
            ..Change::new(identifier, Origin::Task)
        }
    }

    fn prioritiser() -> Prioritiser {
        Prioritiser::new(Configuration::default())
    }

    #[test]
    fn the_highest_scoring_task_runs_next() {
        let queue = order(
            &prioritiser(),
            vec![
                task("docs", Severity::Low, &["README.md"]),
                task("auth", Severity::Critical, &["src/auth/session.rs"]),
                task("format", Severity::Medium, &["src/formatting/pretty.rs"]),
            ],
        );
        assert_eq!(queue.identifiers(), vec!["auth", "format", "docs"]);
        assert_eq!(
            queue
                .next()
                .map(|position| position.change.identifier.as_str()),
            Some("auth")
        );
    }

    #[test]
    fn blast_radius_breaks_a_tie_on_score() {
        let queue = order(
            &prioritiser(),
            vec![
                task("cosmetic", Severity::Medium, &["README.md"]),
                task("critical", Severity::Medium, &["src/auth/login.rs"]),
            ],
        );
        assert_eq!(queue.identifiers(), vec!["critical", "cosmetic"]);
    }

    #[test]
    fn ordering_is_deterministic_for_identical_tasks() {
        let queue = order(
            &prioritiser(),
            vec![
                task("charlie", Severity::High, &["src/api/routes.rs"]),
                task("alpha", Severity::High, &["src/api/routes.rs"]),
                task("bravo", Severity::High, &["src/api/routes.rs"]),
            ],
        );
        assert_eq!(queue.identifiers(), vec!["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn suppressed_tasks_leave_the_run_order_but_are_still_reported() {
        let configuration = Configuration {
            suppression: vec![Rule {
                reason: "Generated code".into(),
                ..Rule::new("generated", Field::Path, "generated")
            }],
            ..Configuration::default()
        };
        let queue = order(
            &Prioritiser::new(configuration),
            vec![
                task("real", Severity::High, &["src/api/routes.rs"]),
                task(
                    "generated",
                    Severity::Critical,
                    &["src/generated/schema.rs"],
                ),
            ],
        );

        assert_eq!(queue.identifiers(), vec!["real"]);
        assert_eq!(queue.suppressed.len(), 1);
        assert_eq!(queue.suppressed[0].change.identifier, "generated");
    }

    #[test]
    fn an_empty_queue_has_no_next_task() {
        assert!(order(&prioritiser(), Vec::new()).next().is_none());
    }

    #[test]
    fn a_database_row_becomes_a_change() {
        let change = QueuedTask {
            identifier: "task-1".into(),
            title: "Fix session expiry".into(),
            priority: Some(5),
            paths: vec!["src/auth/session.rs".into()],
            attempts: 4,
            failures: 3,
            dependents: 2,
            level: Level::Error,
            clustered: true,
            ..QueuedTask::default()
        }
        .into_change();

        assert_eq!(change.origin, Origin::Task);
        assert_eq!(change.signals.severity, Severity::Critical);
        assert_eq!(change.signals.escalation, Ratio::new(0.75));
        assert!(change.signals.unhandled);
    }

    #[test]
    fn a_row_without_a_priority_takes_the_lowest_severity() {
        let change = QueuedTask {
            identifier: "task-1".into(),
            ..QueuedTask::default()
        }
        .into_change();

        assert_eq!(change.signals.severity, Severity::None);
        assert_eq!(change.signals.escalation, Ratio::ZERO);
        assert!(!change.signals.unhandled);
    }

    #[test]
    fn a_repeatedly_failing_task_outranks_a_fresh_one_of_equal_priority() {
        let fresh = QueuedTask {
            identifier: "fresh".into(),
            priority: Some(3),
            paths: vec!["src/api/routes.rs".into()],
            ..QueuedTask::default()
        }
        .into_change();
        let stuck = QueuedTask {
            identifier: "stuck".into(),
            priority: Some(3),
            paths: vec!["src/api/routes.rs".into()],
            attempts: 8,
            failures: 8,
            level: Level::Error,
            ..QueuedTask::default()
        }
        .into_change();

        let queue = order(&prioritiser(), vec![fresh, stuck]);
        assert_eq!(queue.identifiers(), vec!["stuck", "fresh"]);
    }
}
