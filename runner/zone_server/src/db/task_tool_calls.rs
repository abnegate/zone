//! The tool calls a task run made, one row per execution.
//!
//! The agent loop reports a call starting and a call finishing as two separate
//! events with no shared id, and the task worker persists each from its own
//! spawned future, so the finish can reach the database before the start. Both
//! writes therefore upsert on the id the worker minted for the call: whichever
//! lands second completes the row rather than losing it.

use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

pub const STATUS_RUNNING: &str = "running";
pub const STATUS_COMPLETED: &str = "completed";
pub const STATUS_FAILED: &str = "failed";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskToolCallRow {
    pub id: Uuid,
    pub task_run_id: Uuid,
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_output: Option<Value>,
    pub status: String,
    pub error_message: Option<String>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
}

/// Record that run `run_id` started `tool_name` with `tool_input`.
pub async fn start(
    pool: &PgPool,
    id: Uuid,
    run_id: Uuid,
    tool_name: &str,
    tool_input: &Value,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO task_tool_calls (id, task_run_id, tool_name, tool_input, status) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(run_id)
    .bind(tool_name)
    .bind(tool_input)
    .bind(STATUS_RUNNING)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record how call `id` ended, creating the row when its start has not landed yet.
#[allow(clippy::too_many_arguments)]
pub async fn finish(
    pool: &PgPool,
    id: Uuid,
    run_id: Uuid,
    tool_name: &str,
    tool_input: &Value,
    success: bool,
    output: &Value,
    error: Option<&str>,
) -> DbResult<()> {
    let status = if success {
        STATUS_COMPLETED
    } else {
        STATUS_FAILED
    };
    sqlx::query(
        "INSERT INTO task_tool_calls \
             (id, task_run_id, tool_name, tool_input, tool_output, status, error_message, completed_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
             tool_output = EXCLUDED.tool_output, \
             status = EXCLUDED.status, \
             error_message = EXCLUDED.error_message, \
             completed_at = EXCLUDED.completed_at",
    )
    .bind(id)
    .bind(run_id)
    .bind(tool_name)
    .bind(tool_input)
    .bind(output)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every call a run made, oldest first.
pub async fn list_for_run(pool: &PgPool, run_id: Uuid) -> DbResult<Vec<TaskToolCallRow>> {
    sqlx::query_as(
        "SELECT id, task_run_id, tool_name, tool_input, tool_output, status, error_message, \
                started_at, completed_at \
         FROM task_tool_calls WHERE task_run_id = $1 ORDER BY started_at ASC, id ASC",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
}
