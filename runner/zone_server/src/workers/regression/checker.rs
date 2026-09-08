//! What a regression check is asked, and what it is allowed to answer.
//!
//! # The bar
//!
//! A regression check that cries wolf is worse than no check at all: the second
//! false alarm is the one that teaches people to close the notification without
//! reading it, and after that a real regression goes unread too. So the loud
//! answer, [`Verdict::Regressed`], is deliberately hard to reach, and there is a
//! quiet answer, [`Verdict::Suspected`], for everything that looks wrong but has
//! not earned an interruption. Only the loud one is delivered.
//!
//! Four things push a false positive towards `Suspected` instead:
//!
//! - **A failure the code cannot cause.** A run that died on a rate limit or a
//!   reset connection says nothing about the change that shipped. Only the
//!   categories in [`is_attributable`] count as evidence.
//! - **The same failure retried.** One task retried five times in as many
//!   minutes is one event, not five, so failures inside
//!   [`RegressionPolicy::occasion_gap`] of each other collapse into one occasion
//!   and several separate occasions are required.
//! - **The work still settling.** The minutes right after a fix ships are full
//!   of runs that started before it, so a settling period is skipped entirely.
//! - **A rate that is not actually worse.** Three failures out of eighty runs is
//!   a flake, not a regression, so the failures have to make up a real share of
//!   what ran.

use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::workers::analytics::AgentRun;
use crate::workers::learning::error_category::ErrorCategory;

const SETTLING_MINUTES: i64 = 30;
const MONITORING_DAYS: i64 = 7;
const MINIMUM_FAILURES: usize = 3;
const MINIMUM_OCCASIONS: usize = 2;
const OCCASION_GAP_MINUTES: i64 = 10;
const MINIMUM_FAILURE_SHARE: f64 = 0.5;
const REOPEN_GRACE_MINUTES: i64 = 60;

/// Whether a failure of this kind can be blamed on the change that shipped.
///
/// A throttled API, a reset connection, a deadline and a refusal all happen to
/// code that is perfectly correct, and a checker that counts them will report a
/// regression every time the network has a bad afternoon.
pub fn is_attributable(category: ErrorCategory) -> bool {
    matches!(
        category,
        ErrorCategory::Build
            | ErrorCategory::TestFailure
            | ErrorCategory::Dependency
            | ErrorCategory::MergeConflict
            | ErrorCategory::Configuration
    )
}

/// Every bar a check applies, in one place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegressionPolicy {
    /// Runs finishing this soon after a fix are still the same work settling.
    pub settling: Duration,
    /// How long after shipping a fix is still watched.
    pub monitoring: Duration,
    /// Failures needed before the same kind is called a recurrence.
    pub minimum_failures: usize,
    /// Separate occasions those failures must span.
    pub minimum_occasions: usize,
    /// How far apart two failures must be to count as separate occasions.
    pub occasion_gap: Duration,
    /// The share of settled runs that must have hit the failure.
    pub minimum_failure_share: f64,
    /// A reopen this soon after completion is a correction, not a regression.
    pub reopen_grace: Duration,
}

impl Default for RegressionPolicy {
    fn default() -> Self {
        Self {
            settling: Duration::minutes(SETTLING_MINUTES),
            monitoring: Duration::days(MONITORING_DAYS),
            minimum_failures: MINIMUM_FAILURES,
            minimum_occasions: MINIMUM_OCCASIONS,
            occasion_gap: Duration::minutes(OCCASION_GAP_MINUTES),
            minimum_failure_share: MINIMUM_FAILURE_SHARE,
            reopen_grace: Duration::minutes(REOPEN_GRACE_MINUTES),
        }
    }
}

/// A fix that shipped, and everything already known about what happened next.
///
/// Built once per task and handed to every checker, so a checker that only reads
/// what zone already recorded stays a pure function. A checker that has to ask
/// an external service is still free to, which is why [`RegressionChecker`] is
/// asynchronous.
#[derive(Debug, Clone, PartialEq)]
pub struct FixSubject {
    pub task_id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    /// The task's status now.
    pub status: String,
    pub shipped_at: NaiveDateTime,
    /// When the task was last changed, which is when it was reopened if it was.
    pub last_touched_at: NaiveDateTime,
    /// The failure the fix was addressing: the last one classified before it
    /// shipped, or [`ErrorCategory::Unknown`] if it was never diagnosed.
    pub addressed: ErrorCategory,
    /// Runs on this task that finished after it shipped, oldest first.
    pub later_runs: Vec<AgentRun>,
}

