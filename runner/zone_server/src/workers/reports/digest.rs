//! What a scheduled report says.
//!
//! Rendering is a pure function of the numbers and nothing else — no clock, no
//! map iteration order, no locale — so the same window always produces the same
//! text and a diff between two digests is a diff between two weeks rather than
//! between two runs of the same code.
//!
//! Both regression verdicts appear here, which is the point of having two. A
//! [`Verdict::Suspected`] result has not earned an interruption, but a weekly
//! summary is exactly where something that looks slightly wrong belongs.

use std::fmt::Write;

use chrono::NaiveDateTime;
use zone_notify::{Notification, Severity};

use crate::workers::analytics::{
    AgentAnalytics, SeriesPoint, TimePeriod, TimeWindow, TrendDirection,
};
use crate::workers::regression::{RegressionResult, Verdict};

/// Below this many runs, a low success rate is a small sample, not an alarm.
const MINIMUM_RUNS_FOR_ALARM: usize = 10;
/// A success rate under this, with enough runs behind it, is worth flagging.
const POOR_SUCCESS_RATE: f64 = 0.5;
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M";
const DATE_FORMAT: &str = "%Y-%m-%d";

/// One workspace's report for one window.
#[derive(Debug, Clone, PartialEq)]
pub struct Digest {
    pub workspace: String,
    pub period: TimePeriod,
    pub window: TimeWindow,
    pub analytics: AgentAnalytics,
    /// Everything the regression checkers found, loud and quiet alike.
    pub regressions: Vec<RegressionResult>,
    /// Slots that passed undelivered and were folded into this digest.
    pub missed_slots: usize,
}

impl Digest {
    /// The regressions that were also worth interrupting someone for.
    pub fn confirmed(&self) -> Vec<&RegressionResult> {
        self.ranked(Verdict::Regressed)
    }

    /// The regressions that were not.
    pub fn suspected(&self) -> Vec<&RegressionResult> {
        self.ranked(Verdict::Suspected)
    }

    /// One verdict's results, best-evidenced first.
    ///
    /// A digest carrying several regressions should lead with the one backed by
    /// the most observations, and the title breaks ties so the order does not
    /// depend on how the rows came back.
    fn ranked(&self, verdict: Verdict) -> Vec<&RegressionResult> {
        let mut matching: Vec<&RegressionResult> = self
            .regressions
            .iter()
            .filter(|result| result.verdict == verdict)
            .collect();
        matching.sort_by(|left, right| {
            right
                .weight()
                .cmp(&left.weight())
                .then_with(|| left.title.cmp(&right.title))
        });
        matching
    }

    /// How loudly this digest should arrive.
    pub fn severity(&self) -> Severity {
        let outcomes = &self.analytics.outcomes;

        if !self.confirmed().is_empty() {
            return Severity::Error;
        }

        let poor =
            outcomes.total >= MINIMUM_RUNS_FOR_ALARM && outcomes.success_rate < POOR_SUCCESS_RATE;
        match self.analytics.trend.direction {
            _ if poor => Severity::Warning,
            TrendDirection::Declining => Severity::Warning,
            TrendDirection::Improving => Severity::Success,
            TrendDirection::Steady => Severity::Info,
        }
    }

    pub fn headline(&self) -> String {
        format!(
            "{} {} report: {} of {} runs succeeded",
            self.workspace,
            self.period,
            self.analytics.outcomes.succeeded,
            self.analytics.outcomes.total
        )
    }

    /// The whole report, as it reads in a message.
    pub fn body(&self) -> String {
        let mut body = String::new();
        let _ = writeln!(
            body,
            "{} to {}",
            self.window.start.format(TIMESTAMP_FORMAT),
            self.window.end.format(TIMESTAMP_FORMAT)
        );

        if self.missed_slots > 0 {
            let _ = writeln!(
                body,
                "Covers {} slot(s) that went by undelivered.",
                self.missed_slots
            );
        }

        if self.analytics.is_empty() {
            let _ = write!(body, "\nNo runs finished in this window.");
            return body;
        }

        self.write_runs(&mut body);
        self.write_completion(&mut body);
        self.write_failures(&mut body);
        self.write_busiest(&mut body);
        self.write_regressions(&mut body);

        body.trim_end().to_string()
    }

