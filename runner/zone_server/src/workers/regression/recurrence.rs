//! The fix shipped, and the failure it was for came back.
//!
//! The signal is cheap and entirely internal: zone already records what every
//! run failed on and the learning loop already says what kind of failure it was.
//! What makes this worth having is not finding the recurrence — that is one
//! filter — but refusing to call most recurrences a regression.
//!
//! The bar, in full: at least [`RegressionPolicy::minimum_failures`] settled
//! runs failed in the same category the fix addressed, that category is one the
//! code can actually cause, the failures span at least
//! [`RegressionPolicy::minimum_occasions`] occasions more than
//! [`RegressionPolicy::occasion_gap`] apart, and they make up at least
//! [`RegressionPolicy::minimum_failure_share`] of everything that ran. Anything
//! that trips one filter but not all of them is [`Verdict::Suspected`], which is
//! recorded and never sent.

use chrono::NaiveDateTime;

use super::checker::{
    Evidence, FixSubject, RegressionChecker, RegressionError, RegressionPolicy, RegressionResult,
    Verdict, is_attributable,
};
use crate::workers::analytics::AgentRun;
use crate::workers::learning::attempt::AttemptOutcome;

const NAME: &str = "recurrence";

/// Watches for the fixed failure coming back on the same task.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecurrenceChecker {
    policy: RegressionPolicy,
}

impl RecurrenceChecker {
    pub fn new(policy: RegressionPolicy) -> Self {
        Self { policy }
    }

    /// The whole decision, pure over the subject.
    pub fn decide(&self, subject: &FixSubject, now: NaiveDateTime) -> RegressionResult {
        if !is_attributable(subject.addressed) {
            return RegressionResult::clear(
                NAME,
                subject,
                format!(
                    "The fix addressed a {} failure, which code cannot be blamed for",
                    subject.addressed
                ),
                now,
            );
        }

        let settled = subject.settled_runs(&self.policy);
        if settled.is_empty() {
            return RegressionResult::clear(
                NAME,
                subject,
                "Nothing has run on this task since the fix settled",
                now,
            );
        }

        let recurrences: Vec<&AgentRun> = settled
            .iter()
            .copied()
            .filter(|run| {
                run.outcome == AttemptOutcome::Failed && run.category == subject.addressed
            })
            .collect();

        if recurrences.is_empty() {
            return RegressionResult::clear(
                NAME,
                subject,
                format!(
                    "{} runs since the fix, none of them a {} failure",
                    settled.len(),
                    subject.addressed
                ),
                now,
            );
        }

        let occasions = count_occasions(&recurrences, &self.policy);
        let share = recurrences.len() as f64 / settled.len() as f64;

        let cleared = recurrences.len() >= self.policy.minimum_failures
            && occasions >= self.policy.minimum_occasions
            && share >= self.policy.minimum_failure_share;

        let evidence = vec![
            Evidence::new(
                format!(
                    "{} of {} runs since the fix failed on {} again",
                    recurrences.len(),
                    settled.len(),
                    subject.addressed
                ),
                recurrences.len(),
            ),
            Evidence::new(
                format!(
                    "spread over {occasions} separate occasions at least {} minutes apart",
                    self.policy.occasion_gap.num_minutes()
                ),
                occasions,
            ),
        ];

        RegressionResult::new(
            NAME,
            subject,
            if cleared {
                Verdict::Regressed
            } else {
                Verdict::Suspected
            },
            share,
            evidence,
            now,
        )
    }
}

/// Collapse failures closer together than the gap into one occasion.
///
/// A task that retried five times in as many minutes hit one problem five
/// times, not five problems, and counting it as five is the fastest way to a
/// false positive.
fn count_occasions(failures: &[&AgentRun], policy: &RegressionPolicy) -> usize {
    let mut ordered: Vec<NaiveDateTime> = failures.iter().map(|run| run.finished_at).collect();
    ordered.sort_unstable();

    let mut occasions = 0usize;
    let mut last_counted: Option<NaiveDateTime> = None;
    for moment in ordered {
        let separate = last_counted.is_none_or(|last| moment - last >= policy.occasion_gap);
        if separate {
            occasions += 1;
            last_counted = Some(moment);
        }
    }
    occasions
}

#[async_trait::async_trait]
impl RegressionChecker for RecurrenceChecker {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn check(
        &self,
        subject: &FixSubject,
        now: NaiveDateTime,
    ) -> Result<RegressionResult, RegressionError> {
        Ok(self.decide(subject, now))
    }
}

#[cfg(test)]
mod tests {
    use super::super::checker::fixtures::{moment, run, subject};
    use super::*;
    use crate::workers::learning::error_category::ErrorCategory;

    fn failure(index: u128, day: u32, hour: u32, minute: u32) -> AgentRun {
        run(
            index,
            AttemptOutcome::Failed,
            ErrorCategory::TestFailure,
            moment(day, hour, minute),
        )
    }

