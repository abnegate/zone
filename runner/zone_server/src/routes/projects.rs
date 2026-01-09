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
use crate::db::{projects, workspace_members};
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
    workspace_id: Uuid,
    status: Option<String>,
}

/// Create project request
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    name: String,
    description: Option<String>,
    workspace_id: Uuid,
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

/// GET /api/projects?workspace_id=xxx
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListProjectsQuery>,
) -> impl IntoResponse {
    // Verify workspace membership
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Check if user is a member of the workspace
    match workspace_members::is_member(state.db(), user_id, query.workspace_id).await {
        Ok(true) => {
            // User is a member, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Not a member of this workspace")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking membership: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    match projects::list_projects(state.db(), query.workspace_id, query.status.as_deref()).await {
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
    auth: AuthUser,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    // Verify workspace membership (must be writer or higher)
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Check if user can write to the workspace
    match workspace_members::can_write(state.db(), req.workspace_id, user_id).await {
        Ok(true) => {
            // User can write, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    match projects::create_project(
        state.db(),
        &req.name,
        req.description.as_deref(),
        Some(req.workspace_id),
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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user has access to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Project has no workspace association")),
            )
                .into_response();
        }
    };

    match workspace_members::is_member(state.db(), user_id, workspace_id).await {
        Ok(true) => {
            // User is a member, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Not a member of this workspace")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking membership: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    Json(ProjectResponse::from(proj)).into_response()
}

/// PUT /api/projects/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Project has no workspace association")),
            )
                .into_response();
        }
    };

    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => {
            // User can write, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace (delete requires write access)
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Project has no workspace association")),
            )
                .into_response();
        }
    };

    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => {
            // User can write, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

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
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<LinkGitHubRequest>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Project has no workspace association")),
            )
                .into_response();
        }
    };

    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => {
            // User can write, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Project has no workspace association")),
            )
                .into_response();
        }
    };

    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => {
            // User can write, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

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
