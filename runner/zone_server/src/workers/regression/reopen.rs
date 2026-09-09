//! The fix shipped, and somebody moved the task back.
//!
//! A person reopening a completed task is the strongest signal zone has that a
//! fix did not hold, because unlike a failed run it is a judgement rather than
//! an observation. So the bar here is about attribution rather than volume: was
//! the reopen close enough to the fix to be about the fix, and does the status
//! it moved to actually mean the work came undone?
//!
//! Two filters. A reopen inside [`RegressionPolicy::reopen_grace`] is somebody
//! correcting a status they set by mistake, and a reopen after
//! [`RegressionPolicy::monitoring`] is a new problem on an old task rather than
//! this fix unravelling. Between the two, a task pulled back into review is
//! [`Verdict::Suspected`] — more eyes is not the same as broken — and a task
//! pulled back into work is [`Verdict::Regressed`].

use chrono::NaiveDateTime;

use super::checker::{
    Evidence, FixSubject, RegressionChecker, RegressionError, RegressionPolicy, RegressionResult,
    Verdict,
};

const NAME: &str = "reopen";
const COMPLETE: &str = "complete";
const REVIEW: &str = "review";

/// Watches for a completed task being moved back out of completion.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReopenChecker {
    policy: RegressionPolicy,
}

impl ReopenChecker {
    pub fn new(policy: RegressionPolicy) -> Self {
        Self { policy }
    }

    /// The whole decision, pure over the subject.
    pub fn decide(&self, subject: &FixSubject, now: NaiveDateTime) -> RegressionResult {
        if subject.status == COMPLETE {
            return RegressionResult::clear(NAME, subject, "The task is still complete", now);
        }

        let since_shipping = subject.last_touched_at - subject.shipped_at;
        if since_shipping < self.policy.reopen_grace {
            return RegressionResult::clear(
                NAME,
                subject,
                format!(
                    "Moved to {} {} minutes after completion, which is a correction",
                    subject.status,
                    since_shipping.num_minutes().max(0)
                ),
                now,
            );
        }

        if since_shipping > self.policy.monitoring {
            return RegressionResult::clear(
                NAME,
                subject,
                format!(
                    "Moved to {} {} days after completion, too late to blame the fix",
                    subject.status,
                    since_shipping.num_days()
                ),
                now,
            );
        }

        let verdict = if subject.status == REVIEW {
            Verdict::Suspected
        } else {
            Verdict::Regressed
        };

        RegressionResult::new(
            NAME,
            subject,
            verdict,
            1.0,
            vec![Evidence::new(
                format!(
                    "Reopened as {} {} hours after it was completed",
                    subject.status,
                    since_shipping.num_hours()
                ),
                1,
            )],
            now,
        )
    }
}

#[async_trait::async_trait]
impl RegressionChecker for ReopenChecker {
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
    use super::super::checker::fixtures::{moment, subject};
    use super::*;
    use crate::workers::learning::error_category::ErrorCategory;

    fn reopened(status: &str, last_touched_at: NaiveDateTime) -> FixSubject {
        FixSubject {
            status: status.to_string(),
            last_touched_at,
            ..subject(ErrorCategory::Build, Vec::new())
        }
    }

    fn checked(subject: &FixSubject) -> RegressionResult {
        ReopenChecker::default().decide(subject, moment(4, 9, 0))
    }

    #[test]
    fn a_task_moved_back_into_work_the_next_day_is_a_regression() {
        let result = checked(&reopened("in_progress", moment(2, 9, 0)));

        assert_eq!(result.verdict, Verdict::Regressed);
        assert_eq!(result.confidence, 1.0);
        assert!(
            result
                .summary()
                .contains("Reopened as in_progress 24 hours"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_blocked_task_is_a_regression_too() {
        assert_eq!(
            checked(&reopened("blocked", moment(2, 9, 0))).verdict,
            Verdict::Regressed
        );
    }

    #[test]
    fn a_task_still_complete_says_nothing() {
        let result = checked(&reopened("complete", moment(3, 9, 0)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(!result.is_reportable());
    }

    #[test]
    fn a_status_corrected_within_the_hour_is_not_a_regression() {
        let result = checked(&reopened("in_progress", moment(1, 9, 30)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("which is a correction"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_reopen_long_after_the_fix_is_a_new_problem() {
        let result = checked(&reopened("in_progress", moment(25, 9, 0)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("too late to blame the fix"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_task_pulled_back_into_review_is_only_suspected() {
        let result = checked(&reopened("review", moment(2, 9, 0)));

        assert_eq!(
            result.verdict,
            Verdict::Suspected,
            "another pair of eyes is not a regression"
        );
        assert!(!result.is_reportable());
    }

    #[tokio::test]
    async fn the_trait_returns_what_the_pure_decision_returns() {
        let subject = reopened("in_progress", moment(2, 9, 0));
        let checker = ReopenChecker::default();

        assert_eq!(
            checker
                .check(&subject, moment(4, 9, 0))
                .await
                .expect("a pure checker cannot fail"),
            checker.decide(&subject, moment(4, 9, 0))
        );
        assert_eq!(checker.name(), "reopen");
    }
}
