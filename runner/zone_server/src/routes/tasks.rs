//! Task endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::{sources, task_access, tasks, workspace_members};
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

fn database_error(error: sqlx::Error) -> axum::response::Response {
    if matches!(&error, sqlx::Error::Protocol(message) if message == "Workspace access denied") {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Workspace access required")),
        )
            .into_response();
    }
    if matches!(error, sqlx::Error::RowNotFound) {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task or related resource not found")),
        )
            .into_response();
    }
    tracing::error!(%error, "Task database operation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse::new("Internal server error")),
    )
        .into_response()
}

/// A caller who is not a member is told the resource does not exist, so these
/// endpoints cannot be used to enumerate ids across tenants. A member who lacks
/// the role for a write already knows the workspace exists, so that stays 403.
async fn authorize_workspace(
    state: &AppState,
    auth: &AuthUser,
    workspace: Uuid,
    write: bool,
    missing: &str,
) -> Result<Uuid, (StatusCode, Json<ErrorResponse>)> {
    let actor = Uuid::parse_str(&auth.0.sub).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse::new("Invalid user")),
        )
    })?;
    let role = workspace_members::get_role(state.db(), actor, workspace)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Could not authorize task access");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
        })?;
    let Some(role) = role else {
        return Err((StatusCode::NOT_FOUND, Json(ErrorResponse::new(missing))));
    };
    if write && role < workspace_members::WorkspaceRole::Member {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Workspace access required")),
        ));
    }
    Ok(actor)
}

async fn authorize_task(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
    write: bool,
    missing: &str,
) -> Result<(Uuid, tasks::TaskRow), (StatusCode, Json<ErrorResponse>)> {
    let task = tasks::get_task(state.db(), id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Could not resolve task");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(ErrorResponse::new(missing))))?;
    let actor = authorize_workspace(state, auth, task.workspace_id, write, missing).await?;
    Ok((actor, task))
}

async fn authorize_run(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
) -> Result<task_access::Snapshot, (StatusCode, Json<ErrorResponse>)> {
    let run = tasks::get_task_run(state.db(), id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Could not resolve task run");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Task run not found")),
            )
        })?;
    let (actor, _) = authorize_task(state, auth, run.task_id, false, "Task run not found").await?;
    task_access::read(state.db(), id, actor)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Could not read authorized task snapshot");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Task run not found")),
            )
        })
}

async fn validate_projects(
    state: &AppState,
    workspace: Uuid,
    projects: &[Uuid],
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    for project in projects {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND workspace_id = $2)",
        )
        .bind(project)
        .bind(workspace)
        .fetch_one(state.db())
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
        })?;
        if !valid {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Project is not available in this workspace",
                )),
            ));
        }
    }
    Ok(())
}

/// Task response
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    task: TaskData,
}

