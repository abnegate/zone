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
//! pulled back into work is [`Verdict::Regressed`]. A completed task moved to
//! cancelled was dropped, not reopened; one moved to paused is unfinished
//! work again, so it counts as a reopen.

use chrono::NaiveDateTime;

use crate::db::tasks::CANCELLED;
use crate::db::tasks::COMPLETE;
use crate::db::tasks::REVIEW;

use super::checker::{
    Evidence, FixSubject, RegressionChecker, RegressionError, RegressionPolicy, RegressionResult,
    Verdict,
};

const NAME: &str = "reopen";

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
        if subject.status == CANCELLED {
            return RegressionResult::clear(
                NAME,
                subject,
                "Cancelled after completion, which is not a reopen",
                now,
            );
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
    use std::sync::Arc;

    use chrono::Duration;
    use uuid::Uuid;

    use super::super::checker::fixtures::{moment, subject};
    use super::super::worker::{RegressionSettings, scan_workspace};
    use super::*;
    use crate::db::analytics;
    use crate::db::auto_projects::testing::Project;
    use crate::db::tasks;
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
        let result = checked(&reopened(tasks::IN_PROGRESS, moment(2, 9, 0)));

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
            checked(&reopened(tasks::BLOCKED, moment(2, 9, 0))).verdict,
            Verdict::Regressed
        );
    }

    #[test]
    fn a_completed_task_cancelled_inside_the_monitoring_window_is_not_a_reopen() {
        let result = checked(&reopened(CANCELLED, moment(2, 9, 0)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(!result.is_reportable());
        assert_eq!(
            result.summary(),
            "Cancelled after completion, which is not a reopen"
        );
    }

    #[test]
    fn a_completed_task_paused_inside_the_monitoring_window_is_still_a_regression() {
        assert_eq!(
            checked(&reopened(tasks::PAUSED, moment(2, 9, 0))).verdict,
            Verdict::Regressed,
            "paused work is unfinished work"
        );
    }

    #[test]
    fn a_task_still_complete_says_nothing() {
        let result = checked(&reopened(COMPLETE, moment(3, 9, 0)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(!result.is_reportable());
    }

    #[test]
    fn a_status_corrected_within_the_hour_is_not_a_regression() {
        let result = checked(&reopened(tasks::IN_PROGRESS, moment(1, 9, 30)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("which is a correction"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_reopen_long_after_the_fix_is_a_new_problem() {
        let result = checked(&reopened(tasks::IN_PROGRESS, moment(25, 9, 0)));

        assert_eq!(result.verdict, Verdict::Clear);
        assert!(
            result.summary().contains("too late to blame the fix"),
            "got {}",
            result.summary()
        );
    }

    #[test]
    fn a_task_pulled_back_into_review_is_only_suspected() {
        let result = checked(&reopened(REVIEW, moment(2, 9, 0)));

        assert_eq!(
            result.verdict,
            Verdict::Suspected,
            "another pair of eyes is not a regression"
        );
        assert!(!result.is_reportable());
    }

    #[tokio::test]
    async fn the_trait_returns_what_the_pure_decision_returns() {
        let subject = reopened(tasks::IN_PROGRESS, moment(2, 9, 0));
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

    /// The database's own clock, in the zoneless form a task is stamped in, so
    /// a window lines up with the stamps whatever zone the server keeps.
    async fn database_now(project: &Project) -> NaiveDateTime {
        sqlx::query_scalar("SELECT LOCALTIMESTAMP")
            .fetch_one(&project.pool)
            .await
            .expect("the database's clock")
    }

    async fn shipped(project: &Project) -> Vec<Uuid> {
        let now = database_now(project).await;
        analytics::shipped_tasks(
            &project.pool,
            project.workspace,
            now - RegressionPolicy::default().monitoring,
            now + Duration::seconds(1),
            100,
        )
        .await
        .expect("the shipped tasks")
        .into_iter()
        .map(|task| task.task_id)
        .collect()
    }

    async fn scanned(project: &Project) -> Vec<RegressionResult> {
        let checkers: Vec<Arc<dyn RegressionChecker>> = vec![Arc::new(ReopenChecker::default())];
        scan_workspace(
            &project.pool,
            project.workspace,
            &checkers,
            &RegressionSettings::default(),
            database_now(project).await + Duration::seconds(1),
        )
        .await
        .expect("the scan")
    }

    /// Put everything stamped on the task two hours back, as if they had passed.
    async fn two_hours_pass(project: &Project, task: Uuid) {
        sqlx::query(
            "UPDATE tasks SET completed_at = completed_at - INTERVAL '2 hours', \
               shipped_at = shipped_at - INTERVAL '2 hours', \
               updated_at = updated_at - INTERVAL '2 hours' WHERE id = $1",
        )
        .bind(task)
        .execute(&project.pool)
        .await
        .expect("the task's stamps move back");
    }

    #[tokio::test]
    async fn a_task_that_never_completed_is_no_shipped_fix_wherever_it_is_moved() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        let run = tasks::create_task_run(&project.pool, task)
            .await
            .expect("a run");
        tasks::complete_task_run(&project.pool, run.id, "failed", Some("boom"), None)
            .await
            .expect("the run fails");
        let ended = project.status(task).await;
        two_hours_pass(&project, task).await;

        tasks::update_task(
            &project.pool,
            task,
            Some("Renamed"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("an edit that leaves the status alone");
        let after_edit = (shipped(&project).await, scanned(&project).await);
        project.move_to(task, tasks::PAUSED).await;
        let after_pause = (shipped(&project).await, scanned(&project).await);
        project.remove().await;

        assert_eq!(ended, tasks::BLOCKED);
        assert_eq!(
            after_edit,
            (Vec::new(), Vec::new()),
            "a run that ended is not a fix that shipped"
        );
        assert_eq!(
            after_pause,
            (Vec::new(), Vec::new()),
            "pausing a task that was never complete reopens nothing"
        );
    }

    #[tokio::test]
    async fn a_task_a_person_completed_is_watched_and_moving_it_back_out_is_a_reopen() {
        let project = Project::new(1).await;
        let task = project.tasks[0];

        project.move_to(task, COMPLETE).await;
        let completed = (shipped(&project).await, scanned(&project).await);
        two_hours_pass(&project, task).await;
        project.move_to(task, tasks::PAUSED).await;
        let reopened = (shipped(&project).await, scanned(&project).await);
        project.remove().await;

        assert_eq!(
            completed,
            (vec![task], Vec::new()),
            "a task moved to complete by hand is a fix to watch, and still complete"
        );
        assert_eq!(reopened.0, vec![task]);
        let [result] = &reopened.1[..] else {
            panic!("one reopened fix: {:?}", reopened.1);
        };
        assert_eq!(result.verdict, Verdict::Regressed);
        assert_eq!(
            result.summary(),
            "Reopened as paused 2 hours after it was completed"
        );
    }
}