    fn success(index: u128, day: u32, hour: u32, minute: u32) -> AgentRun {
        run(
            index,
            AttemptOutcome::Succeeded,
            ErrorCategory::Unknown,
            moment(day, hour, minute),
        )
    }

    fn checked(subject: &FixSubject) -> RegressionResult {
        RecurrenceChecker::default().decide(subject, moment(3, 9, 0))
    }

    #[test]
    fn the_same_failure_coming_back_across_days_is_a_regression() {
        let subject = subject(
            ErrorCategory::TestFailure,
            vec![
                failure(1, 1, 12, 0),
                failure(2, 2, 9, 0),
                failure(3, 2, 15, 0),
            ],
        );

        let result = checked(&subject);

        assert_eq!(result.verdict, Verdict::Regressed);
        assert!(result.is_reportable());
        assert_eq!(result.confidence, 1.0);
        assert_eq!(result.checker, "recurrence");
        assert!(
            result.summary().contains("3 of 3 runs"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn one_failure_is_never_a_regression() {
        let subject = subject(
            ErrorCategory::TestFailure,
            vec![failure(1, 1, 12, 0), success(2, 2, 9, 0)],
        );

        let result = checked(&subject);

        assert_eq!(
            result.verdict,
            Verdict::Suspected,
            "recorded, but nobody is woken up"
        );
        assert!(!result.is_reportable());
    }

    #[test]
    fn a_retry_storm_inside_ten_minutes_is_one_occasion() {
        let subject = subject(
            ErrorCategory::TestFailure,
            vec![
                failure(1, 1, 12, 0),
                failure(2, 1, 12, 3),
                failure(3, 1, 12, 6),
                failure(4, 1, 12, 9),
            ],
        );

        let result = checked(&subject);

        assert_eq!(
            result.verdict,
            Verdict::Suspected,
            "four retries of one problem is one problem"
        );
        assert_eq!(
            result.evidence[1].occurrences, 1,
            "collapsed into a single occasion"
        );
    }

    #[test]
    fn a_few_failures_among_many_runs_is_a_flake_not_a_regression() {
        let mut runs = vec![
            failure(1, 1, 12, 0),
            failure(2, 2, 9, 0),
            failure(3, 2, 15, 0),
        ];
        runs.extend((0..17).map(|index| success(100 + index, 2, 20, index as u32)));

        let result = checked(&subject(ErrorCategory::TestFailure, runs));

        assert_eq!(result.verdict, Verdict::Suspected);
        assert!(
            result.confidence < 0.2,
            "three in twenty, got {}",
            result.confidence
        );
    }

    #[test]
    fn an_environmental_failure_is_never_blamed_on_the_fix() {
        let subject = subject(
            ErrorCategory::RateLimit,
            vec![
                run(
                    1,
                    AttemptOutcome::Failed,
                    ErrorCategory::RateLimit,
                    moment(1, 12, 0),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::RateLimit,
                    moment(2, 9, 0),
                ),
                run(
                    3,
                    AttemptOutcome::Failed,
                    ErrorCategory::RateLimit,
                    moment(2, 15, 0),
                ),
            ],
        );

        let result = checked(&subject);

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("cannot be blamed"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_different_failure_coming_up_is_not_this_fix_regressing() {
        let subject = subject(
            ErrorCategory::TestFailure,
            vec![
                run(
                    1,
                    AttemptOutcome::Failed,
                    ErrorCategory::MergeConflict,
                    moment(1, 12, 0),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::MergeConflict,
                    moment(2, 9, 0),
                ),
                run(
                    3,
                    AttemptOutcome::Failed,
                    ErrorCategory::MergeConflict,
                    moment(2, 15, 0),
                ),
            ],
        );

        let result = checked(&subject);

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("none of them"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn failures_while_the_fix_is_still_settling_are_ignored() {
        let subject = subject(
            ErrorCategory::TestFailure,
            vec![
                failure(1, 1, 9, 5),
                failure(2, 1, 9, 15),
                failure(3, 1, 9, 25),
            ],
        );

        let result = checked(&subject);

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("Nothing has run"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_quiet_task_is_clear_rather_than_unknown() {
        let result = checked(&subject(ErrorCategory::Build, Vec::new()));

        assert_eq!(result.verdict, Verdict::Clear);
        assert_eq!(result.confidence, 0.0);
    }

    #[tokio::test]
    async fn the_trait_returns_what_the_pure_decision_returns() {
        let subject = subject(
            ErrorCategory::TestFailure,
            vec![
                failure(1, 1, 12, 0),
                failure(2, 2, 9, 0),
                failure(3, 2, 15, 0),
            ],
        );
        let checker = RecurrenceChecker::default();

        let through_trait = checker
            .check(&subject, moment(3, 9, 0))
            .await
            .expect("a pure checker cannot fail");

        assert_eq!(through_trait, checker.decide(&subject, moment(3, 9, 0)));
        assert_eq!(checker.name(), "recurrence");
    }
}
