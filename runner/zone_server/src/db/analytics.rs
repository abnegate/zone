use chrono::NaiveDateTime;
use sqlx::Executor;
use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::Transaction;
use uuid::Uuid;

use super::DbResult;

mod attributed_run_row;
mod capped;
mod count_row;
mod pull_request_row;
mod reportable_workspace;
mod run_row;
mod shipped_task_row;
mod task_activity_row;

pub use attributed_run_row::AttributedRunRow;
pub use capped::Capped;
pub use count_row::CountRow;
pub use pull_request_row::PullRequestRow;
pub use reportable_workspace::ReportableWorkspace;
pub use run_row::RunRow;
pub use shipped_task_row::ShippedTaskRow;
pub use task_activity_row::TaskActivityRow;

const UNFINISHED: [&str; 2] = ["running", "waiting"];

macro_rules! task_source {
    () => {
        r#"
        LEFT JOIN LATERAL (
            SELECT candidate.id AS source_id
            FROM task_projects membership
            JOIN projects project ON project.id = membership.project_id
            JOIN sources candidate
                ON candidate.id = project.source_id
               AND candidate.workspace_id = task.workspace_id
            WHERE task.source_id IS NULL
              AND membership.task_id = task.id
            ORDER BY project.id
            LIMIT 1
        ) attribution ON TRUE
        LEFT JOIN sources source
            ON source.id = COALESCE(task.source_id, attribution.source_id)
           AND source.workspace_id = task.workspace_id
        "#
    };
}

macro_rules! run_columns {
    () => {
        r#"
            run.id AS run_id,
            run.task_id AS task_id,
            run.status AS status,
            run.error_message AS error_message,
            run.artifacts -> 'failure' ->> 'category' AS error_category,
            run.started_at AS started_at,
            COALESCE(run.completed_at, run.started_at) AS finished_at
        "#
    };
}

macro_rules! finished_inside {
    () => {
        r#"
            task.workspace_id = $1
            AND run.status <> ALL($2)
            AND COALESCE(run.completed_at, run.started_at) >= $3
            AND COALESCE(run.completed_at, run.started_at) < $4
        "#
    };
}

macro_rules! newest_first {
    () => {
        r#"
        ORDER BY COALESCE(run.completed_at, run.started_at) DESC, run.id DESC
        LIMIT $5
        "#
    };
}

pub async fn snapshot(pool: &PgPool) -> DbResult<Transaction<'static, Postgres>> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}

/// Active workspaces with a run that finished inside the window.
pub async fn reportable_workspaces<'e, E>(
    executor: E,
    since: NaiveDateTime,
) -> DbResult<Vec<ReportableWorkspace>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(
        r#"
        SELECT DISTINCT
            workspace.id AS workspace_id,
            workspace.name AS name
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        JOIN workspaces workspace ON workspace.id = task.workspace_id
        WHERE run.status <> ALL($1)
          AND COALESCE(run.completed_at, run.started_at) >= $2
          AND workspace.is_active IS NOT FALSE
        ORDER BY workspace.name, workspace.id
        "#,
    )
    .bind(UNFINISHED.as_slice())
    .bind(since)
    .fetch_all(executor)
    .await
}

/// Tasks that reached `complete` inside `[start, end)`, most recent first,
/// wherever they stand now. `completed_at` is not that: every run that ends
/// writes it, whether or not the task was ever finished.
pub async fn shipped_tasks<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Vec<ShippedTaskRow>>
where
    E: Executor<'e, Database = Postgres>,
{
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
    .fetch_all(executor)
    .await
}

pub async fn finished_runs<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Capped<RunRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let query = sqlx::query_as(concat!(
        "SELECT",
        run_columns!(),
        "FROM task_runs run JOIN tasks task ON task.id = run.task_id WHERE",
        finished_inside!(),
        newest_first!(),
    ))
    .bind(workspace_id)
    .bind(UNFINISHED.as_slice())
    .bind(start)
    .bind(end);
    Capped::fetch(query, limit, executor).await
}

pub async fn attributed_runs<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Capped<AttributedRunRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let query = sqlx::query_as(concat!(
        "SELECT",
        run_columns!(),
        r#",
            source.id AS source_id,
            source.name AS source_name,
            source.source_type AS source_type,
            run.model AS model
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        "#,
        task_source!(),
        "WHERE",
        finished_inside!(),
        newest_first!(),
    ))
    .bind(workspace_id)
    .bind(UNFINISHED.as_slice())
    .bind(start)
    .bind(end);
    Capped::fetch(query, limit, executor).await
}

/// A run that recorded the pull request as a bare URL predates reception
/// tracking, so it contributes no timing, review or quality.
pub async fn pull_requests<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    limit: i64,
) -> DbResult<Capped<PullRequestRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let query = sqlx::query_as(concat!(
        r#"
        SELECT
            task.id AS task_id,
            source.id AS source_id,
            source.name AS source_name,
            source.source_type AS source_type,
            task.pr_status AS status,
            task.pr_created_at AS opened_at,
            reception.artifacts AS artifacts,
            reception.quality_band AS quality_band
        FROM tasks task
        "#,
        task_source!(),
        r#"
        LEFT JOIN LATERAL (
            SELECT
                CASE WHEN jsonb_typeof(run.artifacts -> 'pr') = 'object'
                    THEN jsonb_build_object('pr', run.artifacts -> 'pr')::text
                END AS artifacts,
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
        LIMIT $4
        "#
    ))
    .bind(workspace_id)
    .bind(start)
    .bind(end);
    Capped::fetch(query, limit, executor).await
}

/// How many of the workspace's tasks stand in each status right now.
pub async fn task_statuses<'e, E>(executor: E, workspace_id: Uuid) -> DbResult<Vec<CountRow>>
where
    E: Executor<'e, Database = Postgres>,
{
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
    .fetch_all(executor)
    .await
}

/// How many tasks each origin put on the board inside `[start, end)`.
pub async fn task_origins<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> DbResult<Vec<CountRow>>
where
    E: Executor<'e, Database = Postgres>,
{
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
    .fetch_all(executor)
    .await
}

/// Tasks created inside `[start, end)`, tasks shipped inside it, and how many
/// of those shipped have since been reopened.
pub async fn task_activity<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> DbResult<TaskActivityRow>
where
    E: Executor<'e, Database = Postgres>,
{
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
    .fetch_one(executor)
    .await
}

/// How many runs that finished inside `[start, end)` reached each evaluation
/// verdict, reading each run's newest log from `phase` and `agent` that
/// carries one. A log whose verdict is null or not text carries none, so it
/// cannot hide an older log that does.
pub async fn evaluation_verdicts<'e, E>(
    executor: E,
    workspace_id: Uuid,
    start: NaiveDateTime,
    end: NaiveDateTime,
    phase: &str,
    agent: &str,
) -> DbResult<Vec<CountRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
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
            WHERE log.phase = $5
              AND log.agent_type = $6
              AND jsonb_typeof(log.metadata -> 'verdict') = 'string'
              AND
        "#,
        finished_inside!(),
        r#"
            ORDER BY log.task_run_id, log.created_at DESC NULLS LAST, log.id DESC
        ) newest
        GROUP BY newest.verdict
        "#
    ))
    .bind(workspace_id)
    .bind(UNFINISHED.as_slice())
    .bind(start)
    .bind(end)
    .bind(phase)
    .bind(agent)
    .fetch_all(executor)
    .await
}
