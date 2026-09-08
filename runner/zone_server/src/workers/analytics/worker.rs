//! The pass that publishes what the agent has been doing.
//!
//! `metrics.rs` already says how the process is holding up — requests, queries,
//! cache hits. None of that says whether the agent is any good at its job. This
//! pass answers that: how often a run succeeds, what it fails on, and how long
//! it takes, per workspace, on a short interval.
//!
//! Prometheus is itself a time series, so what is published here is the current
//! window's scalars rather than the series [`summary`](super::summary) builds.
//! The series is for a digest, which has to carry its own history because it
//! arrives as one message.

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use super::period::{TimePeriod, TimeWindow};
use super::summary::{AgentAnalytics, AgentRun, summarize};
use crate::db::DbResult;
use crate::db::analytics::{self, ReportableWorkspace};
use crate::metrics::{AgentSnapshot, FailureCount};
use crate::state::AppState;
use crate::workers::learning::error_category::ErrorCategory;

pub const ANALYTICS_INTERVAL_SECONDS: u64 = 15 * 60;
const MAXIMUM_RUNS_PER_WORKSPACE: i64 = 5_000;

/// What the pass covers each time it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsPolicy {
    /// The windows published to Prometheus, one label value each.
    pub periods: Vec<TimePeriod>,
    pub maximum_runs_per_workspace: i64,
}

impl Default for AnalyticsPolicy {
    fn default() -> Self {
        Self {
            periods: vec![TimePeriod::Day, TimePeriod::Week],
            maximum_runs_per_workspace: MAXIMUM_RUNS_PER_WORKSPACE,
        }
    }
}

impl AnalyticsPolicy {
    /// How far back any of the configured periods reaches.
    pub fn lookback_days(&self) -> i64 {
        self.periods
            .iter()
            .map(|period| period.days())
            .max()
            .unwrap_or(0)
    }
}

/// Load one workspace's finished runs over a window.
pub async fn load_runs(
    pool: &PgPool,
    workspace_id: Uuid,
    window: TimeWindow,
    limit: i64,
) -> DbResult<Vec<AgentRun>> {
    let rows =
        analytics::finished_runs(pool, workspace_id, window.start, window.end, limit).await?;

    Ok(rows.iter().filter_map(AgentRun::from_row).collect())
}

/// Every category with its count, including the ones the window never saw.
///
/// A gauge that goes unset holds its last value, so a failure mode that has
/// stopped happening would keep reading as if it still were.
fn failure_counts(analytics: &AgentAnalytics) -> Vec<FailureCount<'static>> {
    ErrorCategory::CLASSIFIED
        .iter()
        .copied()
        .chain(std::iter::once(ErrorCategory::Unknown))
        .map(|category| FailureCount {
            category: category.as_str(),
            count: analytics
                .outcomes
                .categories
                .iter()
                .find(|statistics| statistics.category == category)
                .map(|statistics| statistics.occurrences)
                .unwrap_or_default(),
        })
        .collect()
}

fn publish(workspace: &ReportableWorkspace, analytics: &AgentAnalytics) {
    let workspace_id = workspace.workspace_id.to_string();
    let failures = failure_counts(analytics);

    crate::metrics::record_agent(AgentSnapshot {
        workspace: &workspace_id,
        period: analytics.period.as_str(),
        runs: analytics.outcomes.total,
        success_rate: analytics.outcomes.success_rate,
        recent_success_rate: analytics.outcomes.recent_success_rate,
        success_rate_change: analytics.trend.change,
        median_completion_seconds: analytics.completion.median_seconds,
        ninetieth_completion_seconds: analytics.completion.ninetieth_seconds,
        failures: &failures,
    });
}

async fn refresh_workspace(
    pool: &PgPool,
    workspace: &ReportableWorkspace,
    policy: &AnalyticsPolicy,
) -> DbResult<()> {
    let now = Utc::now().naive_utc();
    let window = TimeWindow::new(now - chrono::Duration::days(policy.lookback_days()), now);
    let runs = load_runs(
        pool,
        workspace.workspace_id,
        window,
        policy.maximum_runs_per_workspace,
    )
    .await?;

    for period in &policy.periods {
        let analytics = summarize(&runs, *period, period.window_ending(now), now);
        publish(workspace, &analytics);
    }

    Ok(())
}

/// Refresh every workspace's agent gauges once.
pub async fn run_cycle(state: &AppState, policy: &AnalyticsPolicy) -> DbResult<()> {
    let pool = state.db();
    let since = Utc::now().naive_utc() - chrono::Duration::days(policy.lookback_days());

    for workspace in analytics::reportable_workspaces(pool, since).await? {
        if let Err(error) = refresh_workspace(pool, &workspace, policy).await {
            tracing::warn!(
                workspace_id = %workspace.workspace_id,
                %error,
                "Agent analytics refresh failed; retrying next cycle"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lookback_covers_the_longest_configured_period() {
        let policy = AnalyticsPolicy::default();
        assert_eq!(policy.lookback_days(), TimePeriod::Week.days());

        let quarterly = AnalyticsPolicy {
            periods: vec![TimePeriod::Day, TimePeriod::Quarter],
            ..AnalyticsPolicy::default()
        };
        assert_eq!(quarterly.lookback_days(), TimePeriod::Quarter.days());
    }

    #[test]
    fn every_category_is_published_even_when_it_never_came_up() {
        let window = TimePeriod::Week.window_ending(
            chrono::NaiveDate::from_ymd_opt(2026, 9, 8)
                .expect("valid date")
                .and_hms_opt(0, 0, 0)
                .expect("valid time"),
        );
        let counts = failure_counts(&AgentAnalytics::empty(TimePeriod::Week, window));

        assert_eq!(counts.len(), ErrorCategory::CLASSIFIED.len() + 1);
        assert!(counts.iter().all(|failure| failure.count == 0));
        assert!(
            counts
                .iter()
                .any(|failure| failure.category == ErrorCategory::Unknown.as_str()),
            "unknown is published alongside the classified kinds"
        );
    }

    #[test]
    fn a_policy_with_no_periods_asks_for_no_history() {
        let policy = AnalyticsPolicy {
            periods: Vec::new(),
            ..AnalyticsPolicy::default()
        };
        assert_eq!(policy.lookback_days(), 0);
    }
}
