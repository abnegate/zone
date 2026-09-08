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

/// A workspace with finished runs worth reporting on.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ReportableWorkspace {
    pub workspace_id: Uuid,
    pub name: String,
}

/// One finished run, as analytics reads it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RunRow {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub error_message: Option<String>,
    /// The category the learning loop assigned, if it has been past this run.
    pub error_category: Option<String>,
    pub started_at: Option<NaiveDateTime>,
    pub finished_at: NaiveDateTime,
}

/// One task whose fix has shipped, and where that task stands now.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ShippedTaskRow {
    pub task_id: Uuid,
    pub title: String,
    pub status: String,
    pub shipped_at: NaiveDateTime,
    pub last_touched_at: NaiveDateTime,
    pub pull_request_url: Option<String>,
}

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
        WHERE run.status <> 'running'
          AND COALESCE(run.completed_at, run.started_at) >= $1
          AND workspace.is_active IS NOT FALSE
        ORDER BY workspace.name, workspace.id
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await
}

/// The workspace's runs that stopped inside `[start, end)`, oldest first.
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
          AND run.status <> 'running'
          AND COALESCE(run.completed_at, run.started_at) >= $2
          AND COALESCE(run.completed_at, run.started_at) < $3
        ORDER BY COALESCE(run.completed_at, run.started_at), run.id
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

/// Tasks the workspace finished inside `[start, end)`, most recent first.
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
            task.completed_at AS shipped_at,
            COALESCE(task.updated_at, task.completed_at) AS last_touched_at,
            task.pr_url AS pull_request_url
        FROM tasks task
        WHERE task.workspace_id = $1
          AND task.completed_at IS NOT NULL
          AND task.completed_at >= $2
          AND task.completed_at < $3
        ORDER BY task.completed_at DESC, task.id
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
