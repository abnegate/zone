//! What a workspace's finished runs add up to.
//!
//! The success rate and the failure-kind distribution come from
//! [`crate::workers::learning::attempt::summarize`], which already discounts
//! observations by age and counts distinct tasks so one task retried ten times
//! cannot dominate. Recomputing either here would give the dashboard one answer
//! and the learning loop another over exactly the same runs.
//!
//! What is added on top is the shape the learning loop has no use for: how long
//! a run takes to finish, how the rate moves across the window, and where in the
//! window the failures sat.
//!
//! Every function here is pure over in-memory rows.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::period::{TimePeriod, TimeWindow};
use crate::workers::learning::attempt::{
    AttemptOutcome, ClassifiedAttempt, OutcomeStatistics, RunAttempt,
    summarize as summarize_outcomes,
};
use crate::workers::learning::error_category::{Categorization, ErrorCategory};

/// A trend is only named when each half of the window has this many runs.
const MINIMUM_TREND_SAMPLE: usize = 5;
/// ...and the two halves differ by at least this much.
const MINIMUM_TREND_CHANGE: f64 = 0.10;

/// One finished run, reduced to what the numbers need.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRun {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub outcome: AttemptOutcome,
    pub category: ErrorCategory,
    pub finished_at: NaiveDateTime,
    /// Wall time from start to finish, when both ends were recorded.
    pub duration_seconds: Option<f64>,
}

impl AgentRun {
    /// Read one loaded row, dropping runs whose status carries no outcome yet.
    ///
    /// An unrecognised failure category reads as [`ErrorCategory::Unknown`]
    /// rather than dropping the run: a run the learning loop has not classified
    /// yet still happened, and still counts against the success rate.
    pub fn from_row(row: &crate::db::analytics::RunRow) -> Option<Self> {
        let outcome = AttemptOutcome::from_status(&row.status)?;
        let duration_seconds = row
            .started_at
            .filter(|started| *started <= row.finished_at)
            .map(|started| (row.finished_at - started).num_milliseconds() as f64 / 1_000.0);

        Some(Self {
            run_id: row.run_id,
            task_id: row.task_id,
            outcome,
            category: row
                .error_category
                .as_deref()
                .and_then(ErrorCategory::parse)
                .unwrap_or(ErrorCategory::Unknown),
            finished_at: row.finished_at,
            duration_seconds,
        })
    }

    fn as_attempt(&self) -> ClassifiedAttempt {
        ClassifiedAttempt {
            attempt: RunAttempt {
                run_id: self.run_id,
                task_id: self.task_id,
                outcome: self.outcome,
                attempts: 1,
                error_message: None,
                finished_at: self.finished_at,
            },
            categorization: Categorization {
                category: self.category,
                confidence: 0.0,
                margin: 0.0,
            },
        }
    }
}

/// How long runs took to finish, over the runs that actually finished.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionTimes {
    pub measured: usize,
    pub median_seconds: f64,
    pub ninetieth_seconds: f64,
    pub mean_seconds: f64,
}

/// One bucket of the time series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub window: TimeWindow,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
}

impl SeriesPoint {
    fn empty(window: TimeWindow) -> Self {
        Self {
            window,
            total: 0,
            succeeded: 0,
            failed: 0,
            cancelled: 0,
        }
    }

    /// The share that succeeded, or `None` when the bucket saw no runs.
    ///
    /// A bucket with nothing in it is not a bucket with a zero success rate,
    /// and a chart that draws it as one invents a cliff that never happened.
    pub fn success_rate(&self) -> Option<f64> {
        (self.total > 0).then(|| self.succeeded as f64 / self.total as f64)
    }
}

/// Which way the success rate is moving, if it clearly is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    Improving,
    Steady,
    Declining,
}

impl TrendDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            TrendDirection::Improving => "improving",
            TrendDirection::Steady => "steady",
            TrendDirection::Declining => "declining",
        }
    }
}

impl std::fmt::Display for TrendDirection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The success rate in the later half of the window against the earlier half.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Trend {
    pub direction: TrendDirection,
    pub earlier_success_rate: Option<f64>,
    pub later_success_rate: Option<f64>,
    /// Later minus earlier, as a share. Zero when either half is too thin.
    pub change: f64,
}

