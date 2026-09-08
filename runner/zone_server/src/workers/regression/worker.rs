//! Running every checker over every fix that is still being watched.
//!
//! [`subjects`] turns loaded rows into what a checker is asked about, and
//! [`unalerted`] decides what is worth sending. Both are pure, so the schedule
//! can be exercised without a database and the ledger without a network.
//!
//! A fix is alerted on once. The scan runs hourly and a regression stays true
//! for as long as the fix is watched, so without a ledger the same task would
//! produce the same alert every hour until the window closed — which is the
//! other way to teach people to ignore a notification.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{NaiveDateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::checker::{FixSubject, RegressionChecker, RegressionPolicy, RegressionResult, Verdict};
use super::recurrence::RecurrenceChecker;
use super::reopen::ReopenChecker;
use crate::db::DbResult;
use crate::db::analytics::{self, ReportableWorkspace, ShippedTaskRow};
use crate::state::AppState;
use crate::workers::analytics::{AgentRun, TimeWindow, load_runs};
use crate::workers::learning::attempt::AttemptOutcome;
use crate::workers::learning::error_category::ErrorCategory;
use zone_notify::{Fanout, Notification, Severity};

pub const REGRESSION_INTERVAL_SECONDS: u64 = 60 * 60;
const MAXIMUM_TASKS_PER_WORKSPACE: i64 = 500;
const MAXIMUM_RUNS_PER_WORKSPACE: i64 = 10_000;

/// Which fixes a scan covers, and how much of their history it loads.
#[derive(Debug, Clone, PartialEq)]
pub struct RegressionSettings {
    pub policy: RegressionPolicy,
    pub maximum_tasks_per_workspace: i64,
    pub maximum_runs_per_workspace: i64,
}

impl Default for RegressionSettings {
    fn default() -> Self {
        Self {
            policy: RegressionPolicy::default(),
            maximum_tasks_per_workspace: MAXIMUM_TASKS_PER_WORKSPACE,
            maximum_runs_per_workspace: MAXIMUM_RUNS_PER_WORKSPACE,
        }
    }
}

impl RegressionSettings {
    /// The runs a scan has to read.
    ///
    /// Twice the watch window: a fix that shipped at the far edge of it still
    /// needs the runs from before it shipped, which is where the failure it was
    /// addressing is recorded.
    fn history(&self, now: NaiveDateTime) -> TimeWindow {
        TimeWindow::new(now - self.policy.monitoring * 2, now)
    }
}

/// Pair each shipped fix with the runs that followed it.
///
/// The failure a fix addressed is the last one classified before it shipped: an
/// earlier, already-fixed failure on the same task says nothing about the change
/// that just landed.
pub fn subjects(
    workspace_id: Uuid,
    tasks: &[ShippedTaskRow],
    runs: &[AgentRun],
    policy: &RegressionPolicy,
    now: NaiveDateTime,
) -> Vec<FixSubject> {
    let mut by_task: BTreeMap<Uuid, Vec<&AgentRun>> = BTreeMap::new();
    for run in runs {
        by_task.entry(run.task_id).or_default().push(run);
    }

    tasks
        .iter()
        .map(|task| {
            let history = by_task.get(&task.task_id).cloned().unwrap_or_default();
            let mut later_runs: Vec<AgentRun> = history
                .iter()
                .filter(|run| run.finished_at > task.shipped_at)
                .map(|run| (*run).clone())
                .collect();
            later_runs.sort_by_key(|run| (run.finished_at, run.run_id));

            FixSubject {
                task_id: task.task_id,
                workspace_id,
                title: task.title.clone(),
                status: task.status.clone(),
                shipped_at: task.shipped_at,
                last_touched_at: task.last_touched_at,
                addressed: addressed_failure(&history, task.shipped_at),
                later_runs,
            }
        })
        .filter(|subject| subject.is_monitored(policy, now))
        .collect()
}

fn addressed_failure(history: &[&AgentRun], shipped_at: NaiveDateTime) -> ErrorCategory {
    history
        .iter()
        .filter(|run| run.finished_at <= shipped_at && run.outcome == AttemptOutcome::Failed)
        .max_by_key(|run| (run.finished_at, run.run_id))
        .map(|run| run.category)
        .unwrap_or(ErrorCategory::Unknown)
}

