//! Every database read and write the learning loop makes.
//!
//! Loading is kept here and decision-making is kept out, so each threshold in the loop
//! can be exercised against in-memory data without a database. JSON columns are read and
//! written as text and parsed in Rust, matching how the promotion worker reads embedding
//! vectors, so nothing here depends on a compile-time query cache.

use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeMap;
use uuid::Uuid;

use super::observation::{ChangeType, FileChange};
use super::strategy::ToolInvocation;
use crate::db::DbResult;

/// One run that has stopped, with everything the loop learns from it.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedRun {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub error_message: Option<String>,
    pub finished_at: NaiveDateTime,
    pub artifacts: Option<Value>,
    pub pull_request_url: Option<String>,
    pub pull_request_opened_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct FinishedRunRow {
    run_id: Uuid,
    task_id: Uuid,
    status: String,
    error_message: Option<String>,
    finished_at: NaiveDateTime,
    artifacts: Option<String>,
    pull_request_url: Option<String>,
    pull_request_opened_at: Option<NaiveDateTime>,
}

impl From<FinishedRunRow> for FinishedRun {
    fn from(row: FinishedRunRow) -> Self {
        Self {
            run_id: row.run_id,
            task_id: row.task_id,
            status: row.status,
            error_message: row.error_message,
            finished_at: row.finished_at,
            artifacts: row
                .artifacts
                .as_deref()
                .and_then(|text| serde_json::from_str(text).ok()),
            pull_request_url: row.pull_request_url,
            pull_request_opened_at: row.pull_request_opened_at,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct FileChangeRow {
    run_id: Uuid,
    path: String,
    change_type: String,
    diff: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ToolCallRow {
    run_id: Uuid,
    tool_name: String,
    command: Option<String>,
}

/// Workspaces with a run that finished inside the window.
pub async fn active_workspaces(pool: &PgPool, since: NaiveDateTime) -> DbResult<Vec<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT DISTINCT task.workspace_id
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        JOIN workspaces workspace ON workspace.id = task.workspace_id
        WHERE run.status NOT IN ('running', 'waiting')
          AND COALESCE(run.completed_at, run.started_at) >= $1
          AND workspace.is_active IS NOT FALSE
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await
}

/// The workspace's finished runs, most recent first.
pub async fn load_runs(
    pool: &PgPool,
    workspace_id: Uuid,
    since: NaiveDateTime,
    limit: i64,
) -> DbResult<Vec<FinishedRun>> {
    let rows: Vec<FinishedRunRow> = sqlx::query_as(
        r#"
        SELECT
            run.id AS run_id,
            run.task_id AS task_id,
            run.status AS status,
            run.error_message AS error_message,
            COALESCE(run.completed_at, run.started_at, NOW()) AS finished_at,
            run.artifacts::text AS artifacts,
            task.pr_url AS pull_request_url,
            task.pr_created_at AS pull_request_opened_at
        FROM task_runs run
        JOIN tasks task ON task.id = run.task_id
        WHERE task.workspace_id = $1
          AND run.status NOT IN ('running', 'waiting')
          AND COALESCE(run.completed_at, run.started_at) >= $2
        ORDER BY COALESCE(run.completed_at, run.started_at) DESC, run.id DESC
        LIMIT $3
        "#,
    )
    .bind(workspace_id)
    .bind(since)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(FinishedRun::from).collect())
}

/// Files the given runs changed and left applied.
pub async fn load_file_changes(pool: &PgPool, run_ids: &[Uuid]) -> DbResult<Vec<FileChange>> {
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows: Vec<FileChangeRow> = sqlx::query_as(
        r#"
        SELECT
            change.task_run_id AS run_id,
            change.file_path AS path,
            change.change_type AS change_type,
            change.diff AS diff
        FROM task_file_changes change
        WHERE change.task_run_id = ANY($1)
          AND change.reverted IS NOT TRUE
        ORDER BY change.task_run_id, change.created_at, change.id
        "#,
    )
    .bind(run_ids)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(FileChange {
                run_id: row.run_id,
                path: row.path,
                change_type: ChangeType::parse(&row.change_type)?,
                diff: row.diff,
            })
        })
        .collect())
}

/// Tool calls the given runs made, in the order they were made, keyed by run.
pub async fn load_tool_calls(
    pool: &PgPool,
    run_ids: &[Uuid],
) -> DbResult<BTreeMap<Uuid, Vec<ToolInvocation>>> {
    if run_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let rows: Vec<ToolCallRow> = sqlx::query_as(
        r#"
        SELECT
            call.task_run_id AS run_id,
            call.tool_name AS tool_name,
            call.tool_input ->> 'command' AS command
        FROM task_tool_calls call
        WHERE call.task_run_id = ANY($1)
        ORDER BY call.task_run_id, call.started_at, call.id
        "#,
    )
    .bind(run_ids)
    .fetch_all(pool)
    .await?;

    let mut calls: BTreeMap<Uuid, Vec<ToolInvocation>> = BTreeMap::new();
    for row in rows {
        calls.entry(row.run_id).or_default().push(ToolInvocation {
            tool_name: row.tool_name,
            command: row.command,
        });
    }
    Ok(calls)
}

/// Merge one key into a run's artifacts, leaving every other key untouched.
pub async fn record_artifact(
    pool: &PgPool,
    run_id: Uuid,
    key: &str,
    artifact: &Value,
) -> DbResult<bool> {
    let encoded = serde_json::to_string(artifact).unwrap_or_else(|_| "null".to_string());

    let result = sqlx::query(
        r#"
        UPDATE task_runs
        SET artifacts = COALESCE(artifacts, '{}'::jsonb) || jsonb_build_object($2::text, $3::jsonb)
        WHERE id = $1
        "#,
    )
    .bind(run_id)
    .bind(key)
    .bind(encoded)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(artifacts: Option<&str>) -> FinishedRunRow {
        FinishedRunRow {
            run_id: Uuid::from_u128(1),
            task_id: Uuid::from_u128(2),
            status: "completed".to_string(),
            error_message: None,
            finished_at: NaiveDateTime::default(),
            artifacts: artifacts.map(str::to_string),
            pull_request_url: None,
            pull_request_opened_at: None,
        }
    }

    #[test]
    fn artifacts_are_parsed_out_of_the_text_column() {
        let parsed = FinishedRun::from(row(Some(r#"{"attempts":2}"#)));
        assert_eq!(parsed.artifacts, Some(json!({ "attempts": 2 })));
    }

    #[test]
    fn unreadable_artifacts_are_treated_as_absent() {
        assert_eq!(FinishedRun::from(row(Some("{not json"))).artifacts, None);
        assert_eq!(FinishedRun::from(row(None)).artifacts, None);
    }

    #[test]
    fn change_types_outside_the_schema_are_dropped() {
        assert_eq!(ChangeType::parse("create"), Some(ChangeType::Create));
        assert_eq!(ChangeType::parse("modify"), Some(ChangeType::Modify));
        assert_eq!(ChangeType::parse("delete"), Some(ChangeType::Delete));
        assert_eq!(ChangeType::parse("renamed"), None);
    }
}