impl Trend {
    fn steady() -> Self {
        Self {
            direction: TrendDirection::Steady,
            earlier_success_rate: None,
            later_success_rate: None,
            change: 0.0,
        }
    }
}

/// Everything one workspace's finished runs say over one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentAnalytics {
    pub period: TimePeriod,
    pub window: TimeWindow,
    pub outcomes: OutcomeStatistics,
    pub completion: CompletionTimes,
    pub series: Vec<SeriesPoint>,
    pub trend: Trend,
}

impl AgentAnalytics {
    pub fn empty(period: TimePeriod, window: TimeWindow) -> Self {
        Self {
            period,
            window,
            outcomes: OutcomeStatistics::empty(),
            completion: CompletionTimes::default(),
            series: window
                .buckets(period.bucket())
                .into_iter()
                .map(SeriesPoint::empty)
                .collect(),
            trend: Trend::steady(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.outcomes.total == 0
    }

    /// The failure kinds that showed up, heaviest first.
    pub fn failures(&self) -> impl Iterator<Item = (ErrorCategory, usize)> + '_ {
        self.outcomes
            .categories
            .iter()
            .map(|statistics| (statistics.category, statistics.occurrences))
    }
}

/// Fold finished runs into one window's numbers.
///
/// Runs outside the window are ignored rather than rejected, so a caller may
/// hand over everything it loaded and ask for any sub-window of it.
pub fn summarize(
    runs: &[AgentRun],
    period: TimePeriod,
    window: TimeWindow,
    now: NaiveDateTime,
) -> AgentAnalytics {
    let inside: Vec<&AgentRun> = runs
        .iter()
        .filter(|run| window.contains(run.finished_at))
        .collect();

    if inside.is_empty() {
        return AgentAnalytics::empty(period, window);
    }

    let attempts: Vec<ClassifiedAttempt> = inside.iter().map(|run| run.as_attempt()).collect();

    AgentAnalytics {
        period,
        window,
        outcomes: summarize_outcomes(&attempts, now),
        completion: completion_times(&inside),
        series: series(&inside, window, period),
        trend: trend(&inside, window),
    }
}

fn completion_times(runs: &[&AgentRun]) -> CompletionTimes {
    let mut durations: Vec<f64> = runs
        .iter()
        .filter(|run| run.outcome.is_success())
        .filter_map(|run| run.duration_seconds)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .collect();

    if durations.is_empty() {
        return CompletionTimes::default();
    }

    durations.sort_by(f64::total_cmp);
    let total: f64 = durations.iter().sum();

    CompletionTimes {
        measured: durations.len(),
        median_seconds: percentile(&durations, 0.5),
        ninetieth_seconds: percentile(&durations, 0.9),
        mean_seconds: total / durations.len() as f64,
    }
}

/// Nearest-rank percentile over an already sorted, non-empty slice.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    let index = rank.clamp(1, sorted.len()) - 1;
    sorted[index]
}

fn series(runs: &[&AgentRun], window: TimeWindow, period: TimePeriod) -> Vec<SeriesPoint> {
    let mut points: Vec<SeriesPoint> = window
        .buckets(period.bucket())
        .into_iter()
        .map(SeriesPoint::empty)
        .collect();

    for run in runs {
        let Some(point) = points
            .iter_mut()
            .find(|point| point.window.contains(run.finished_at))
        else {
            continue;
        };

        point.total += 1;
        match run.outcome {
            AttemptOutcome::Succeeded => point.succeeded += 1,
            AttemptOutcome::Failed => point.failed += 1,
            AttemptOutcome::Cancelled => point.cancelled += 1,
        }
    }

    points
}

