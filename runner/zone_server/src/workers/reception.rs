//! Keeping each run's record of how its change was received up to date.
//!
//! A pull request has no reception at the moment it is opened: nothing has been
//! reviewed, approved or merged yet. The facts the learning loop scores a change
//! on — [`crate::workers::learning::quality`] for merge speed, review rounds and
//! approvals, [`crate::workers::learning::review`] for what reviewers actually
//! said — only exist afterwards, so they are read back on a timer rather than
//! written once at creation.
//!
//! A run is revisited until its pull request merges. Until then the reception is
//! an open question, and a run whose change is still being argued over is exactly
//! the run whose score will change.

use sqlx::Row;
use uuid::Uuid;

use crate::db::DbResult;
use crate::state::AppState;
use crate::workers::pr::{ReceptionSyncResult, sync_reception};

/// How often reception is re-read.
pub const SYNC_INTERVAL_SECONDS: u64 = 30 * 60;

/// How far back a cycle looks for runs still worth revisiting.
const LOOKBACK_DAYS: i32 = 14;

/// Runs one cycle will sync, so one busy workspace cannot monopolise the GitHub
/// rate limit the rest of the deployment shares.
const RUNS_PER_CYCLE: i64 = 50;

/// A finished run whose pull request has not merged yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRun {
    pub run_id: Uuid,
    pub task_id: Uuid,
}

const PENDING_QUERY: &str = r#"
SELECT run.id AS run_id, run.task_id AS task_id
FROM task_runs run
JOIN tasks task ON task.id = run.task_id
WHERE task.pr_url IS NOT NULL
  AND run.completed_at IS NOT NULL
  AND run.completed_at > NOW() - make_interval(days => $1::int)
  AND (run.artifacts -> 'pr' ->> 'merged_at') IS NULL
ORDER BY run.completed_at ASC
LIMIT $2::bigint
"#;

/// Runs whose change has a pull request that has not been recorded as merged.
///
/// Oldest first. Newest-first with a per-cycle cap re-fetches the same recent
/// runs every sweep once more than `MAXIMUM_PER_CYCLE` are outstanding, and the
/// older ones age out of the lookback window unrecorded — leaving fix-quality
/// scoring to judge them on evidence that was never collected.
pub async fn pending(state: &AppState) -> DbResult<Vec<PendingRun>> {
    let rows = sqlx::query(PENDING_QUERY)
        .bind(LOOKBACK_DAYS)
        .bind(RUNS_PER_CYCLE)
        .fetch_all(state.db())
        .await?;

    Ok(rows
        .iter()
        .map(|row| PendingRun {
            run_id: row.get("run_id"),
            task_id: row.get("task_id"),
        })
        .collect())
}

/// Sync every pending run once, and say how many were recorded.
pub async fn run_cycle(state: &AppState) -> DbResult<usize> {
    let pending = pending(state).await?;
    let mut recorded = 0usize;

    for run in pending {
        match sync_reception(state, run.run_id, run.task_id).await {
            ReceptionSyncResult::Recorded(reception) => {
                recorded += 1;
                tracing::debug!(
                    "Recorded reception for run {}: {} review cycle(s), {} approval(s)",
                    run.run_id,
                    reception.review_cycles,
                    reception.approvals
                );
            }
            ReceptionSyncResult::NoPullRequest | ReceptionSyncResult::NoCredentials => {}
            ReceptionSyncResult::Error(reason) => {
                tracing::warn!("Reception sync failed for run {}: {}", run.run_id, reason);
            }
        }
    }

    Ok(recorded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_only_revisits_runs_whose_change_has_not_merged_yet() {
        assert!(
            PENDING_QUERY.contains("'merged_at') IS NULL"),
            "a run whose pull request already merged has a final reception and must not be re-read"
        );
        assert!(
            PENDING_QUERY.contains("task.pr_url IS NOT NULL"),
            "a run with no pull request has nothing to read a reception from"
        );
        assert!(
            PENDING_QUERY.contains("run.completed_at IS NOT NULL"),
            "a run still going has not handed anything over to be received"
        );
    }

    #[test]
    fn a_cycle_takes_a_bounded_bite_out_of_the_shared_rate_limit() {
        assert!(
            PENDING_QUERY.contains("LIMIT"),
            "an unbounded cycle would spend the whole deployment's GitHub rate limit"
        );
        assert!(
            PENDING_QUERY.contains("make_interval"),
            "an unbounded lookback would resync every run ever finished"
        );
    }
}
