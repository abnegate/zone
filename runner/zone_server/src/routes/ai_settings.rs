//! AI provider settings endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
use zone_core::SecretValue;

use crate::auth::AuthUser;
use crate::db::ai_settings;
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

/// AI settings response (credentials redacted)
#[derive(Debug, Serialize)]
pub struct AiSettingsResponse {
    pub provider: String,
    pub has_litellm_key: bool,
    pub litellm_host: Option<String>,
    pub has_openai_api_key: bool,
    pub openai_base_url: Option<String>,
    pub has_anthropic_api_key: bool,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_use_iam_role: bool,
    pub has_bedrock_credentials: bool,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub model_audio: Option<String>,
}

impl From<ai_settings::OrgAiSettingsRow> for AiSettingsResponse {
    fn from(row: ai_settings::OrgAiSettingsRow) -> Self {
        Self {
            provider: row.provider,
            has_litellm_key: row.litellm_key.is_some(),
            litellm_host: row.litellm_host,
            has_openai_api_key: row.openai_api_key.is_some(),
            openai_base_url: row.openai_base_url,
            has_anthropic_api_key: row.anthropic_api_key.is_some(),
            anthropic_base_url: row.anthropic_base_url,
            bedrock_region: row.bedrock_region,
            bedrock_use_iam_role: row.bedrock_use_iam_role.unwrap_or(false),
            has_bedrock_credentials: row.bedrock_access_key.is_some()
                && row.bedrock_secret_key.is_some(),
            model_fast: row.model_fast,
            model_reasoning: row.model_reasoning,
            model_embedding: row.model_embedding,
            model_image: row.model_image,
            model_video: row.model_video,
            model_audio: row.model_audio,
        }
    }
}

impl From<ai_settings::WorkspaceAiSettingsRow> for AiSettingsResponse {
    fn from(row: ai_settings::WorkspaceAiSettingsRow) -> Self {
        Self {
            provider: row
                .provider
                .unwrap_or_else(|| PROVIDER_SELF_HOSTED.to_string()),
            has_litellm_key: row.litellm_key.is_some(),
            litellm_host: row.litellm_host,
            has_openai_api_key: row.openai_api_key.is_some(),
            openai_base_url: row.openai_base_url,
            has_anthropic_api_key: row.anthropic_api_key.is_some(),
            anthropic_base_url: row.anthropic_base_url,
            bedrock_region: row.bedrock_region,
            bedrock_use_iam_role: row.bedrock_use_iam_role.unwrap_or(false),
            has_bedrock_credentials: row.bedrock_access_key.is_some()
                && row.bedrock_secret_key.is_some(),
            model_fast: row.model_fast,
            model_reasoning: row.model_reasoning,
            model_embedding: row.model_embedding,
            model_image: row.model_image,
            model_video: row.model_video,
            model_audio: row.model_audio,
        }
    }
}

impl From<ai_settings::EffectiveAiSettings> for AiSettingsResponse {
    fn from(settings: ai_settings::EffectiveAiSettings) -> Self {
        Self {
            provider: settings.provider,
            has_litellm_key: settings.litellm_key.is_some(),
            litellm_host: settings.litellm_host,
            has_openai_api_key: settings.openai_api_key.is_some(),
            openai_base_url: settings.openai_base_url,
            has_anthropic_api_key: settings.anthropic_api_key.is_some(),
            anthropic_base_url: settings.anthropic_base_url,
            bedrock_region: settings.bedrock_region,
            bedrock_use_iam_role: settings.bedrock_use_iam_role,
            has_bedrock_credentials: settings.bedrock_access_key.is_some()
                && settings.bedrock_secret_key.is_some(),
            model_fast: settings.model_fast,
            model_reasoning: settings.model_reasoning,
            model_embedding: settings.model_embedding,
            model_image: settings.model_image,
            model_video: settings.model_video,
            model_audio: settings.model_audio,
        }
    }
}

/// Update AI settings request
#[derive(Debug, Deserialize)]
pub struct UpdateAiSettingsRequest {
    pub provider: Option<String>,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<SecretValue>,
    pub openai_api_key: Option<SecretValue>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<SecretValue>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<SecretValue>,
    pub bedrock_secret_key: Option<SecretValue>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub model_audio: Option<String>,
}

impl UpdateAiSettingsRequest {
    fn update(&self) -> ai_settings::Update<'_> {
        ai_settings::Update {
            provider: self.provider.as_deref(),
            litellm_host: self.litellm_host.as_deref(),
            litellm_key: self.litellm_key.as_deref(),
            openai_api_key: self.openai_api_key.as_deref(),
            openai_base_url: self.openai_base_url.as_deref(),
            anthropic_api_key: self.anthropic_api_key.as_deref(),
            anthropic_base_url: self.anthropic_base_url.as_deref(),
            bedrock_region: self.bedrock_region.as_deref(),
            bedrock_access_key: self.bedrock_access_key.as_deref(),
            bedrock_secret_key: self.bedrock_secret_key.as_deref(),
            bedrock_use_iam_role: self.bedrock_use_iam_role,
            model_fast: self.model_fast.as_deref(),
            model_reasoning: self.model_reasoning.as_deref(),
            model_embedding: self.model_embedding.as_deref(),
            model_image: self.model_image.as_deref(),
            model_video: self.model_video.as_deref(),
            model_audio: self.model_audio.as_deref(),
        }
    }
}