    fn write_runs(&self, body: &mut String) {
        let outcomes = &self.analytics.outcomes;
        let _ = writeln!(body, "\nRuns");
        let _ = writeln!(
            body,
            "  {} finished: {} succeeded, {} failed, {} cancelled",
            outcomes.total, outcomes.succeeded, outcomes.failed, outcomes.cancelled
        );
        let _ = writeln!(
            body,
            "  Success rate {} ({} weighted towards recent runs)",
            share(outcomes.success_rate),
            share(outcomes.recent_success_rate)
        );

        let trend = &self.analytics.trend;
        let _ = match (trend.earlier_success_rate, trend.later_success_rate) {
            (Some(earlier), Some(later)) => writeln!(
                body,
                "  Trend {}: {} in the first half, {} in the second",
                trend.direction,
                share(earlier),
                share(later)
            ),
            _ => writeln!(body, "  Trend {}: too little to compare", trend.direction),
        };
    }

    fn write_completion(&self, body: &mut String) {
        let completion = &self.analytics.completion;
        if completion.measured == 0 {
            return;
        }

        let _ = writeln!(body, "\nTime to finish");
        let _ = writeln!(
            body,
            "  Median {}, 90th percentile {}, over {} runs",
            humanize(completion.median_seconds),
            humanize(completion.ninetieth_seconds),
            completion.measured
        );
    }

    fn write_failures(&self, body: &mut String) {
        let categories = &self.analytics.outcomes.categories;
        if categories.is_empty() {
            return;
        }

        let _ = writeln!(body, "\nFailures");
        for statistics in categories {
            let _ = writeln!(
                body,
                "  {}: {} across {} task(s), last seen {}",
                statistics.category,
                statistics.occurrences,
                statistics.distinct_tasks,
                statistics.last_seen.format(TIMESTAMP_FORMAT)
            );
        }
    }

    fn write_busiest(&self, body: &mut String) {
        let Some(busiest) = busiest_bucket(&self.analytics.series) else {
            return;
        };

        let _ = writeln!(body, "\nBusiest {}", self.analytics.period.bucket());
        let _ = writeln!(
            body,
            "  {}: {} runs, {} failed",
            busiest.window.start.format(DATE_FORMAT),
            busiest.total,
            busiest.failed
        );
    }

    fn write_regressions(&self, body: &mut String) {
        let confirmed = self.confirmed();
        let suspected = self.suspected();

        if !confirmed.is_empty() {
            let _ = writeln!(body, "\nRegressions");
            for result in confirmed {
                write_regression(body, result);
            }
        }

        if !suspected.is_empty() {
            let _ = writeln!(body, "\nWorth a look");
            for result in suspected {
                write_regression(body, result);
            }
        }
    }

    /// The digest as the message that goes out.
    ///
    /// Workspace names, task titles and error text are all workspace data, and
    /// [`Notification::new`] sanitizes the title and body it is handed. Nothing
    /// here becomes a link, which is the one part a notification does not
    /// redact.
    pub fn to_notification(&self) -> Notification {
        Notification::new(self.headline(), self.body())
            .severity(self.severity())
            .field("Workspace", self.workspace.clone())
            .field("Period", self.period.to_string())
            .field("Runs", self.analytics.outcomes.total.to_string())
            .at(self.window.end.and_utc())
    }
}

fn write_regression(body: &mut String, result: &RegressionResult) {
    let _ = writeln!(
        body,
        "  {} ({}, {} confidence)",
        result.title,
        result.checker,
        share(result.confidence)
    );
    for evidence in &result.evidence {
        let _ = writeln!(body, "    {}", evidence.observation);
    }
}

/// The bucket that saw the most runs, ties going to the earliest.
fn busiest_bucket(series: &[SeriesPoint]) -> Option<&SeriesPoint> {
    series
        .iter()
        .filter(|point| point.total > 0)
        .max_by_key(|point| (point.total, std::cmp::Reverse(point.window.start)))
}

fn share(value: f64) -> String {
    format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0)
}

