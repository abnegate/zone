//! Project endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::projects;
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

/// Project response
#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    id: Uuid,
    workspace_id: Option<Uuid>,
    source_id: Option<Uuid>,
    name: String,
    description: Option<String>,
    status: String,
    github_repo_url: Option<String>,
}

impl From<projects::ProjectRow> for ProjectResponse {
    fn from(row: projects::ProjectRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            source_id: row.source_id,
            name: row.name,
            description: row.description,
            status: row.status,
            github_repo_url: row.github_repo_url,
        }
    }
}

/// Query parameters for listing projects
#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    status: Option<String>,
}

/// Create project request
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    name: String,
    description: Option<String>,
    workspace_id: Option<Uuid>,
}

/// Update project request
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

/// Link GitHub request
#[derive(Debug, Deserialize)]
pub struct LinkGitHubRequest {
    repo_url: String,
    access_token: Option<String>,
}

/// GET /api/projects
pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<ListProjectsQuery>,
) -> impl IntoResponse {
    match projects::list_projects(state.db(), query.status.as_deref()).await {
        Ok(projs) => Json(
            projs
                .into_iter()
                .map(ProjectResponse::from)
                .collect::<Vec<_>>(),
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

/// POST /api/projects
pub async fn create(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    match projects::create_project(
        state.db(),
        &req.name,
        req.description.as_deref(),
        req.workspace_id,
    )
    .await
    {
        Ok(proj) => (StatusCode::CREATED, Json(ProjectResponse::from(proj))).into_response(),
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

/// GET /api/projects/:id
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => Json(ProjectResponse::from(proj)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
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

/// PUT /api/projects/:id
pub async fn update(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    match projects::update_project(
        state.db(),
        id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.status.as_deref(),
    )
    .await
    {
        Ok(Some(proj)) => Json(ProjectResponse::from(proj)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
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

/// DELETE /api/projects/:id
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match projects::delete_project(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
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

/// POST /api/projects/:id/github
pub async fn link_github(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<LinkGitHubRequest>,
) -> impl IntoResponse {
    match projects::link_github(state.db(), id, &req.repo_url, req.access_token.as_deref()).await {
        Ok(Some(proj)) => Json(ProjectResponse::from(proj)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
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

/// DELETE /api/projects/:id/github
pub async fn unlink_github(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match projects::unlink_github(state.db(), id).await {
        Ok(Some(proj)) => Json(ProjectResponse::from(proj)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
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