/// The reportable results that have not already been alerted on.
pub fn unalerted<'a>(
    results: &'a [RegressionResult],
    alerted: &BTreeMap<Uuid, NaiveDateTime>,
) -> Vec<&'a RegressionResult> {
    results
        .iter()
        .filter(|result| result.is_reportable() && !alerted.contains_key(&result.task_id))
        .collect()
}

/// Forget tasks whose watch window has closed, so the ledger stays bounded.
pub fn prune(
    alerted: &mut BTreeMap<Uuid, NaiveDateTime>,
    policy: &RegressionPolicy,
    now: NaiveDateTime,
) {
    alerted.retain(|_, alerted_at| now - *alerted_at <= policy.monitoring);
}

/// Render one result as the message that goes out.
///
/// The task title and the workspace name are workspace data, and
/// [`Notification::new`] and [`Notification::field`] sanitize what they are
/// given. Nothing here becomes a link, which is the one part a notification
/// does not redact.
fn describe(result: &RegressionResult, workspace: &ReportableWorkspace) -> Notification {
    Notification::new(
        format!("Regression suspected: {}", result.title),
        result.summary(),
    )
    .severity(Severity::Error)
    .field("Workspace", workspace.name.clone())
    .field("Checker", result.checker)
    .field("Confidence", format!("{:.0}%", result.confidence * 100.0))
}

/// Run every checker over every watched fix in one workspace.
pub async fn scan_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    checkers: &[Arc<dyn RegressionChecker>],
    settings: &RegressionSettings,
    now: NaiveDateTime,
) -> DbResult<Vec<RegressionResult>> {
    let start = now - settings.policy.monitoring;
    let tasks = analytics::shipped_tasks(
        pool,
        workspace_id,
        start,
        now,
        settings.maximum_tasks_per_workspace,
    )
    .await?;

    if tasks.is_empty() {
        return Ok(Vec::new());
    }

    let runs = load_runs(
        pool,
        workspace_id,
        settings.history(now),
        settings.maximum_runs_per_workspace,
    )
    .await?;

    let watched = subjects(workspace_id, &tasks, &runs, &settings.policy, now);
    Ok(check_all(checkers, &watched, now).await)
}

/// Ask every checker about every fix, keeping only what they had to say.
///
/// A checker that fails costs its own answer and nothing else. One of them
/// reaching an external service that is down should not lose the verdicts the
/// others already reached about the same fix.
pub async fn check_all(
    checkers: &[Arc<dyn RegressionChecker>],
    subjects: &[FixSubject],
    now: NaiveDateTime,
) -> Vec<RegressionResult> {
    let mut results = Vec::new();

    for subject in subjects {
        for checker in checkers {
            match checker.check(subject, now).await {
                Ok(result) => {
                    crate::metrics::record_regression(result.checker, result.verdict.as_str());
                    if result.verdict != Verdict::Clear {
                        results.push(result);
                    }
                }
                Err(error) => tracing::warn!(
                    checker = checker.name(),
                    task_id = %subject.task_id,
                    %error,
                    "Regression check failed; retrying next cycle"
                ),
            }
        }
    }

    results
}

/// Check every monitored fix once, alerting on the ones that came back.
pub async fn run_cycle(
    state: &AppState,
    checkers: &[Arc<dyn RegressionChecker>],
    settings: &RegressionSettings,
    fanout: &Fanout,
    alerted: &mut BTreeMap<Uuid, NaiveDateTime>,
) -> DbResult<()> {
    let pool = state.db();
    let now = Utc::now().naive_utc();
    prune(alerted, &settings.policy, now);

    for workspace in
        analytics::reportable_workspaces(pool, now - settings.policy.monitoring).await?
    {
        let results =
            match scan_workspace(pool, workspace.workspace_id, checkers, settings, now).await {
                Ok(results) => results,
                Err(error) => {
                    tracing::warn!(
                        workspace_id = %workspace.workspace_id,
                        %error,
                        "Regression scan failed; retrying next cycle"
                    );
                    continue;
                }
            };

        for result in unalerted(&results, alerted) {
            tracing::warn!(
                workspace_id = %workspace.workspace_id,
                task_id = %result.task_id,
                checker = result.checker,
                confidence = result.confidence,
                "Regression detected on a shipped fix"
            );
            let report = fanout.deliver(&describe(result, &workspace)).await;
            for delivery in report.deliveries() {
                crate::metrics::record_report_delivery(
                    delivery.channel().as_str(),
                    if delivery.is_delivered() {
                        "delivered"
                    } else {
                        "failed"
                    },
                );
            }
            alerted.insert(result.task_id, now);
        }
    }

    Ok(())
}