/// Seconds as something a person reads without counting zeros.
fn humanize(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (hours, minutes, seconds) = (total / 3_600, (total % 3_600) / 60, total % 60);

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Build a digest for one window.
pub fn generate(
    workspace: impl Into<String>,
    period: TimePeriod,
    window: TimeWindow,
    runs: &[crate::workers::analytics::AgentRun],
    regressions: Vec<RegressionResult>,
    missed_slots: usize,
    now: NaiveDateTime,
) -> Digest {
    Digest {
        workspace: workspace.into(),
        period,
        window,
        analytics: crate::workers::analytics::summarize(runs, period, window, now),
        regressions,
        missed_slots,
    }
}

#[cfg(test)]
pub(super) mod fixtures {
    use super::*;
    use crate::workers::analytics::AgentRun;
    use crate::workers::learning::attempt::AttemptOutcome;
    use crate::workers::learning::error_category::ErrorCategory;
    use crate::workers::regression::{Evidence, FixSubject};
    use chrono::NaiveDate;
    use uuid::Uuid;

    pub fn moment(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .expect("valid date")
            .and_hms_opt(hour, 0, 0)
            .expect("valid time")
    }

    pub fn window() -> TimeWindow {
        TimeWindow::new(moment(1, 8), moment(8, 8))
    }

    /// A fixed set of runs, so the rendered digest can be asserted exactly.
    pub fn runs() -> Vec<AgentRun> {
        let mut runs = Vec::new();

        for index in 0..6u128 {
            runs.push(AgentRun {
                run_id: Uuid::from_u128(index),
                task_id: Uuid::from_u128(100 + index),
                outcome: AttemptOutcome::Succeeded,
                category: ErrorCategory::Unknown,
                finished_at: moment(2, 9 + index as u32),
                duration_seconds: Some(120.0 * (index as f64 + 1.0)),
            });
        }

        for index in 0..4u128 {
            runs.push(AgentRun {
                run_id: Uuid::from_u128(10 + index),
                task_id: Uuid::from_u128(200 + index),
                outcome: AttemptOutcome::Failed,
                category: ErrorCategory::TestFailure,
                finished_at: moment(6, 9 + index as u32),
                duration_seconds: Some(45.0),
            });
        }

        runs.push(AgentRun {
            run_id: Uuid::from_u128(20),
            task_id: Uuid::from_u128(300),
            outcome: AttemptOutcome::Succeeded,
            category: ErrorCategory::Unknown,
            finished_at: moment(6, 14),
            duration_seconds: Some(600.0),
        });
        runs.push(AgentRun {
            run_id: Uuid::from_u128(21),
            task_id: Uuid::from_u128(301),
            outcome: AttemptOutcome::Cancelled,
            category: ErrorCategory::Unknown,
            finished_at: moment(6, 15),
            duration_seconds: None,
        });

        runs
    }

    pub fn regression(verdict: Verdict) -> RegressionResult {
        backed(
            verdict,
            "Stop the importer dropping rows",
            "3 of 3 runs since the fix failed on test_failure again",
            3,
        )
    }

    pub fn backed(
        verdict: Verdict,
        title: &str,
        observation: &str,
        occurrences: usize,
    ) -> RegressionResult {
        RegressionResult::new(
            "recurrence",
            &FixSubject {
                task_id: Uuid::from_u128(400),
                workspace_id: Uuid::from_u128(1),
                title: title.to_string(),
                status: "complete".to_string(),
                shipped_at: moment(3, 8),
                last_touched_at: moment(3, 8),
                addressed: ErrorCategory::TestFailure,
                later_runs: Vec::new(),
            },
            verdict,
            1.0,
            vec![Evidence::new(observation, occurrences)],
            moment(8, 8),
        )
    }

    pub fn digest(regressions: Vec<RegressionResult>, missed_slots: usize) -> Digest {
        generate(
            "Platform",
            TimePeriod::Week,
            window(),
            &runs(),
            regressions,
            missed_slots,
            moment(8, 8),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{backed, digest, moment, regression, window};
    use super::*;
    use crate::workers::analytics::AgentAnalytics;

    #[test]
    fn a_known_dataset_renders_the_same_report_every_time() {
        let rendered = digest(Vec::new(), 0).body();

        assert_eq!(
            rendered,
            "\
2026-09-01 08:00 to 2026-09-08 08:00

Runs
  12 finished: 7 succeeded, 4 failed, 1 cancelled
  Success rate 58% (54% weighted towards recent runs)
  Trend declining: 100% in the first half, 17% in the second

Time to finish
  Median 8m 0s, 90th percentile 12m 0s, over 7 runs

Failures
  test_failure: 4 across 4 task(s), last seen 2026-09-06 12:00
  unknown: 1 across 1 task(s), last seen 2026-09-06 15:00

Busiest day
  2026-09-02: 6 runs, 0 failed"
        );
    }

    #[test]
    fn rendering_is_stable_across_repeated_calls() {
        let digest = digest(Vec::new(), 0);

        assert_eq!(digest.body(), digest.body());
        assert_eq!(digest.headline(), digest.headline());
    }

    #[test]
    fn the_headline_names_the_workspace_and_the_score() {
        assert_eq!(
            digest(Vec::new(), 0).headline(),
            "Platform week report: 7 of 12 runs succeeded"
        );
    }

    #[test]
    fn an_empty_window_says_so_rather_than_rendering_zeros() {
        let empty = Digest {
            workspace: "Platform".to_string(),
            period: TimePeriod::Week,
            window: window(),
            analytics: AgentAnalytics::empty(TimePeriod::Week, window()),
            regressions: Vec::new(),
            missed_slots: 0,
        };

        assert_eq!(
            empty.body(),
            "2026-09-01 08:00 to 2026-09-08 08:00\n\nNo runs finished in this window."
        );
        assert_eq!(empty.severity(), Severity::Info);
    }

    #[test]
    fn a_folded_in_missed_slot_is_stated_rather_than_hidden() {
        let body = digest(Vec::new(), 3).body();

        assert!(
            body.contains("Covers 3 slot(s) that went by undelivered."),
            "got {body}"
        );
    }

    #[test]
    fn a_confirmed_regression_makes_the_digest_loud() {
        let digest = digest(vec![regression(Verdict::Regressed)], 0);

        assert_eq!(digest.severity(), Severity::Error);
        assert_eq!(digest.confirmed().len(), 1);
        assert!(
            digest.body().contains(
                "\nRegressions\n  Stop the importer dropping rows (recurrence, 100% confidence)"
            ),
            "got {}",
            digest.body()
        );
    }

    #[test]
    fn a_suspected_regression_appears_without_making_the_digest_loud() {
        let digest = digest(vec![regression(Verdict::Suspected)], 0);

        assert_eq!(
            digest.severity(),
            Severity::Warning,
            "warning comes from the declining trend, not from the suspicion"
        );
        assert!(digest.confirmed().is_empty());
        assert_eq!(digest.suspected().len(), 1);
        assert!(
            digest.body().contains("\nWorth a look\n"),
            "got {}",
            digest.body()
        );
    }

    #[test]
    fn several_regressions_lead_with_the_best_evidenced_one() {
        let digest = digest(
            vec![
                backed(Verdict::Regressed, "Thin evidence", "1 of 2 runs", 1),
                backed(Verdict::Regressed, "Strong evidence", "9 of 9 runs", 9),
                backed(Verdict::Regressed, "Also thin", "1 of 2 runs", 1),
            ],
            0,
        );

        let order: Vec<&str> = digest
            .confirmed()
            .iter()
            .map(|result| result.title.as_str())
            .collect();

        assert_eq!(
            order,
            vec!["Strong evidence", "Also thin", "Thin evidence"],
            "weight leads, and the title settles a tie so the order never wanders"
        );
    }

    #[test]
    fn a_clear_result_is_neither_reported_nor_counted() {
        let digest = digest(vec![regression(Verdict::Clear)], 0);

        assert!(digest.confirmed().is_empty());
        assert!(digest.suspected().is_empty());
        assert!(!digest.body().contains("Worth a look"));
    }

    #[test]
    fn the_notification_carries_the_window_end_rather_than_the_wall_clock() {
        let notification = digest(Vec::new(), 0).to_notification();

        assert_eq!(notification.timestamp(), moment(8, 8).and_utc());
        assert_eq!(notification.kind(), Severity::Warning);
        assert_eq!(notification.body(), digest(Vec::new(), 0).body());
        assert!(
            notification.url().is_none(),
            "nothing untrusted becomes a link"
        );
    }

    #[test]
    fn durations_read_as_time_rather_than_a_pile_of_seconds() {
        assert_eq!(humanize(0.0), "0s");
        assert_eq!(humanize(-5.0), "0s");
        assert_eq!(humanize(45.4), "45s");
        assert_eq!(humanize(250.0), "4m 10s");
        assert_eq!(humanize(3_600.0), "1h 0m");
        assert_eq!(humanize(4_355.0), "1h 12m");
    }

    #[test]
    fn a_share_is_rounded_and_kept_inside_its_range() {
        assert_eq!(share(0.0), "0%");
        assert_eq!(share(0.5), "50%");
        assert_eq!(share(2.0 / 3.0), "67%");
        assert_eq!(share(1.0), "100%");
        assert_eq!(share(1.4), "100%");
    }

    #[test]
    fn the_busiest_bucket_breaks_ties_towards_the_earlier_one() {
        let digest = digest(Vec::new(), 0);
        let busiest = busiest_bucket(&digest.analytics.series).expect("some bucket had runs");

        assert_eq!(
            busiest.window.start,
            moment(2, 0),
            "the 2nd and the 6th both saw six runs, and the earlier one wins"
        );
        assert_eq!(busiest.total, 6);
    }

    #[test]
    fn nothing_is_named_busiest_when_nothing_ran() {
        let empty = AgentAnalytics::empty(TimePeriod::Week, window());

        assert!(busiest_bucket(&empty.series).is_none());
    }
}
