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

    let mut snapshot = analytics::snapshot(pool).await?;
    let rows =
        analytics::attributed_runs(&mut *snapshot, workspace_id, start, end, MAXIMUM_RUNS).await?;
    let pull_requests = analytics::pull_requests(&mut *snapshot, workspace_id, start, end).await?;
    let statuses = analytics::task_statuses(&mut *snapshot, workspace_id).await?;
    let origins = analytics::task_origins(&mut *snapshot, workspace_id, start, end).await?;
    let activity = analytics::task_activity(&mut *snapshot, workspace_id, start, end).await?;
    let verdicts = analytics::evaluation_verdicts(
        &mut *snapshot,
        workspace_id,
        start,
        end,
        evaluation::PHASE,
        evaluation::AGENT,
    )
    .await?;
    snapshot.commit().await?;

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