fn trend(runs: &[&AgentRun], window: TimeWindow) -> Trend {
    let (earlier_window, later_window) = window.halves();
    let earlier = success_rate(runs, earlier_window);
    let later = success_rate(runs, later_window);

    let (Some((earlier_rate, earlier_count)), Some((later_rate, later_count))) = (earlier, later)
    else {
        return Trend::steady();
    };

    let change = later_rate - earlier_rate;
    let thin = earlier_count < MINIMUM_TREND_SAMPLE || later_count < MINIMUM_TREND_SAMPLE;
    let direction = if thin || change.abs() < MINIMUM_TREND_CHANGE {
        TrendDirection::Steady
    } else if change.is_sign_positive() {
        TrendDirection::Improving
    } else {
        TrendDirection::Declining
    };

    Trend {
        direction,
        earlier_success_rate: Some(earlier_rate),
        later_success_rate: Some(later_rate),
        change: if thin { 0.0 } else { change },
    }
}

fn success_rate(runs: &[&AgentRun], window: TimeWindow) -> Option<(f64, usize)> {
    let inside: Vec<&&AgentRun> = runs
        .iter()
        .filter(|run| window.contains(run.finished_at))
        .collect();

    if inside.is_empty() {
        return None;
    }

    let succeeded = inside.iter().filter(|run| run.outcome.is_success()).count();
    Some((succeeded as f64 / inside.len() as f64, inside.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn moment(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .expect("valid date")
            .and_hms_opt(hour, 0, 0)
            .expect("valid time")
    }

    fn run(index: u128, outcome: AttemptOutcome, day: u32, hour: u32) -> AgentRun {
        AgentRun {
            run_id: Uuid::from_u128(index),
            task_id: Uuid::from_u128(1_000 + index),
            outcome,
            category: if outcome.is_success() {
                ErrorCategory::Unknown
            } else {
                ErrorCategory::Build
            },
            finished_at: moment(day, hour),
            duration_seconds: Some(60.0),
        }
    }

    fn week() -> TimeWindow {
        TimeWindow::new(moment(1, 0), moment(8, 0))
    }

    #[test]
    fn an_empty_set_of_runs_still_describes_its_window() {
        let analytics = summarize(&[], TimePeriod::Week, week(), moment(8, 0));

        assert!(analytics.is_empty());
        assert_eq!(analytics.outcomes.total, 0);
        assert_eq!(analytics.outcomes.success_rate, 0.0);
        assert_eq!(analytics.completion, CompletionTimes::default());
        assert_eq!(analytics.trend.direction, TrendDirection::Steady);
        assert_eq!(
            analytics.series.len(),
            7,
            "a week of daily buckets is still seven buckets with nothing in them"
        );
        assert!(analytics.series.iter().all(|point| point.total == 0));
        assert!(
            analytics
                .series
                .iter()
                .all(|point| point.success_rate().is_none()),
            "an empty bucket has no rate, not a zero one"
        );
    }

    #[test]
    fn a_single_run_does_not_divide_by_zero_anywhere() {
        let runs = vec![run(1, AttemptOutcome::Succeeded, 4, 12)];
        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.outcomes.total, 1);
        assert_eq!(analytics.outcomes.succeeded, 1);
        assert_eq!(analytics.outcomes.success_rate, 1.0);
        assert_eq!(analytics.completion.measured, 1);
        assert_eq!(analytics.completion.median_seconds, 60.0);
        assert_eq!(analytics.completion.ninetieth_seconds, 60.0);
        assert_eq!(analytics.completion.mean_seconds, 60.0);
        assert_eq!(
            analytics.trend.direction,
            TrendDirection::Steady,
            "one run cannot establish a direction"
        );
    }

    #[test]
    fn runs_outside_the_window_are_left_out_of_every_number() {
        let runs = vec![
            run(1, AttemptOutcome::Succeeded, 4, 12),
            run(2, AttemptOutcome::Failed, 20, 12),
        ];
        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.outcomes.total, 1);
        assert_eq!(analytics.outcomes.failed, 0);
    }

    #[test]
    fn a_run_on_a_bucket_boundary_is_counted_once_in_the_later_bucket() {
        let runs = vec![run(1, AttemptOutcome::Succeeded, 4, 0)];
        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        let occupied: Vec<usize> = analytics
            .series
            .iter()
            .enumerate()
            .filter(|(_, point)| point.total > 0)
            .map(|(index, _)| index)
            .collect();

        assert_eq!(occupied, vec![3], "the fourth day, exactly once");
        assert_eq!(
            analytics
                .series
                .iter()
                .map(|point| point.total)
                .sum::<usize>(),
            1
        );
        assert_eq!(analytics.series[3].window.start, moment(4, 0));
    }

    #[test]
    fn the_series_sums_back_to_the_totals() {
        let runs = vec![
            run(1, AttemptOutcome::Succeeded, 1, 9),
            run(2, AttemptOutcome::Failed, 3, 9),
            run(3, AttemptOutcome::Cancelled, 5, 9),
            run(4, AttemptOutcome::Succeeded, 7, 23),
        ];
        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        let totals: usize = analytics.series.iter().map(|point| point.total).sum();
        let succeeded: usize = analytics.series.iter().map(|point| point.succeeded).sum();
        let failed: usize = analytics.series.iter().map(|point| point.failed).sum();
        let cancelled: usize = analytics.series.iter().map(|point| point.cancelled).sum();

        assert_eq!(totals, analytics.outcomes.total);
        assert_eq!(succeeded, analytics.outcomes.succeeded);
        assert_eq!(failed, analytics.outcomes.failed);
        assert_eq!(cancelled, analytics.outcomes.cancelled);
    }

    #[test]
    fn failures_are_grouped_by_kind_and_ordered_by_weight() {
        let mut runs = Vec::new();
        for index in 0..4 {
            runs.push(AgentRun {
                category: ErrorCategory::TestFailure,
                ..run(index, AttemptOutcome::Failed, 6, 9)
            });
        }
        runs.push(AgentRun {
            category: ErrorCategory::Network,
            ..run(10, AttemptOutcome::Failed, 6, 9)
        });

        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));
        let grouped: Vec<(ErrorCategory, usize)> = analytics.failures().collect();

        assert_eq!(
            grouped,
            vec![(ErrorCategory::TestFailure, 4), (ErrorCategory::Network, 1),]
        );
    }

    #[test]
    fn one_task_retried_many_times_counts_once_in_distinct_tasks() {
        let runs: Vec<AgentRun> = (0..5)
            .map(|index| AgentRun {
                task_id: Uuid::from_u128(77),
                category: ErrorCategory::Build,
                ..run(index, AttemptOutcome::Failed, 6, 9)
            })
            .collect();

        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));
        let build = analytics
            .outcomes
            .categories
            .iter()
            .find(|statistics| statistics.category == ErrorCategory::Build)
            .expect("build failures were recorded");

        assert_eq!(build.occurrences, 5);
        assert_eq!(build.distinct_tasks, 1);
    }

    #[test]
    fn successes_that_never_finished_are_not_counted_as_instant() {
        let runs = vec![
            AgentRun {
                duration_seconds: None,
                ..run(1, AttemptOutcome::Succeeded, 2, 9)
            },
            AgentRun {
                duration_seconds: Some(120.0),
                ..run(2, AttemptOutcome::Succeeded, 3, 9)
            },
        ];
        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.completion.measured, 1);
        assert_eq!(analytics.completion.mean_seconds, 120.0);
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let sorted = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];

        assert_eq!(percentile(&sorted, 0.5), 50.0);
        assert_eq!(percentile(&sorted, 0.9), 90.0);
        assert_eq!(
            percentile(&sorted, 0.0),
            10.0,
            "clamped to the first element"
        );
        assert_eq!(percentile(&sorted, 1.0), 100.0);
    }

    #[test]
    fn a_collapse_across_the_halves_reads_as_declining() {
        let mut runs: Vec<AgentRun> = (0..6)
            .map(|index| run(index, AttemptOutcome::Succeeded, 2, 9))
            .collect();
        runs.extend((0..6).map(|index| run(100 + index, AttemptOutcome::Failed, 6, 9)));

        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.trend.direction, TrendDirection::Declining);
        assert_eq!(analytics.trend.earlier_success_rate, Some(1.0));
        assert_eq!(analytics.trend.later_success_rate, Some(0.0));
        assert_eq!(analytics.trend.change, -1.0);
    }

    #[test]
    fn a_recovery_across_the_halves_reads_as_improving() {
        let mut runs: Vec<AgentRun> = (0..6)
            .map(|index| run(index, AttemptOutcome::Failed, 2, 9))
            .collect();
        runs.extend((0..6).map(|index| run(100 + index, AttemptOutcome::Succeeded, 6, 9)));

        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.trend.direction, TrendDirection::Improving);
        assert_eq!(analytics.trend.change, 1.0);
    }

    #[test]
    fn too_few_runs_on_one_side_is_not_a_trend() {
        let mut runs: Vec<AgentRun> = (0..6)
            .map(|index| run(index, AttemptOutcome::Succeeded, 2, 9))
            .collect();
        runs.push(run(100, AttemptOutcome::Failed, 6, 9));

        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(
            analytics.trend.direction,
            TrendDirection::Steady,
            "one failure against six successes is not a decline"
        );
        assert_eq!(analytics.trend.change, 0.0);
    }

    #[test]
    fn a_small_wobble_with_enough_runs_is_still_steady() {
        let mut runs: Vec<AgentRun> = (0..10)
            .map(|index| run(index, AttemptOutcome::Succeeded, 2, 9))
            .collect();
        runs.extend((0..9).map(|index| run(100 + index, AttemptOutcome::Succeeded, 6, 9)));
        runs.push(run(200, AttemptOutcome::Failed, 6, 9));

        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.trend.direction, TrendDirection::Steady);
        assert!(
            analytics.trend.change.abs() < MINIMUM_TREND_CHANGE,
            "a ten-point move is the bar, and this is under it"
        );
    }

    fn row(status: &str, category: Option<&str>) -> crate::db::analytics::RunRow {
        crate::db::analytics::RunRow {
            run_id: Uuid::from_u128(1),
            task_id: Uuid::from_u128(2),
            status: status.to_string(),
            error_message: None,
            error_category: category.map(str::to_string),
            started_at: Some(moment(4, 9)),
            finished_at: moment(4, 10),
        }
    }

    #[test]
    fn a_loaded_row_carries_its_duration_and_its_category() {
        let parsed = AgentRun::from_row(&row("failed", Some("test_failure"))).expect("an outcome");

        assert_eq!(parsed.outcome, AttemptOutcome::Failed);
        assert_eq!(parsed.category, ErrorCategory::TestFailure);
        assert_eq!(parsed.duration_seconds, Some(3_600.0));
    }

    #[test]
    fn a_row_the_learning_loop_has_not_reached_still_counts() {
        let unclassified = AgentRun::from_row(&row("failed", None)).expect("an outcome");
        let unrecognised =
            AgentRun::from_row(&row("failed", Some("gremlins"))).expect("an outcome");

        assert_eq!(unclassified.category, ErrorCategory::Unknown);
        assert_eq!(unrecognised.category, ErrorCategory::Unknown);
    }

    #[test]
    fn a_row_with_no_outcome_yet_is_dropped() {
        assert_eq!(AgentRun::from_row(&row("running", None)), None);
    }

    #[test]
    fn a_run_that_finished_before_it_started_has_no_duration() {
        let backwards = crate::db::analytics::RunRow {
            started_at: Some(moment(5, 9)),
            finished_at: moment(4, 9),
            ..row("completed", None)
        };

        assert_eq!(
            AgentRun::from_row(&backwards).and_then(|run| run.duration_seconds),
            None
        );
    }

    #[test]
    fn recent_runs_weigh_more_than_old_ones_in_the_recent_rate() {
        let runs = vec![
            run(1, AttemptOutcome::Failed, 1, 9),
            run(2, AttemptOutcome::Succeeded, 7, 9),
        ];
        let analytics = summarize(&runs, TimePeriod::Week, week(), moment(8, 0));

        assert_eq!(analytics.outcomes.success_rate, 0.5);
        assert!(
            analytics.outcomes.recent_success_rate > 0.5,
            "the recent success should outweigh the old failure, got {}",
            analytics.outcomes.recent_success_rate
        );
    }
}
