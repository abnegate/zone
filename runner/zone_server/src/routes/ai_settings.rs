//! AI provider settings endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_context::embeddings::providers::{
    PROVIDER_BEDROCK, PROVIDER_OPENAI, PROVIDER_SELF_HOSTED,
};

use crate::auth::AuthUser;
use crate::db::{ai_settings, organization_members, workspace_members};
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
    pub model_image: Option<String>,
    pub model_video: Option<String>,
    pub model_audio: Option<String>,
}

// ============================================================================
// Organization AI Settings Endpoints
// ============================================================================

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

/// These settings name the host every model call is sent to and hold the key
/// sent with it, so an unscoped write here does not just corrupt a tenant's
/// configuration -- it redirects their chat and knowledge text, and their
/// provider key, to a host of the caller's choosing. Reading is limited to
/// members; writing to admins, matching who may change billing.
async fn authorize_org(
    state: &AppState,
    auth: &AuthUser,
    org_id: Uuid,
    write: bool,
) -> Result<(), Box<Response>> {
    let user_id = caller(auth)?;

    let permitted = if write {
        organization_members::is_admin(state.db(), org_id, user_id).await
    } else {
        organization_members::is_member(state.db(), org_id, user_id).await
    }
    .map_err(database_error)?;

    if permitted {
        Ok(())
    } else if write {
        Err(denied(
            StatusCode::FORBIDDEN,
            "Only organization admins can change AI settings",
        ))
    } else {
        Err(denied(StatusCode::NOT_FOUND, "Organization not found"))
    }
}

/// The workspace has to be checked as well as the organization: the path
/// carries both, and nothing else ties the two together, so an admin of their
/// own organization could otherwise name any workspace id in the world.
async fn authorize_workspace(
    state: &AppState,
    auth: &AuthUser,
    org_id: Uuid,
    workspace_id: Uuid,
    write: bool,
) -> Result<(), Box<Response>> {
    authorize_org(state, auth, org_id, write).await?;

    let user_id = caller(auth)?;
    let member = workspace_members::is_member(state.db(), user_id, workspace_id)
        .await
        .map_err(database_error)?;

    if member {
        Ok(())
    } else {
        Err(denied(StatusCode::NOT_FOUND, "Workspace not found"))
    }
}

/// GET /api/organizations/{org_id}/settings/ai
pub async fn get_org(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(response) = authorize_org(&state, &auth, org_id, false).await {
        return *response;
    }
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
                model_image: None,
                model_video: None,
                model_audio: None,
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
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    if let Err(response) = authorize_org(&state, &auth, org_id, true).await {
        return *response;
    }
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
        req.model_image.as_deref(),
        req.model_video.as_deref(),
        req.model_audio.as_deref(),
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
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(response) = authorize_org(&state, &auth, org_id, true).await {
        return *response;
    }
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
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    if let Err(response) = authorize_workspace(&state, &auth, path.org_id, path.ws_id, false).await
    {
        return *response;
    }
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
                model_image: None,
                model_video: None,
                model_audio: None,
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
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> impl IntoResponse {
    if let Err(response) = authorize_workspace(&state, &auth, path.org_id, path.ws_id, true).await {
        return *response;
    }
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
        req.model_image.as_deref(),
        req.model_video.as_deref(),
        req.model_audio.as_deref(),
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
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    if let Err(response) = authorize_workspace(&state, &auth, path.org_id, path.ws_id, true).await {
        return *response;
    }
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
    auth: AuthUser,
    Path(path): Path<WorkspaceAiSettingsPath>,
) -> impl IntoResponse {
    if let Err(response) = authorize_workspace(&state, &auth, path.org_id, path.ws_id, false).await
    {
        return *response;
    }
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