/// Task data
#[derive(Debug, Serialize)]
pub struct TaskData {
    id: Uuid,
    workspace_id: Uuid,
    project_ids: Vec<Uuid>,
    title: String,
    description: String,
    acceptance_criteria: Option<String>,
    status: String,
    priority: Option<i32>,
    is_agentic: bool,
    model_name: Option<String>,
    dependencies: serde_json::Value,
    github_repo_url: Option<String>,
    source_id: Option<Uuid>,
    source_ids: Vec<Uuid>,
    worker_id: Option<String>,
    queued_at: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    pr_url: Option<String>,
    branch_name: Option<String>,
    pr_status: Option<String>,
    pr_created_at: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

/// Tasks list response
#[derive(Debug, Serialize)]
pub struct TasksListResponse {
    tasks: Vec<TaskData>,
}

impl From<tasks::TaskRow> for TaskData {
    fn from(row: tasks::TaskRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            project_ids: row.project_ids,
            title: row.title,
            description: row.description,
            acceptance_criteria: row.acceptance_criteria,
            status: row.status,
            priority: row.priority,
            is_agentic: row.is_agentic,
            model_name: row.model_name,
            dependencies: row.dependencies.unwrap_or_else(|| serde_json::json!([])),
            github_repo_url: row.github_repo_url,
            source_id: row.source_id,
            source_ids: row.source_ids.unwrap_or_default(),
            worker_id: row.worker_id,
            queued_at: row
                .queued_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            started_at: row
                .started_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            completed_at: row
                .completed_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            pr_url: row.pr_url,
            branch_name: row.branch_name,
            pr_status: row.pr_status,
            pr_created_at: row
                .pr_created_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl From<tasks::TaskRow> for TaskResponse {
    fn from(row: tasks::TaskRow) -> Self {
        Self {
            task: TaskData::from(row),
        }
    }
}

/// Task run response
#[derive(Debug, Serialize)]
pub struct TaskRunResponse {
    run: TaskRunData,
}

/// Task run data
#[derive(Debug, Serialize)]
pub struct TaskRunData {
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option<String>,
    progress_percent: Option<i32>,
    error_message: Option<String>,
}

/// Task runs list response
#[derive(Debug, Serialize)]
pub struct TaskRunsListResponse {
    runs: Vec<TaskRunData>,
}

impl From<tasks::TaskRunRow> for TaskRunData {
    fn from(row: tasks::TaskRunRow) -> Self {
        Self {
            id: row.id,
            task_id: row.task_id,
            status: row.status,
            current_phase: row.current_phase,
            progress_percent: row.progress_percent,
            error_message: row.error_message,
        }
    }
}

impl From<tasks::TaskRunRow> for TaskRunResponse {
    fn from(row: tasks::TaskRunRow) -> Self {
        Self {
            run: TaskRunData::from(row),
        }
    }
}

/// Task run log response
#[derive(Debug, Serialize)]
pub struct TaskRunLogResponse {
    log: TaskRunLogData,
}

/// Task run log data
#[derive(Debug, Serialize)]
pub struct TaskRunLogData {
    id: Uuid,
    phase: String,
    agent_type: String,
    log_level: String,
    message: String,
    metadata: Option<serde_json::Value>,
    created_at: String,
}

/// Task run logs list response
#[derive(Debug, Serialize)]
pub struct TaskRunLogsListResponse {
    logs: Vec<TaskRunLogData>,
}

impl From<tasks::TaskRunLogRow> for TaskRunLogData {
    fn from(row: tasks::TaskRunLogRow) -> Self {
        Self {
            id: row.id,
            phase: row.phase,
            agent_type: row.agent_type,
            log_level: row.log_level,
            message: row.message,
            metadata: row.metadata,
            created_at: row
                .created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

impl From<tasks::TaskRunLogRow> for TaskRunLogResponse {
    fn from(row: tasks::TaskRunLogRow) -> Self {
        Self {
            log: TaskRunLogData::from(row),
        }
    }
}

/// Query parameters for listing tasks
#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    project_id: Option<Uuid>,
    status: Option<String>,
}

/// Create task request
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    #[serde(default)]
    project_ids: Vec<Uuid>,
    title: String,
    description: String,
    acceptance_criteria: Option<String>,
    priority: Option<i32>,
    is_agentic: Option<bool>,
    source_id: Option<Uuid>,
}

/// Update task request
#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    title: Option<String>,
    description: Option<String>,
    acceptance_criteria: Option<String>,
    status: Option<String>,
    priority: Option<i32>,
    project_ids: Option<Vec<Uuid>>,
}

/// GET /api/workspaces/:workspace_id/tasks
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListTasksQuery>,
) -> impl IntoResponse {
    let _actor = match authorize_workspace(
        &state,
        &auth,
        workspace_id,
        false,
        "Workspace not found",
    )
    .await
    {
        Ok(actor) => actor,
        Err(response) => return response.into_response(),
    };

    match tasks::list_tasks(
        state.db(),
        workspace_id,
        query.project_id,
        query.status.as_deref(),
    )
    .await
    {
        Ok(items) => Json(TasksListResponse {
            tasks: items.into_iter().map(TaskData::from).collect(),
        })
        .into_response(),
        Err(error) => database_error(error),
    }
}

/// POST /api/workspaces/:workspace_id/tasks
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let actor =
        match authorize_workspace(&state, &auth, workspace_id, true, "Workspace not found").await {
            Ok(actor) => actor,
            Err(response) => return response.into_response(),
        };
    if let Err(response) = validate_projects(&state, workspace_id, &request.project_ids).await {
        return response.into_response();
    }

    if let Some(source_id) = request.source_id {
        match sources::get_source(state.db(), source_id, workspace_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "Source is not available in this workspace",
                    )),
                )
                    .into_response();
            }
            Err(error) => {
                tracing::error!("Database error: {}", error);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    match tasks::create_task_as(
        state.db(),
        workspace_id,
        &request.project_ids,
        &request.title,
        &request.description,
        request.acceptance_criteria.as_deref(),
        request.priority,
        request.is_agentic.unwrap_or(false),
        request.source_id,
        Some(actor),
    )
    .await
    {
        Ok(task) => (StatusCode::CREATED, Json(TaskResponse::from(task))).into_response(),
        Err(error) => database_error(error),
    }
}

/// GET /api/tasks/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (_actor, task) = match authorize_task(&state, &auth, id, false, "Task not found").await {
        Ok(task) => task,
        Err(response) => return response.into_response(),
    };

    Json(TaskResponse::from(task)).into_response()
}

/// PUT /api/tasks/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTaskRequest>,
) -> impl IntoResponse {
    let (actor, task) = match authorize_task(&state, &auth, id, true, "Task not found").await {
        Ok(task) => task,
        Err(response) => return response.into_response(),
    };
    if let Some(projects) = &req.project_ids
        && let Err(response) = validate_projects(&state, task.workspace_id, projects).await
    {
        return response.into_response();
    }

    match tasks::update_task_as(
        state.db(),
        id,
        req.title.as_deref(),
        req.description.as_deref(),
        req.acceptance_criteria.as_deref(),
        req.status.as_deref(),
        req.priority,
        req.project_ids.as_deref(),
        Some(actor),
    )
    .await
    {
        Ok(Some(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse::new(
                "Task has an active run or is no longer available",
            )),
        )
            .into_response(),
        Err(error) => database_error(error),
    }
}

