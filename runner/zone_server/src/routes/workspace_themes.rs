//! Workspace theme endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::workspace_themes;
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

/// Theme response
#[derive(Debug, Serialize)]
pub struct ThemeResponse {
    workspace_id: Uuid,
    primary_color_light: Option<String>,
    secondary_color_light: Option<String>,
    primary_color_dark: Option<String>,
    secondary_color_dark: Option<String>,
    font_family: Option<String>,
    font_size_base: Option<String>,
    border_radius: Option<String>,
}

impl From<workspace_themes::WorkspaceThemeRow> for ThemeResponse {
    fn from(row: workspace_themes::WorkspaceThemeRow) -> Self {
        Self {
            workspace_id: row.workspace_id,
            primary_color_light: row.primary_color_light,
            secondary_color_light: row.secondary_color_light,
            primary_color_dark: row.primary_color_dark,
            secondary_color_dark: row.secondary_color_dark,
            font_family: row.font_family,
            font_size_base: row.font_size_base,
            border_radius: row.border_radius,
        }
    }
}

/// Update theme request
#[derive(Debug, Deserialize)]
pub struct UpdateThemeRequest {
    primary_color_light: Option<String>,
    secondary_color_light: Option<String>,
    primary_color_dark: Option<String>,
    secondary_color_dark: Option<String>,
    font_family: Option<String>,
    font_size_base: Option<String>,
    border_radius: Option<String>,
}

/// GET /api/workspaces/:id/theme
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> impl IntoResponse {
    match workspace_themes::get_theme(state.db(), workspace_id).await {
        Ok(Some(theme)) => Json(ThemeResponse::from(theme)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Theme not found")),
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

/// PUT /api/workspaces/:id/theme
pub async fn upsert(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(req): Json<UpdateThemeRequest>,
) -> impl IntoResponse {
    match workspace_themes::upsert_theme(
        state.db(),
        workspace_id,
        req.primary_color_light.as_deref(),
        req.secondary_color_light.as_deref(),
        req.primary_color_dark.as_deref(),
        req.secondary_color_dark.as_deref(),
        req.font_family.as_deref(),
        req.font_size_base.as_deref(),
        req.border_radius.as_deref(),
    )
    .await
    {
        Ok(theme) => Json(ThemeResponse::from(theme)).into_response(),
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

/// DELETE /api/workspaces/:id/theme
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> impl IntoResponse {
    match workspace_themes::delete_theme(state.db(), workspace_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Theme not found")),
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