/// The checkers a scan runs, in the order they were written.
pub fn checkers(settings: &RegressionSettings) -> Vec<Arc<dyn RegressionChecker>> {
    vec![
        Arc::new(RecurrenceChecker::new(settings.policy)),
        Arc::new(ReopenChecker::new(settings.policy)),
    ]
}

/// Which fixes have already been alerted on, so a regression is reported once
/// rather than every hour it stays broken.
pub type Alerted = BTreeMap<Uuid, NaiveDateTime>;

#[cfg(test)]
mod tests {
    use super::super::checker::fixtures::{moment, run};
    use super::*;

    fn task(status: &str, shipped: NaiveDateTime, touched: NaiveDateTime) -> ShippedTaskRow {
        ShippedTaskRow {
            task_id: Uuid::from_u128(7),
            title: "Stop the importer dropping rows".to_string(),
            status: status.to_string(),
            shipped_at: shipped,
            last_touched_at: touched,
            pull_request_url: None,
        }
    }

    fn built(runs: Vec<AgentRun>, now: NaiveDateTime) -> Vec<FixSubject> {
        subjects(
            Uuid::from_u128(1),
            &[task("complete", moment(2, 9, 0), moment(2, 9, 0))],
            &runs,
            &RegressionPolicy::default(),
            now,
        )
    }

    #[test]
    fn the_addressed_failure_is_the_last_one_before_the_fix_shipped() {
        let subjects = built(
            vec![
                run(
                    1,
                    AttemptOutcome::Failed,
                    ErrorCategory::MergeConflict,
                    moment(1, 9, 0),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::TestFailure,
                    moment(2, 8, 0),
                ),
                run(
                    3,
                    AttemptOutcome::Succeeded,
                    ErrorCategory::Unknown,
                    moment(2, 9, 0),
                ),
            ],
            moment(3, 9, 0),
        );

        assert_eq!(subjects.len(), 1);
        assert_eq!(subjects[0].addressed, ErrorCategory::TestFailure);
    }

    #[test]
    fn a_fix_that_was_never_diagnosed_addresses_nothing_in_particular() {
        let subjects = built(
            vec![run(
                1,
                AttemptOutcome::Succeeded,
                ErrorCategory::Unknown,
                moment(2, 9, 0),
            )],
            moment(3, 9, 0),
        );

        assert_eq!(subjects[0].addressed, ErrorCategory::Unknown);
    }

    #[test]
    fn only_runs_after_the_fix_are_offered_to_a_checker() {
        let subjects = built(
            vec![
                run(
                    1,
                    AttemptOutcome::Failed,
                    ErrorCategory::Build,
                    moment(1, 9, 0),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::Build,
                    moment(3, 9, 0),
                ),
            ],
            moment(4, 9, 0),
        );

        assert_eq!(subjects[0].later_runs.len(), 1);
        assert_eq!(subjects[0].later_runs[0].run_id, Uuid::from_u128(2));
    }

    #[test]
    fn a_fix_past_its_watch_window_is_dropped_before_any_checker_sees_it() {
        assert!(built(Vec::new(), moment(30, 9, 0)).is_empty());
    }

    #[test]
    fn a_task_with_no_runs_at_all_is_still_a_subject_for_the_reopen_check() {
        let subjects = built(Vec::new(), moment(3, 9, 0));

        assert_eq!(subjects.len(), 1);
        assert!(subjects[0].later_runs.is_empty());
    }

    #[test]
    fn later_runs_arrive_in_the_order_they_finished() {
        let subjects = built(
            vec![
                run(
                    3,
                    AttemptOutcome::Failed,
                    ErrorCategory::Build,
                    moment(4, 9, 0),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::Build,
                    moment(3, 9, 0),
                ),
            ],
            moment(5, 9, 0),
        );

        let order: Vec<Uuid> = subjects[0]
            .later_runs
            .iter()
            .map(|run| run.run_id)
            .collect();
        assert_eq!(order, vec![Uuid::from_u128(2), Uuid::from_u128(3)]);
    }