/// DELETE /api/tasks/:id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (actor, _task) = match authorize_task(&state, &auth, id, true, "Task not found").await {
        Ok(task) => task,
        Err(response) => return response.into_response(),
    };

    match tasks::delete_task_as(state.db(), id, Some(actor)).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task not found")),
        )
            .into_response(),
        Err(error) => database_error(error),
    }
}

/// POST /api/tasks/:id/queue
pub async fn queue(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (actor, _task) = match authorize_task(&state, &auth, id, true, "Task not found").await {
        Ok(task) => task,
        Err(response) => return response.into_response(),
    };

    match tasks::queue_task_as(state.db(), id, Some(actor)).await {
        Ok(Some(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse::new(
                "Task has an active run or is no longer available",
            )),
        )
            .into_response(),
        Err(error) => database_error(error),
    }
}

/// GET /api/tasks/:id/runs
pub async fn list_runs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (actor, _task) = match authorize_task(&state, &auth, id, false, "Task not found").await {
        Ok(task) => task,
        Err(response) => return response.into_response(),
    };

    match tasks::list_task_runs_as(state.db(), id, Some(actor)).await {
        Ok(runs) => Json(TaskRunsListResponse {
            runs: runs.into_iter().map(TaskRunData::from).collect(),
        })
        .into_response(),
        Err(error) => database_error(error),
    }
}

/// POST /api/tasks/:id/runs
pub async fn create_run(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (actor, _task) = match authorize_task(&state, &auth, id, true, "Task not found").await {
        Ok(task) => task,
        Err(response) => return response.into_response(),
    };

    match tasks::create_task_run_as(state.db(), id, Some(actor)).await {
        Ok(run) => {
            let run_id = run.id;
            let task_id = id;
            let state_clone = state.clone();

            // Spawn background task execution
            tokio::spawn(async move {
                crate::workers::task::execute_task_run(&state_clone, run_id, task_id).await;
            });

            (StatusCode::CREATED, Json(TaskRunResponse::from(run))).into_response()
        }
        Err(e) => {
            if e.as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                return (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new("Task already has an active run")),
                )
                    .into_response();
            }

            database_error(e)
        }
    }
}

/// GET /api/tasks/runs/:run_id
pub async fn get_run(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    match authorize_run(&state, &auth, run_id).await {
        Ok(snapshot) => Json(TaskRunResponse::from(snapshot.run)).into_response(),
        Err(response) => response.into_response(),
    }
}

/// GET /api/tasks/runs/:run_id/logs
pub async fn get_run_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    match authorize_run(&state, &auth, run_id).await {
        Ok(snapshot) => Json(TaskRunLogsListResponse {
            logs: snapshot
                .logs
                .into_iter()
                .map(TaskRunLogData::from)
                .collect(),
        })
        .into_response(),
        Err(response) => response.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use serde_json::Value;

    fn row(populated: bool) -> tasks::TaskRow {
        let timestamp = NaiveDateTime::parse_from_str("2026-09-04 12:05:28", "%Y-%m-%d %H:%M:%S")
            .expect("valid timestamp");
        tasks::TaskRow {
            id: Uuid::from_u128(1),
            workspace_id: Uuid::from_u128(2),
            created_by: None,
            project_ids: if populated {
                vec![Uuid::from_u128(3)]
            } else {
                vec![]
            },
            title: "Test task".into(),
            description: "Test description".into(),
            acceptance_criteria: populated.then(|| "Checks pass".into()),
            status: "created".into(),
            priority: populated.then_some(5),
            model_name: populated.then(|| "test-model".into()),
            dependencies: populated.then(|| serde_json::json!([Uuid::from_u128(4)])),
            is_agentic: true,
            github_repo_url: populated.then(|| "https://github.com/abnegate/zone".into()),
            source_id: populated.then_some(Uuid::from_u128(5)),
            source_ids: populated.then(|| vec![Uuid::from_u128(5)]),
            worker_id: populated.then(|| "worker-1".into()),
            queued_at: populated.then_some(timestamp),
            started_at: populated.then_some(timestamp),
            completed_at: populated.then_some(timestamp),
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
            pr_url: populated.then(|| "https://github.com/abnegate/zone/pull/1".into()),
            branch_name: populated.then(|| "fix/task".into()),
            pr_status: populated.then(|| "open".into()),
            pr_created_at: populated.then_some(timestamp),
        }
    }

    #[test]
    fn task_response_preserves_nullable_fields() {
        let expected: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/task-created.json")).unwrap();
        assert_eq!(
            serde_json::to_value(TaskResponse::from(row(false))).unwrap(),
            expected
        );
    }

    #[test]
    fn task_response_preserves_populated_fields() {
        let expected: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/task-populated.json")).unwrap();
        assert_eq!(
            serde_json::to_value(TaskResponse::from(row(true))).unwrap(),
            expected
        );
        let list = TasksListResponse {
            tasks: vec![TaskData::from(row(true))],
        };
        assert_eq!(
            serde_json::to_value(list).unwrap()["tasks"][0],
            expected["task"]
        );
    }
}
