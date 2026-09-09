//! Workspace theme endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::workspace_themes;
use crate::state::AppState;

use super::common::Timestamps;

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
    #[serde(flatten)]
    timestamps: Timestamps,
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
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
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

impl UpdateThemeRequest {
    fn update(&self) -> workspace_themes::Update<'_> {
        workspace_themes::Update {
            primary_color_light: self.primary_color_light.as_deref(),
            secondary_color_light: self.secondary_color_light.as_deref(),
            primary_color_dark: self.primary_color_dark.as_deref(),
            secondary_color_dark: self.secondary_color_dark.as_deref(),
            font_family: self.font_family.as_deref(),
            font_size_base: self.font_size_base.as_deref(),
            border_radius: self.border_radius.as_deref(),
        }
    }
}

fn caller(auth: &AuthUser) -> Result<Uuid, Box<Response>> {
    Uuid::parse_str(&auth.0.sub).map_err(|_| {
        Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response(),
        )
    })
}

fn access_error(error: workspace_themes::AccessError) -> Box<Response> {
    match error {
        workspace_themes::AccessError::Forbidden(message) => {
            Box::new((StatusCode::FORBIDDEN, Json(ErrorResponse::new(message))).into_response())
        }
        workspace_themes::AccessError::NotFound(message) => {
            Box::new((StatusCode::NOT_FOUND, Json(ErrorResponse::new(message))).into_response())
        }
        workspace_themes::AccessError::Database(error) => {
            tracing::error!("Database error: {error}");
            Box::new(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response(),
            )
        }
    }
}

/// GET /api/workspaces/:id/theme
#[derive(Debug, Serialize)]
struct SingleThemeResponse {
    theme: ThemeResponse,
}

pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match workspace_themes::get_authorized(state.db(), workspace_id, user_id).await {
        Ok(Some(theme)) => Json(SingleThemeResponse {
            theme: ThemeResponse::from(theme),
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Theme not found")),
        )
            .into_response(),
        Err(error) => *access_error(error),
    }
}

/// PUT /api/workspaces/:id/theme
pub async fn upsert(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(req): Json<UpdateThemeRequest>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match workspace_themes::upsert_authorized(state.db(), workspace_id, user_id, req.update()).await
    {
        Ok(theme) => Json(SingleThemeResponse {
            theme: ThemeResponse::from(theme),
        })
        .into_response(),
        Err(error) => *access_error(error),
    }
}

/// DELETE /api/workspaces/:id/theme
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match workspace_themes::delete_authorized(state.db(), workspace_id, user_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Theme not found")),
        )
            .into_response(),
        Err(error) => *access_error(error),
    }
}
