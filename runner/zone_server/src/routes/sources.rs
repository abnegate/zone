//! Source endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::sources;
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

/// Source response
#[derive(Debug, Serialize)]
pub struct SourceResponse {
    id: Uuid,
    name: String,
    source_type: String,
    config: serde_json::Value,
    description: Option<String>,
    url: Option<String>,
    is_active: bool,
    last_error: Option<String>,
}

impl From<sources::SourceRow> for SourceResponse {
    fn from(row: sources::SourceRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            source_type: row.source_type,
            config: row.config,
            description: row.description,
            url: row.url,
            is_active: row.is_active.unwrap_or(true),
            last_error: row.last_error,
        }
    }
}

/// Query parameters for listing sources
#[derive(Debug, Deserialize)]
pub struct ListSourcesQuery {
    source_type: Option<String>,
    is_active: Option<bool>,
}

/// Create source request
#[derive(Debug, Deserialize)]
pub struct CreateSourceRequest {
    name: String,
    source_type: String,
    config: serde_json::Value,
    description: Option<String>,
    url: Option<String>,
    credentials: Option<String>,
}

/// Update source request
#[derive(Debug, Deserialize)]
pub struct UpdateSourceRequest {
    name: Option<String>,
    config: Option<serde_json::Value>,
    description: Option<String>,
    url: Option<String>,
    is_active: Option<bool>,
}

/// GET /api/sources
pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<ListSourcesQuery>,
) -> impl IntoResponse {
    match sources::list_sources(state.db(), query.source_type.as_deref(), query.is_active).await {
        Ok(items) => Json(
            items
                .into_iter()
                .map(SourceResponse::from)
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

/// POST /api/sources
pub async fn create(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<CreateSourceRequest>,
) -> impl IntoResponse {
    // TODO: Encrypt credentials before storing
    match sources::create_source(
        state.db(),
        &req.name,
        &req.source_type,
        req.config,
        req.description.as_deref(),
        req.url.as_deref(),
        req.credentials.as_deref(),
    )
    .await
    {
        Ok(source) => (StatusCode::CREATED, Json(SourceResponse::from(source))).into_response(),
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

/// GET /api/sources/:id
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match sources::get_source(state.db(), id).await {
        Ok(Some(source)) => Json(SourceResponse::from(source)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Source not found")),
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

/// PUT /api/sources/:id
pub async fn update(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSourceRequest>,
) -> impl IntoResponse {
    match sources::update_source(
        state.db(),
        id,
        req.name.as_deref(),
        req.config,
        req.description.as_deref(),
        req.url.as_deref(),
        req.is_active,
    )
    .await
    {
        Ok(Some(source)) => Json(SourceResponse::from(source)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Source not found")),
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

/// DELETE /api/sources/:id
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match sources::delete_source(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Source not found")),
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

/// POST /api/sources/:id/verify
pub async fn verify(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // TODO: Actually verify the source connection
    // For now, just update the last_verified_at timestamp
    match sources::update_verification(state.db(), id, None).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
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

/// Source type info
#[derive(Debug, Serialize)]
pub struct SourceTypeInfo {
    name: String,
    display_name: String,
    category: String,
    description: String,
    config_schema: serde_json::Value,
}

/// GET /api/sources/types
pub async fn list_types(_auth: AuthUser) -> impl IntoResponse {
    let types = vec![
        SourceTypeInfo {
            name: "github".to_string(),
            display_name: "GitHub".to_string(),
            category: "code".to_string(),
            description: "GitHub repository integration".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string", "description": "Repository owner"},
                    "repo": {"type": "string", "description": "Repository name"},
                    "branch": {"type": "string", "description": "Default branch"}
                },
                "required": ["owner", "repo"]
            }),
        },
        SourceTypeInfo {
            name: "gitlab".to_string(),
            display_name: "GitLab".to_string(),
            category: "code".to_string(),
            description: "GitLab repository integration".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Project ID or path"},
                    "branch": {"type": "string", "description": "Default branch"}
                },
                "required": ["project_id"]
            }),
        },
        SourceTypeInfo {
            name: "local".to_string(),
            display_name: "Local Directory".to_string(),
            category: "filesystem".to_string(),
            description: "Local filesystem directory".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path to directory"}
                },
                "required": ["path"]
            }),
        },
        SourceTypeInfo {
            name: "confluence".to_string(),
            display_name: "Confluence".to_string(),
            category: "documentation".to_string(),
            description: "Atlassian Confluence integration".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "base_url": {"type": "string", "description": "Confluence base URL"},
                    "space_key": {"type": "string", "description": "Space key"}
                },
                "required": ["base_url"]
            }),
        },
        SourceTypeInfo {
            name: "notion".to_string(),
            display_name: "Notion".to_string(),
            category: "documentation".to_string(),
            description: "Notion workspace integration".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "workspace_id": {"type": "string", "description": "Workspace ID"}
                },
                "required": ["workspace_id"]
            }),
        },
    ];

    Json(types)
}