fn denied(status: StatusCode, message: &str) -> Box<Response> {
    Box::new((status, Json(ErrorResponse::new(message))).into_response())
}

fn database_error(error: impl std::fmt::Display) -> Box<Response> {
    tracing::error!("Database error: {error}");
    denied(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
}

fn caller(auth: &AuthUser) -> Result<Uuid, Box<Response>> {
    Uuid::parse_str(&auth.0.sub)
        .map_err(|_| denied(StatusCode::UNAUTHORIZED, "Invalid user ID in token"))
}

fn access_error(error: ai_settings::AccessError) -> Box<Response> {
    match error {
        ai_settings::AccessError::Forbidden(message) => denied(StatusCode::FORBIDDEN, message),
        ai_settings::AccessError::NotFound(message) => denied(StatusCode::NOT_FOUND, message),
        ai_settings::AccessError::Invalid(message) => denied(StatusCode::BAD_REQUEST, &message),
        ai_settings::AccessError::Database(error) => database_error(error),
    }
}

/// GET /api/organizations/{org_id}/settings/ai
pub async fn get_org(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::get_org_authorized(state.db(), org_id, user_id).await {
        Ok(Some(settings)) => Json(AiSettingsResponse::from(settings)).into_response(),
        Ok(None) => {
            // Return default settings if none exist
            Json(AiSettingsResponse {
                provider: "self_hosted".to_string(),
                has_litellm_key: false,
                litellm_host: None,
                has_openai_api_key: false,
                openai_base_url: None,
                has_anthropic_api_key: false,
                anthropic_base_url: None,
                bedrock_region: None,
                bedrock_use_iam_role: false,
                has_bedrock_credentials: false,
                model_fast: None,
                model_reasoning: None,
                model_embedding: None,
                model_image: None,
                model_video: None,
                model_audio: None,
            })
            .into_response()
        }
        Err(error) => *access_error(error),
    }
}

/// PUT /api/organizations/{org_id}/settings/ai
pub async fn upsert_org(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::upsert_org_authorized(state.db(), org_id, user_id, req.update()).await {
        Ok(settings) => Json(AiSettingsResponse::from(settings)).into_response(),
        Err(error) => *access_error(error),
    }
}

/// DELETE /api/organizations/{org_id}/settings/ai
pub async fn delete_org(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::delete_org_authorized(state.db(), org_id, user_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("AI settings not found")),
        )
            .into_response(),
        Err(error) => *access_error(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceAiSettingsPath {
    pub org_id: Uuid,
    pub ws_id: Uuid,
}

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai
pub async fn get_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::get_workspace_authorized(state.db(), path.org_id, path.ws_id, user_id).await
    {
        Ok(Some(settings)) => Json(AiSettingsResponse::from(settings)).into_response(),
        Ok(None) => {
            // Return empty response indicating workspace inherits from org
            Json(AiSettingsResponse {
                provider: "self_hosted".to_string(),
                has_litellm_key: false,
                litellm_host: None,
                has_openai_api_key: false,
                openai_base_url: None,
                has_anthropic_api_key: false,
                anthropic_base_url: None,
                bedrock_region: None,
                bedrock_use_iam_role: false,
                has_bedrock_credentials: false,
                model_fast: None,
                model_reasoning: None,
                model_embedding: None,
                model_image: None,
                model_video: None,
                model_audio: None,
            })
            .into_response()
        }
        Err(error) => *access_error(error),
    }
}

/// PUT /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai
pub async fn upsert_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::upsert_workspace_authorized(
        state.db(),
        path.org_id,
        path.ws_id,
        user_id,
        req.update(),
    )
    .await
    {
        Ok(settings) => Json(AiSettingsResponse::from(settings)).into_response(),
        Err(error) => *access_error(error),
    }
}

/// DELETE /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai
pub async fn delete_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::delete_workspace_authorized(state.db(), path.org_id, path.ws_id, user_id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("AI settings not found")),
        )
            .into_response(),
        Err(error) => *access_error(error),
    }
}

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai/effective
pub async fn get_effective(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    let user_id = match caller(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match ai_settings::get_effective_authorized(state.db(), path.org_id, path.ws_id, user_id).await
    {
        Ok(settings) => Json(AiSettingsResponse::from(settings)).into_response(),
        Err(error) => *access_error(error),
    }
}