impl FixSubject {
    /// Runs late enough after the fix to be about the fix, and early enough to
    /// still be attributed to it.
    pub fn settled_runs(&self, policy: &RegressionPolicy) -> Vec<&AgentRun> {
        let opens = self.shipped_at + policy.settling;
        let closes = self.shipped_at + policy.monitoring;
        self.later_runs
            .iter()
            .filter(|run| run.finished_at >= opens && run.finished_at <= closes)
            .collect()
    }

    /// Whether the fix is still inside its watch window.
    pub fn is_monitored(&self, policy: &RegressionPolicy, now: NaiveDateTime) -> bool {
        now <= self.shipped_at + policy.monitoring
    }
}

/// One thing a checker saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// What was observed, in one line, as it will read in a notification.
    pub observation: String,
    /// How many separate observations back it.
    pub occurrences: usize,
}

impl Evidence {
    pub fn new(observation: impl Into<String>, occurrences: usize) -> Self {
        Self {
            observation: observation.into(),
            occurrences,
        }
    }
}

/// How sure a checker is that a fix came undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Nothing to say.
    Clear,
    /// Something looks wrong but has not cleared the bar. Recorded, not sent.
    Suspected,
    /// Cleared the bar. Worth interrupting someone for.
    Regressed,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Clear => "clear",
            Verdict::Suspected => "suspected",
            Verdict::Regressed => "regressed",
        }
    }

    /// Whether this verdict is worth delivering rather than only recording.
    pub fn is_reportable(self) -> bool {
        matches!(self, Verdict::Regressed)
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What one checker concluded about one fix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionResult {
    pub checker: &'static str,
    pub task_id: Uuid,
    pub title: String,
    pub verdict: Verdict,
    /// The share of the evidence pointing at a regression, 0 to 1.
    ///
    /// A measured quantity rather than a score: each checker documents what it
    /// measured, so a reader can tell 0.6 from a different checker's 0.6.
    pub confidence: f64,
    pub evidence: Vec<Evidence>,
    pub checked_at: NaiveDateTime,
}

impl RegressionResult {
    pub fn new(
        checker: &'static str,
        subject: &FixSubject,
        verdict: Verdict,
        confidence: f64,
        evidence: Vec<Evidence>,
        checked_at: NaiveDateTime,
    ) -> Self {
        Self {
            checker,
            task_id: subject.task_id,
            title: subject.title.clone(),
            verdict,
            confidence: confidence.clamp(0.0, 1.0),
            evidence,
            checked_at,
        }
    }

    /// Nothing found, with a note saying what was looked at.
    pub fn clear(
        checker: &'static str,
        subject: &FixSubject,
        observation: impl Into<String>,
        checked_at: NaiveDateTime,
    ) -> Self {
        Self::new(
            checker,
            subject,
            Verdict::Clear,
            0.0,
            vec![Evidence::new(observation, 0)],
            checked_at,
        )
    }

    pub fn is_reportable(&self) -> bool {
        self.verdict.is_reportable()
    }

    /// How much was observed behind the strongest piece of evidence.
    pub fn weight(&self) -> usize {
        self.evidence
            .iter()
            .map(|evidence| evidence.occurrences)
            .max()
            .unwrap_or_default()
    }

    /// Every observation, one per line.
    pub fn summary(&self) -> String {
        self.evidence
            .iter()
            .map(|evidence| evidence.observation.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Error)]
pub enum RegressionError {
    #[error("regression checker {checker} could not reach its source: {message}")]
    Unreachable {
        checker: &'static str,
        message: String,
    },
}

/// One way of asking whether a shipped fix came undone.
///
/// Asynchronous even though both implementations here are pure over
/// [`FixSubject`], because the next one will not be: an issue tracker or a crash
/// reporter answers the same question from outside zone, and it should be able
/// to join the same schedule without reshaping the trait.
#[async_trait::async_trait]
pub trait RegressionChecker: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    async fn check(
        &self,
        subject: &FixSubject,
        now: NaiveDateTime,
    ) -> Result<RegressionResult, RegressionError>;
}

#[cfg(test)]
pub(super) mod fixtures {
    use super::*;
    use crate::workers::learning::attempt::AttemptOutcome;
    use chrono::NaiveDate;

    pub fn moment(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    pub fn run(
        index: u128,
        outcome: AttemptOutcome,
        category: ErrorCategory,
        finished_at: NaiveDateTime,
    ) -> AgentRun {
        AgentRun {
            run_id: Uuid::from_u128(index),
            task_id: Uuid::from_u128(7),
            outcome,
            category,
            finished_at,
            duration_seconds: Some(90.0),
        }
    }

    pub fn subject(addressed: ErrorCategory, later_runs: Vec<AgentRun>) -> FixSubject {
        FixSubject {
            task_id: Uuid::from_u128(7),
            workspace_id: Uuid::from_u128(1),
            title: "Stop the importer dropping rows".to_string(),
            status: "complete".to_string(),
            shipped_at: moment(1, 9, 0),
            last_touched_at: moment(1, 9, 0),
            addressed,
            later_runs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{moment, run, subject};
    use super::*;
    use crate::workers::learning::attempt::AttemptOutcome;

    #[test]
    fn only_failures_the_code_can_cause_count_as_evidence() {
        for attributable in [
            ErrorCategory::Build,
            ErrorCategory::TestFailure,
            ErrorCategory::Dependency,
            ErrorCategory::MergeConflict,
            ErrorCategory::Configuration,
        ] {
            assert!(is_attributable(attributable), "{attributable} is code");
        }

        for environmental in [
            ErrorCategory::Network,
            ErrorCategory::RateLimit,
            ErrorCategory::Timeout,
            ErrorCategory::Permission,
            ErrorCategory::Refusal,
            ErrorCategory::Unknown,
        ] {
            assert!(
                !is_attributable(environmental),
                "{environmental} happens to correct code too"
            );
        }
    }

    #[test]
    fn only_the_loud_verdict_is_delivered() {
        assert!(Verdict::Regressed.is_reportable());
        assert!(!Verdict::Suspected.is_reportable());
        assert!(!Verdict::Clear.is_reportable());
        assert!(Verdict::Regressed > Verdict::Suspected);
        assert!(Verdict::Suspected > Verdict::Clear);
    }

    #[test]
    fn runs_inside_the_settling_period_are_not_looked_at() {
        let policy = RegressionPolicy::default();
        let subject = subject(
            ErrorCategory::Build,
            vec![
                run(
                    1,
                    AttemptOutcome::Failed,
                    ErrorCategory::Build,
                    moment(1, 9, 10),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::Build,
                    moment(1, 10, 30),
                ),
            ],
        );

        let settled = subject.settled_runs(&policy);

        assert_eq!(settled.len(), 1, "the ten-minute-old run is still settling");
        assert_eq!(settled[0].run_id, Uuid::from_u128(2));
    }

    #[test]
    fn runs_past_the_monitoring_window_are_no_longer_attributed_to_the_fix() {
        let policy = RegressionPolicy::default();
        let subject = subject(
            ErrorCategory::Build,
            vec![run(
                1,
                AttemptOutcome::Failed,
                ErrorCategory::Build,
                moment(20, 9, 0),
            )],
        );

        assert!(subject.settled_runs(&policy).is_empty());
        assert!(!subject.is_monitored(&policy, moment(20, 9, 0)));
        assert!(subject.is_monitored(&policy, moment(3, 9, 0)));
    }

    #[test]
    fn confidence_is_clamped_to_a_share() {
        let subject = subject(ErrorCategory::Build, Vec::new());
        let result = RegressionResult::new(
            "test",
            &subject,
            Verdict::Regressed,
            4.2,
            Vec::new(),
            moment(2, 9, 0),
        );

        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn a_clear_result_still_says_what_was_looked_at() {
        let subject = subject(ErrorCategory::Build, Vec::new());
        let result =
            RegressionResult::clear("test", &subject, "nothing ran since", moment(2, 9, 0));

        assert!(!result.is_reportable());
        assert_eq!(result.summary(), "nothing ran since");
        assert_eq!(result.task_id, subject.task_id);
    }
}
