//! Reviewer endpoints that stop answering, followed across ticks.
//!
//! A restarting model server fails a review without judging anything, so such
//! a tick records no round. Each reviewer a task asks keeps its own streak of
//! unanswered attempts. Once a streak reaches [`ATTEMPTS`] the task asks the
//! next reviewer in its lineup instead, and once every reviewer's streak has
//! also lasted [`WINDOW`] the task waits for a person.
//!
//! A streak ends only when a reviewer answers or the task leaves review. Time
//! between attempts never ends one: a drive reviews a project's tasks one after
//! another, so a task's attempts can land many minutes apart while its
//! siblings are reviewed.
//!
//! The streaks live in the driver's memory, so a restart starts them all over:
//! the task asks its round's own reviewer first again, and every reviewer gets
//! its attempts and its window once more.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use uuid::Uuid;

use super::model;
use crate::services::endpoint::Origin;

/// Unanswered attempts after which a task asks its next reviewer, and which a
/// streak needs before it can pause a task.
pub const ATTEMPTS: u32 = 5;
/// How long every reviewer's streak has to last before the task pauses.
pub const WINDOW: TimeDelta = TimeDelta::minutes(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outage {
    pub attempts: u32,
    pub since: DateTime<Utc>,
    pub last: DateTime<Utc>,
}

impl Outage {
    fn begin(now: DateTime<Utc>) -> Self {
        Self {
            attempts: 1,
            since: now,
            last: now,
        }
    }

    fn extend(self, now: DateTime<Utc>) -> Self {
        Self {
            attempts: self.attempts + 1,
            since: self.since,
            last: now,
        }
    }

    /// Whether the task should ask another reviewer before this one again.
    pub fn exhausted(&self) -> bool {
        self.attempts >= ATTEMPTS
    }

    /// Whether the endpoint has been silent long enough, and often enough,
    /// that another tick is unlikely to reach it.
    pub fn outlasted(&self) -> bool {
        self.exhausted() && self.last - self.since >= WINDOW
    }

    fn describe(&self, reviewer: &str) -> String {
        format!(
            "{reviewer} on {} attempts over {} minutes",
            self.attempts,
            (self.last - self.since).num_minutes()
        )
    }
}

/// Why a task waits for a person once no reviewer in its lineup answered, in
/// the words of the last failure.
pub fn reason(streaks: &[(&str, Outage)], failure: &str, origin: Origin) -> String {
    match streaks {
        [(reviewer, streak)] => format!(
            "the reviewer model {reviewer} could not be reached on {} attempts over {} minutes; \
             the last: {failure}. Check that the model server is running",
            streak.attempts,
            (streak.last - streak.since).num_minutes()
        ),
        _ => format!(
            "no reviewer model could be reached: {}; the last: {failure}. Check that the model \
             server is running, or {}",
            streaks
                .iter()
                .map(|(reviewer, streak)| streak.describe(reviewer))
                .collect::<Vec<_>>()
                .join(", "),
            model::remedy(origin)
        ),
    }
}

#[derive(Debug)]
struct Streaks {
    project: Uuid,
    reviewers: HashMap<String, Outage>,
}

/// Each task's streaks, one per reviewer it asked, kept for the life of the
/// driver.
#[derive(Debug, Default)]
pub struct Outages(DashMap<Uuid, Streaks>);

impl Outages {
    /// Count an unanswered attempt by `reviewer` at reviewing `task` and
    /// return its streak.
    pub fn record(&self, project: Uuid, task: Uuid, reviewer: &str, now: DateTime<Utc>) -> Outage {
        let mut entry = self.0.entry(task).or_insert_with(|| Streaks {
            project,
            reviewers: HashMap::new(),
        });
        entry.project = project;
        let streak = match entry.reviewers.get(reviewer) {
            Some(previous) => previous.extend(now),
            None => Outage::begin(now),
        };
        entry.reviewers.insert(reviewer.to_string(), streak);
        streak
    }

    /// `reviewer`'s current streak on `task`, if it has one.
    pub fn streak(&self, task: Uuid, reviewer: &str) -> Option<Outage> {
        self.0
            .get(&task)
            .and_then(|entry| entry.reviewers.get(reviewer).copied())
    }

    /// Which of `lineup`, in its order, `task` asks next: the first reviewer
    /// with attempts to spare, else the first whose streak has not yet lasted
    /// the window. `None` once every one has.
    pub fn next(&self, task: Uuid, lineup: &[String]) -> Option<usize> {
        let streaks: Vec<Option<Outage>> = lineup
            .iter()
            .map(|reviewer| self.streak(task, reviewer))
            .collect();
        streaks
            .iter()
            .position(|streak| !streak.is_some_and(|streak| streak.exhausted()))
            .or_else(|| {
                streaks
                    .iter()
                    .position(|streak| !streak.is_some_and(|streak| streak.outlasted()))
            })
    }

    /// Why `task` waits for a person, naming every reviewer in `lineup` that
    /// went unanswered.
    pub fn reason(&self, task: Uuid, lineup: &[String], failure: &str, origin: Origin) -> String {
        let streaks: Vec<(&str, Outage)> = lineup
            .iter()
            .filter_map(|reviewer| {
                self.streak(task, reviewer)
                    .map(|streak| (reviewer.as_str(), streak))
            })
            .collect();
        reason(&streaks, failure, origin)
    }

    /// End `task`'s streaks: a reviewer answered, or a person was told.
    pub fn clear(&self, task: Uuid) {
        self.0.remove(&task);
    }

    /// Forget `project`'s tasks that are no longer awaiting review, which
    /// leave it without an answer when they move on mid-outage.
    pub fn retain(&self, project: Uuid, reviewing: &HashSet<Uuid>) {
        self.0
            .retain(|task, streaks| streaks.project != project || reviewing.contains(task));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::UNIX_EPOCH + TimeDelta::minutes(minutes)
    }

    fn lineup(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_lone_reviewer_pauses_only_after_enough_attempts_over_the_whole_window() {
        let outages = Outages::default();
        let (project, task) = (Uuid::new_v4(), Uuid::new_v4());
        let only = lineup(&["qwen3:32b"]);

        for minute in 0..=i64::from(ATTEMPTS) {
            outages.record(project, task, "qwen3:32b", at(minute));
            assert_eq!(
                outages.next(task, &only),
                Some(0),
                "a server restarting for a few ticks is retried at minute {minute}"
            );
        }

        outages.record(project, task, "qwen3:32b", at(9));
        assert_eq!(outages.next(task, &only), Some(0));
        let streak = outages.record(project, task, "qwen3:32b", at(10));
        assert_eq!(outages.next(task, &only), None);
        assert_eq!(streak.attempts, ATTEMPTS + 3);
        assert_eq!(streak.since, at(0));
    }

    #[test]
    fn attempts_spaced_a_window_apart_by_sibling_reviews_still_pause_the_task() {
        let outages = Outages::default();
        let (project, task) = (Uuid::new_v4(), Uuid::new_v4());
        let only = lineup(&["qwen3:32b"]);

        let paused_after = (1..=20).find(|attempt| {
            outages.record(project, task, "qwen3:32b", at(attempt * 10));
            outages.next(task, &only).is_none()
        });

        assert_eq!(
            paused_after,
            Some(i64::from(ATTEMPTS)),
            "a gap between attempts is not an answer"
        );
    }

    #[test]
    fn a_reviewer_that_used_its_attempts_hands_the_task_to_the_next() {
        let outages = Outages::default();
        let (project, task) = (Uuid::new_v4(), Uuid::new_v4());
        let pair = lineup(&["gemma3:27b", "qwen3:32b"]);

        for minute in 0..i64::from(ATTEMPTS) {
            assert_eq!(outages.next(task, &pair), Some(0));
            outages.record(project, task, "gemma3:27b", at(minute));
        }

        assert_eq!(outages.next(task, &pair), Some(1));
        assert_eq!(outages.streak(task, "qwen3:32b"), None);
    }

    #[test]
    fn once_every_reviewer_used_its_attempts_the_one_still_inside_its_window_is_asked() {
        let outages = Outages::default();
        let (project, task) = (Uuid::new_v4(), Uuid::new_v4());
        let pair = lineup(&["gemma3:27b", "qwen3:32b"]);
        for minute in 0..i64::from(ATTEMPTS) {
            outages.record(project, task, "gemma3:27b", at(minute * 3));
            outages.record(project, task, "qwen3:32b", at(20 + minute));
        }

        assert_eq!(
            outages.next(task, &pair),
            Some(1),
            "gemma3:27b has been silent for 12 minutes, qwen3:32b for 4"
        );
        outages.record(project, task, "qwen3:32b", at(30));
        assert_eq!(outages.next(task, &pair), None);
    }

    #[test]
    fn an_answer_ends_every_streak_on_the_task() {
        let outages = Outages::default();
        let (project, task) = (Uuid::new_v4(), Uuid::new_v4());
        for minute in 0..i64::from(ATTEMPTS) {
            outages.record(project, task, "gemma3:27b", at(minute));
        }

        outages.clear(task);

        assert_eq!(
            outages.record(project, task, "gemma3:27b", at(60)).attempts,
            1
        );
    }

    #[test]
    fn each_task_and_reviewer_keeps_its_own_streak() {
        let outages = Outages::default();
        let project = Uuid::new_v4();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());

        outages.record(project, first, "gemma3:27b", at(0));
        outages.record(project, first, "gemma3:27b", at(1));

        assert_eq!(
            outages
                .record(project, second, "gemma3:27b", at(1))
                .attempts,
            1
        );
        assert_eq!(
            outages.record(project, first, "qwen3:32b", at(1)).attempts,
            1
        );
    }

    #[test]
    fn tasks_that_left_review_are_forgotten_and_other_projects_are_left_alone() {
        let outages = Outages::default();
        let (project, other) = (Uuid::new_v4(), Uuid::new_v4());
        let (reviewing, fixing, elsewhere) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for task in [reviewing, fixing] {
            outages.record(project, task, "gemma3:27b", at(0));
        }
        outages.record(other, elsewhere, "gemma3:27b", at(0));

        outages.retain(project, &HashSet::from([reviewing]));

        assert!(outages.streak(reviewing, "gemma3:27b").is_some());
        assert_eq!(outages.streak(fixing, "gemma3:27b"), None);
        assert!(
            outages.streak(elsewhere, "gemma3:27b").is_some(),
            "another project's drive decides what its tasks are doing"
        );

        outages.retain(project, &HashSet::new());
        assert_eq!(outages.streak(reviewing, "gemma3:27b"), None);
    }

    #[test]
    fn the_pause_names_the_reviewer_the_streak_and_the_last_failure() {
        let streak = Outage {
            attempts: 7,
            since: at(0),
            last: at(12),
        };

        assert_eq!(
            reason(
                &[("qwen3:32b", streak)],
                "connection refused",
                Origin::Instance
            ),
            "the reviewer model qwen3:32b could not be reached on 7 attempts over 12 minutes; the \
             last: connection refused. Check that the model server is running"
        );
    }

    #[test]
    fn the_pause_names_every_reviewer_that_went_unanswered() {
        let first = Outage {
            attempts: 5,
            since: at(0),
            last: at(40),
        };
        let second = Outage {
            attempts: 6,
            since: at(50),
            last: at(61),
        };

        assert_eq!(
            reason(
                &[("gemma3:27b", first), ("qwen3:32b", second)],
                "HTTP 503",
                Origin::Instance
            ),
            "no reviewer model could be reached: gemma3:27b on 5 attempts over 40 minutes, \
             qwen3:32b on 6 attempts over 11 minutes; the last: HTTP 503. Check that the model \
             server is running, or name another in ZONE_AUTO_REVIEW_MODELS"
        );
    }

    #[test]
    fn a_saved_endpoint_s_pause_points_at_ai_settings() {
        let streak = Outage {
            attempts: 5,
            since: at(0),
            last: at(40),
        };

        let reason = reason(
            &[("gemma3:27b", streak), ("qwen3:32b", streak)],
            "HTTP 503",
            Origin::Settings,
        );

        assert!(
            reason.ends_with("or set the Fast/Reasoning model in AI Settings"),
            "{reason}"
        );
    }
}
