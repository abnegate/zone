//! Every row the analytics, regression and report passes read.
//!
//! Loading lives here and deciding lives outside it, matching the learning loop:
//! an aggregate, a regression bar and a digest are all pure functions over the
//! rows below, so each can be exercised without a database.
//!
//! The failure category is read back from the artifact the learning loop wrote
//! rather than recomputed. Two independent classifications of the same error
//! would eventually disagree, and a digest that contradicts the lessons drawn
//! from the same runs is worse than no digest.

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

mod attributed_run_row;
mod count_row;
mod pull_request_row;
mod reportable_workspace;
mod run_row;
mod shipped_task_row;
mod task_activity_row;

pub use attributed_run_row::AttributedRunRow;
pub use count_row::CountRow;
pub use pull_request_row::PullRequestRow;
pub use reportable_workspace::ReportableWorkspace;
pub use run_row::RunRow;
pub use shipped_task_row::ShippedTaskRow;
pub use task_activity_row::TaskActivityRow;

/// Active workspaces with a run that finished inside the window.
pub async fn reportable_workspaces(
    pool: &PgPool,
    since: NaiveDateTime,
) -> DbResult<Vec<ReportableWorkspace>> {
    sqlx::query_as(
        r#"
        SELECT DISTINCT
            workspace.id AS workspace_id,
            workspace.name AS name
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        JOIN workspaces workspace ON workspace.id = task.workspace_id
        WHERE run.status NOT IN ('running', 'waiting')
          AND COALESCE(run.completed_at, run.started_at) >= $1
          AND workspace.is_active IS NOT FALSE
        ORDER BY workspace.name, workspace.id
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await
}

/// The workspace's runs that stopped inside `[start, end)`, newest first.
///
/// Newest first because `limit` is a cap, not a promise: over it, the rows past
/// the cap are dropped silently. Every period summarised from these rows ends
/// at `now`, so dropping the oldest costs the longest period some of its early
/// history, while dropping the newest -- what ascending order did -- empties
/// the day and week windows entirely on exactly the busiest workspaces.
/// Nothing downstream reads these in order; each consumer filters or weights
/// by `finished_at`.
pub async fn finished_runs(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Vec<RunRow>> {
    sqlx::query_as(
        r#"
        SELECT
            run.id AS run_id,
            run.task_id AS task_id,
            run.status AS status,
            run.error_message AS error_message,
            run.artifacts -> 'failure' ->> 'category' AS error_category,
            run.started_at AS started_at,
            COALESCE(run.completed_at, run.started_at) AS finished_at
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        WHERE task.workspace_id = $1
          AND run.status NOT IN ('running', 'waiting')
          AND COALESCE(run.completed_at, run.started_at) >= $2
          AND COALESCE(run.completed_at, run.started_at) < $3
        ORDER BY COALESCE(run.completed_at, run.started_at) DESC, run.id DESC
        LIMIT $4
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Tasks that reached `complete` inside `[start, end)`, most recent first,
/// wherever they stand now. `completed_at` is not that: every run that ends
/// writes it, whether or not the task was ever finished.
pub async fn shipped_tasks(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Vec<ShippedTaskRow>> {
    sqlx::query_as(
        r#"
        SELECT
            task.id AS task_id,
            task.title AS title,
            task.status AS status,
            task.shipped_at AS shipped_at,
            task.reopened_at AS reopened_at,
            task.pr_url AS pull_request_url
        FROM tasks task
        WHERE task.workspace_id = $1
          AND task.shipped_at IS NOT NULL
          AND task.shipped_at >= $2
          AND task.shipped_at < $3
        ORDER BY task.shipped_at DESC, task.id
        LIMIT $4
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// [`finished_runs`], with the source each run's task belongs to and the model
/// the run used. A task without a source, or whose source was deleted, reads
/// back with every source column null.
pub async fn attributed_runs(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Vec<AttributedRunRow>> {
    sqlx::query_as(
        r#"
        SELECT
            run.id AS run_id,
            run.task_id AS task_id,
            run.status AS status,
            run.error_message AS error_message,
            run.artifacts -> 'failure' ->> 'category' AS error_category,
            run.started_at AS started_at,
            COALESCE(run.completed_at, run.started_at) AS finished_at,
            source.id AS source_id,
            source.name AS source_name,
            source.source_type AS source_type,
            run.model AS model
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        LEFT JOIN sources source ON source.id = task.source_id
        WHERE task.workspace_id = $1
          AND run.status NOT IN ('running', 'waiting')
          AND COALESCE(run.completed_at, run.started_at) >= $2
          AND COALESCE(run.completed_at, run.started_at) < $3
        ORDER BY COALESCE(run.completed_at, run.started_at) DESC, run.id DESC
        LIMIT $4
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Every task whose pull request was opened inside `[start, end)`, with how it
/// was received as recorded on the newest run that opened the same pull request.
///
/// A run that recorded the pull request as a bare URL predates reception
/// tracking, so it contributes no timing, review or quality.
pub async fn pull_requests(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> DbResult<Vec<PullRequestRow>> {
    sqlx::query_as(
        r#"
        SELECT
            task.id AS task_id,
            source.id AS source_id,
            task.pr_status AS pr_status,
            reception.minutes_to_merge AS minutes_to_merge,
            reception.review_cycles AS review_cycles,
            reception.approvals AS approvals,
            reception.quality_band AS quality_band
        FROM tasks task
        LEFT JOIN sources source ON source.id = task.source_id
        LEFT JOIN LATERAL (
            SELECT
                CASE WHEN jsonb_typeof(run.artifacts -> 'pr' -> 'minutes_to_merge') = 'number'
                    THEN (run.artifacts -> 'pr' ->> 'minutes_to_merge')::numeric::float8
                END AS minutes_to_merge,
                CASE WHEN jsonb_typeof(run.artifacts -> 'pr' -> 'review_cycles') = 'number'
                    THEN (run.artifacts -> 'pr' ->> 'review_cycles')::numeric::int8
                END AS review_cycles,
                CASE WHEN jsonb_typeof(run.artifacts -> 'pr' -> 'approvals') = 'number'
                    THEN (run.artifacts -> 'pr' ->> 'approvals')::numeric::int8
                END AS approvals,
                CASE WHEN jsonb_typeof(run.artifacts -> 'pr') = 'object'
                    THEN run.artifacts -> 'quality' ->> 'band'
                END AS quality_band
            FROM task_runs run
            WHERE run.task_id = task.id
              AND CASE jsonb_typeof(run.artifacts -> 'pr')
                    WHEN 'object' THEN run.artifacts -> 'pr' ->> 'pr_url'
                    WHEN 'string' THEN run.artifacts ->> 'pr'
                  END = task.pr_url
            ORDER BY COALESCE(run.completed_at, run.started_at) DESC NULLS LAST, run.id DESC
            LIMIT 1
        ) reception ON TRUE
        WHERE task.workspace_id = $1
          AND task.pr_url IS NOT NULL
          AND task.pr_created_at >= $2
          AND task.pr_created_at < $3
        ORDER BY task.pr_created_at DESC, task.id
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
}

/// How many of the workspace's tasks stand in each status right now.
pub async fn task_statuses(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<CountRow>> {
    sqlx::query_as(
        r#"
        SELECT
            task.status AS name,
            COUNT(*) AS count
        FROM tasks task
        WHERE task.workspace_id = $1
        GROUP BY task.status
        "#,
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

/// How many tasks each origin put on the board inside `[start, end)`.
pub async fn task_origins(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> DbResult<Vec<CountRow>> {
    sqlx::query_as(
        r#"
        SELECT
            task.origin AS name,
            COUNT(*) AS count
        FROM tasks task
        WHERE task.workspace_id = $1
          AND task.created_at >= $2
          AND task.created_at < $3
        GROUP BY task.origin
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
}

/// Tasks created inside `[start, end)`, tasks shipped inside it, and how many
/// of those shipped have since been reopened.
pub async fn task_activity(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> DbResult<TaskActivityRow> {
    sqlx::query_as(
        r#"
        SELECT
            COUNT(*) FILTER (
                WHERE task.created_at >= $2 AND task.created_at < $3
            ) AS created,
            COUNT(*) FILTER (
                WHERE task.shipped_at >= $2 AND task.shipped_at < $3
            ) AS shipped,
            COUNT(*) FILTER (
                WHERE task.shipped_at >= $2
                  AND task.shipped_at < $3
                  AND task.reopened_at IS NOT NULL
            ) AS reopened
        FROM tasks task
        WHERE task.workspace_id = $1
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
}

/// How many runs that finished inside `[start, end)` reached each evaluation
/// verdict, reading each run's newest log from `phase` and `agent` that
/// carries one.
pub async fn evaluation_verdicts(
    pool: &PgPool,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    phase: &str,
    agent: &str,
) -> DbResult<Vec<CountRow>> {
    sqlx::query_as(
        r#"
        SELECT
            newest.verdict AS name,
            COUNT(*) AS count
        FROM (
            SELECT DISTINCT ON (log.task_run_id)
                log.metadata ->> 'verdict' AS verdict
            FROM task_run_logs log
            JOIN task_runs run ON run.id = log.task_run_id
            JOIN tasks task ON task.id = run.task_id
            WHERE task.workspace_id = $1
              AND log.phase = $4
              AND log.agent_type = $5
              AND log.metadata ? 'verdict'
              AND COALESCE(run.completed_at, run.started_at) >= $2
              AND COALESCE(run.completed_at, run.started_at) < $3
            ORDER BY log.task_run_id, log.created_at DESC NULLS LAST, log.id DESC
        ) newest
        WHERE newest.verdict IS NOT NULL
        GROUP BY newest.verdict
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .bind(end)
    .bind(phase)
    .bind(agent)
    .fetch_all(pool)
    .await
}
