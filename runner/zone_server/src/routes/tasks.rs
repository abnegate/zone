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
use crate::db::tasks;
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
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
    _auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListTasksQuery>,
) -> impl IntoResponse {
    match tasks::list_tasks(state.db(), workspace_id, query.project_id, query.status.as_deref()).await {
        Ok(items) => Json(TasksListResponse {
            tasks: items.into_iter().map(TaskData::from).collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/workspaces/:workspace_id/tasks
pub async fn create(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(req): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    match tasks::create_task(
        state.db(),
        workspace_id,
        &req.project_ids,
        &req.title,
        &req.description,
        req.acceptance_criteria.as_deref(),
        req.priority,
        req.is_agentic.unwrap_or(false),
    )
    .await
    {
        Ok(task) => (StatusCode::CREATED, Json(TaskResponse::from(task))).into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/tasks/:id
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match tasks::get_task(state.db(), id).await {
        Ok(Some(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// PUT /api/tasks/:id
pub async fn update(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTaskRequest>,
) -> impl IntoResponse {
    match tasks::update_task(
        state.db(),
        id,
        req.title.as_deref(),
        req.description.as_deref(),
        req.acceptance_criteria.as_deref(),
        req.status.as_deref(),
        req.priority,
        req.project_ids.as_deref(),
    )
    .await
    {
        Ok(Some(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// DELETE /api/tasks/:id
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match tasks::delete_task(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/tasks/:id/queue
pub async fn queue(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match tasks::queue_task(state.db(), id).await {
        Ok(Some(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/tasks/:id/runs
pub async fn list_runs(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match tasks::list_task_runs(state.db(), id).await {
        Ok(runs) => Json(TaskRunsListResponse {
            runs: runs.into_iter().map(TaskRunData::from).collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/tasks/:id/runs
pub async fn create_run(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Check if there's already a running task run for this task
    // This prevents duplicate concurrent executions
    match tasks::list_task_runs(state.db(), id).await {
        Ok(runs) => {
            let active_run = runs
                .iter()
                .find(|r| r.status == "running" || r.status == "pending");
            if let Some(existing) = active_run {
                return (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new(format!(
                        "Task already has an active run (id: {}, status: {})",
                        existing.id, existing.status
                    ))),
                )
                    .into_response();
            }
        }
        Err(e) => {
            tracing::error!("Failed to check existing runs: {}", e);
            // Continue anyway - better to potentially have duplicates than fail entirely
        }
    }

    match tasks::create_task_run(state.db(), id).await {
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
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/tasks/runs/:run_id
pub async fn get_run(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    match tasks::get_task_run(state.db(), run_id).await {
        Ok(Some(run)) => Json(TaskRunResponse::from(run)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task run not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/tasks/runs/:run_id/logs
pub async fn get_run_logs(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    match tasks::get_task_run_logs(state.db(), run_id).await {
        Ok(logs) => Json(TaskRunLogsListResponse {
            logs: logs.into_iter().map(TaskRunLogData::from).collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}
