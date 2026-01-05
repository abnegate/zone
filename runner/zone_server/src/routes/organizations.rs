//! Organization endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::{organizations, workspaces};
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

/// Organization response
#[derive(Debug, Serialize)]
pub struct OrganizationResponse {
    id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
}

impl From<organizations::OrganizationRow> for OrganizationResponse {
    fn from(row: organizations::OrganizationRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            slug: row.slug,
            description: row.description,
            is_active: row.is_active.unwrap_or(true),
        }
    }
}

/// Workspace response
#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
}

impl From<workspaces::WorkspaceRow> for WorkspaceResponse {
    fn from(row: workspaces::WorkspaceRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            name: row.name,
            slug: row.slug,
            description: row.description,
            is_active: row.is_active.unwrap_or(true),
        }
    }
}

/// Create organization request
#[derive(Debug, Deserialize)]
pub struct CreateOrganizationRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

/// Update organization request
#[derive(Debug, Deserialize)]
pub struct UpdateOrganizationRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// Create workspace request
#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

/// Update workspace request
#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// GET /api/organizations
pub async fn list(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    match organizations::list_organizations(state.db()).await {
        Ok(orgs) => Json(
            orgs.into_iter()
                .map(OrganizationResponse::from)
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

/// POST /api/organizations
pub async fn create(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<CreateOrganizationRequest>,
) -> impl IntoResponse {
    match organizations::create_organization(
        state.db(),
        &req.name,
        &req.slug,
        req.description.as_deref(),
    )
    .await
    {
        Ok(org) => (StatusCode::CREATED, Json(OrganizationResponse::from(org))).into_response(),
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

/// GET /api/organizations/:id
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match organizations::get_organization(state.db(), id).await {
        Ok(Some(org)) => Json(OrganizationResponse::from(org)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
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

/// PUT /api/organizations/:id
pub async fn update(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateOrganizationRequest>,
) -> impl IntoResponse {
    match organizations::update_organization(
        state.db(),
        id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.is_active,
    )
    .await
    {
        Ok(Some(org)) => Json(OrganizationResponse::from(org)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
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

/// DELETE /api/organizations/:id
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match organizations::delete_organization(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
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

/// GET /api/organizations/:org_id/workspaces
pub async fn list_workspaces(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    match workspaces::list_workspaces(state.db(), org_id).await {
        Ok(ws) => Json(
            ws.into_iter()
                .map(WorkspaceResponse::from)
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

/// POST /api/organizations/:org_id/workspaces
pub async fn create_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    match workspaces::create_workspace(
        state.db(),
        org_id,
        &req.name,
        &req.slug,
        req.description.as_deref(),
    )
    .await
    {
        Ok(ws) => (StatusCode::CREATED, Json(WorkspaceResponse::from(ws))).into_response(),
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

/// GET /api/workspaces/:id
pub async fn get_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match workspaces::get_workspace(state.db(), id).await {
        Ok(Some(ws)) => Json(WorkspaceResponse::from(ws)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Workspace not found")),
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

/// PUT /api/workspaces/:id
pub async fn update_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> impl IntoResponse {
    match workspaces::update_workspace(
        state.db(),
        id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.is_active,
    )
    .await
    {
        Ok(Some(ws)) => Json(WorkspaceResponse::from(ws)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Workspace not found")),
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

/// DELETE /api/workspaces/:id
pub async fn delete_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match workspaces::delete_workspace(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Workspace not found")),
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
