//! AI provider settings endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_context::embeddings::providers::{
    PROVIDER_BEDROCK, PROVIDER_OPENAI, PROVIDER_SELF_HOSTED,
};

use crate::auth::AuthUser;
use crate::db::ai_settings;
use crate::state::AppState;

// Anthropic provider constant (not in zone_context yet)
const PROVIDER_ANTHROPIC: &str = "anthropic";

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
        }
    }
}

/// Update AI settings request
#[derive(Debug, Deserialize)]
pub struct UpdateAiSettingsRequest {
    pub provider: Option<String>,
    pub litellm_host: Option<String>,
    pub litellm_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub bedrock_region: Option<String>,
    pub bedrock_access_key: Option<String>,
    pub bedrock_secret_key: Option<String>,
    pub bedrock_use_iam_role: Option<bool>,
    pub model_fast: Option<String>,
    pub model_reasoning: Option<String>,
    pub model_embedding: Option<String>,
}

// ============================================================================
// Organization AI Settings Endpoints
// ============================================================================

/// GET /api/organizations/{org_id}/settings/ai
pub async fn get_org(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    match ai_settings::get_org_ai_settings(state.db(), org_id).await {
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
            })
            .into_response()
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

/// PUT /api/organizations/{org_id}/settings/ai
pub async fn upsert_org(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    // Validate provider if provided
    if let Some(ref provider) = req.provider
        && ![
            PROVIDER_SELF_HOSTED,
            PROVIDER_OPENAI,
            PROVIDER_ANTHROPIC,
            PROVIDER_BEDROCK,
        ]
        .contains(&provider.as_str())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Invalid provider. Must be one of: {}, {}, {}, {}",
                PROVIDER_SELF_HOSTED, PROVIDER_OPENAI, PROVIDER_ANTHROPIC, PROVIDER_BEDROCK
            ))),
        )
            .into_response();
    }

    match ai_settings::upsert_org_ai_settings(
        state.db(),
        org_id,
        req.provider.as_deref(),
        req.litellm_host.as_deref(),
        req.litellm_key.as_deref(),
        req.openai_api_key.as_deref(),
        req.openai_base_url.as_deref(),
        req.anthropic_api_key.as_deref(),
        req.anthropic_base_url.as_deref(),
        req.bedrock_region.as_deref(),
        req.bedrock_access_key.as_deref(),
        req.bedrock_secret_key.as_deref(),
        req.bedrock_use_iam_role,
        req.model_fast.as_deref(),
        req.model_reasoning.as_deref(),
        req.model_embedding.as_deref(),
    )
    .await
    {
        Ok(settings) => Json(AiSettingsResponse::from(settings)).into_response(),
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

/// DELETE /api/organizations/{org_id}/settings/ai
pub async fn delete_org(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    match ai_settings::delete_org_ai_settings(state.db(), org_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("AI settings not found")),
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

// ============================================================================
// Workspace AI Settings Endpoints
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct WorkspaceAiSettingsPath {
    pub org_id: Uuid,
    pub ws_id: Uuid,
}

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai
pub async fn get_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    match ai_settings::get_workspace_ai_settings(state.db(), path.ws_id).await {
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
            })
            .into_response()
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

/// PUT /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai
pub async fn upsert_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    // Validate provider if provided
    if let Some(ref provider) = req.provider
        && ![
            PROVIDER_SELF_HOSTED,
            PROVIDER_OPENAI,
            PROVIDER_ANTHROPIC,
            PROVIDER_BEDROCK,
        ]
        .contains(&provider.as_str())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Invalid provider. Must be one of: {}, {}, {}, {}",
                PROVIDER_SELF_HOSTED, PROVIDER_OPENAI, PROVIDER_ANTHROPIC, PROVIDER_BEDROCK
            ))),
        )
            .into_response();
    }

    match ai_settings::upsert_workspace_ai_settings(
        state.db(),
        path.ws_id,
        req.provider.as_deref(),
        req.litellm_host.as_deref(),
        req.litellm_key.as_deref(),
        req.openai_api_key.as_deref(),
        req.openai_base_url.as_deref(),
        req.anthropic_api_key.as_deref(),
        req.anthropic_base_url.as_deref(),
        req.bedrock_region.as_deref(),
        req.bedrock_access_key.as_deref(),
        req.bedrock_secret_key.as_deref(),
        req.bedrock_use_iam_role,
        req.model_fast.as_deref(),
        req.model_reasoning.as_deref(),
        req.model_embedding.as_deref(),
    )
    .await
    {
        Ok(settings) => Json(AiSettingsResponse::from(settings)).into_response(),
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

/// DELETE /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai
pub async fn delete_workspace(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    match ai_settings::delete_workspace_ai_settings(state.db(), path.ws_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("AI settings not found")),
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

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/settings/ai/effective
pub async fn get_effective(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    match ai_settings::get_effective_ai_settings(state.db(), path.org_id, path.ws_id).await {
        Ok(settings) => Json(AiSettingsResponse::from(settings)).into_response(),
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