    fn regressed(task_id: u128) -> RegressionResult {
        RegressionResult::new(
            "recurrence",
            &FixSubject {
                task_id: Uuid::from_u128(task_id),
                workspace_id: Uuid::from_u128(1),
                title: "Stop the importer dropping rows".to_string(),
                status: "complete".to_string(),
                shipped_at: moment(2, 9, 0),
                last_touched_at: moment(2, 9, 0),
                addressed: ErrorCategory::Build,
                later_runs: Vec::new(),
            },
            Verdict::Regressed,
            0.8,
            Vec::new(),
            moment(3, 9, 0),
        )
    }

    struct Unreachable;

    #[async_trait::async_trait]
    impl RegressionChecker for Unreachable {
        fn name(&self) -> &'static str {
            "unreachable"
        }

        async fn check(
            &self,
            _subject: &FixSubject,
            _now: NaiveDateTime,
        ) -> Result<RegressionResult, super::super::checker::RegressionError> {
            Err(super::super::checker::RegressionError::Unreachable {
                checker: "unreachable",
                message: "the issue tracker did not answer".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn a_checker_that_cannot_answer_does_not_lose_the_others() {
        let watched = built(
            vec![
                run(
                    1,
                    AttemptOutcome::Failed,
                    ErrorCategory::TestFailure,
                    moment(1, 9, 0),
                ),
                run(
                    2,
                    AttemptOutcome::Failed,
                    ErrorCategory::TestFailure,
                    moment(3, 9, 0),
                ),
                run(
                    3,
                    AttemptOutcome::Failed,
                    ErrorCategory::TestFailure,
                    moment(4, 9, 0),
                ),
                run(
                    4,
                    AttemptOutcome::Failed,
                    ErrorCategory::TestFailure,
                    moment(5, 9, 0),
                ),
            ],
            moment(6, 9, 0),
        );
        let checkers: Vec<Arc<dyn RegressionChecker>> = vec![
            Arc::new(Unreachable),
            Arc::new(RecurrenceChecker::new(RegressionPolicy::default())),
        ];

        let results = check_all(&checkers, &watched, moment(6, 9, 0)).await;

        assert_eq!(results.len(), 1, "the working checker still answered");
        assert_eq!(results[0].checker, "recurrence");
        assert_eq!(results[0].verdict, Verdict::Regressed);
    }

    #[tokio::test]
    async fn a_clear_verdict_is_counted_but_not_carried_forward() {
        let watched = built(Vec::new(), moment(3, 9, 0));
        let checkers: Vec<Arc<dyn RegressionChecker>> = vec![Arc::new(RecurrenceChecker::new(
            RegressionPolicy::default(),
        ))];

        assert!(
            check_all(&checkers, &watched, moment(3, 9, 0))
                .await
                .is_empty()
        );
    }

    #[test]
    fn a_fix_is_only_alerted_on_once() {
        let results = vec![regressed(7)];
        let mut alerted = BTreeMap::new();

        assert_eq!(unalerted(&results, &alerted).len(), 1);
        alerted.insert(Uuid::from_u128(7), moment(3, 9, 0));
        assert!(unalerted(&results, &alerted).is_empty());
    }

    #[test]
    fn a_suspected_result_is_never_alerted_on() {
        let mut suspected = regressed(7);
        suspected.verdict = Verdict::Suspected;

        assert!(unalerted(&[suspected], &BTreeMap::new()).is_empty());
    }

    #[test]
    fn the_ledger_forgets_fixes_once_their_window_closes() {
        let policy = RegressionPolicy::default();
        let mut alerted = BTreeMap::from([
            (Uuid::from_u128(1), moment(1, 9, 0)),
            (Uuid::from_u128(2), moment(20, 9, 0)),
        ]);

        prune(&mut alerted, &policy, moment(21, 9, 0));

        assert_eq!(alerted.len(), 1, "only the recent one survives");
        assert!(alerted.contains_key(&Uuid::from_u128(2)));
    }
}
