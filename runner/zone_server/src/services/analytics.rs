//! A workspace's analytics overview: how its runs went, broken down by source
//! and model, alongside its tasks, pull requests and evaluations.
//!
//! Loading happens in [`overview`]; every type below assembles its numbers
//! from already loaded rows, so each can be exercised without a database.

mod evaluation_summary;
mod model_breakdown;
mod overview;
mod pull_request_summary;
mod source_breakdown;
mod tally;
mod task_summary;

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::analytics::{self, AttributedRunRow};
use crate::workers::analytics::{AgentRun, TimePeriod, summarize};
use crate::workers::evaluation;

pub use evaluation_summary::EvaluationSummary;
pub use model_breakdown::ModelBreakdown;
pub use overview::Overview;
pub use pull_request_summary::PullRequestSummary;
pub use source_breakdown::SourceBreakdown;
pub use tally::Tally;
pub use task_summary::TaskSummary;

pub const MAXIMUM_RUNS: i64 = 10_000;

/// The workspace's analytics over the `period` ending at `now`.
pub async fn overview(
    pool: &PgPool,
    workspace_id: Uuid,
    period: TimePeriod,
    now: NaiveDateTime,
) -> DbResult<Overview> {
    let window = period.window_ending(now);
    let (start, end) = (window.start, window.end);

    let (rows, pull_requests, statuses, origins, activity, verdicts) = tokio::try_join!(
        analytics::attributed_runs(pool, workspace_id, start, end, MAXIMUM_RUNS),
        analytics::pull_requests(pool, workspace_id, start, end),
        analytics::task_statuses(pool, workspace_id),
        analytics::task_origins(pool, workspace_id, start, end),
        analytics::task_activity(pool, workspace_id, start, end),
        analytics::evaluation_verdicts(
            pool,
            workspace_id,
            start,
            end,
            evaluation::PHASE,
            evaluation::AGENT,
        ),
    )?;

    let truncated = rows.len() as i64 >= MAXIMUM_RUNS;
    let attributed: Vec<(AgentRun, &AttributedRunRow)> = rows
        .iter()
        .filter_map(|row| AgentRun::from_row(&row.run).map(|run| (run, row)))
        .collect();
    let runs: Vec<AgentRun> = attributed.iter().map(|(run, _)| run.clone()).collect();

    Ok(Overview {
        period,
        window,
        truncated,
        runs: summarize(&runs, period, window, now),
        sources: SourceBreakdown::group(&attributed, &pull_requests, period, window, now),
        models: ModelBreakdown::group(&attributed, period, window, now),
        tasks: TaskSummary::new(&statuses, &origins, activity),
        pull_requests: PullRequestSummary::new(&pull_requests),
        evaluations: EvaluationSummary::new(&verdicts),
    })
}
