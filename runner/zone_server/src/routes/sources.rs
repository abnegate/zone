//! Source endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::ErrorResponse;
use crate::auth::AuthUser;
use crate::db::sources;
use crate::error::ServerError;
use crate::state::AppState;

/// Allowed source types
const ALLOWED_SOURCE_TYPES: &[&str] = &["github", "gitlab", "filesystem", "notion", "text"];

/// Maximum name length
const MAX_NAME_LENGTH: usize = 256;

/// Maximum config size in bytes
const MAX_CONFIG_SIZE: usize = 65536;

/// Sanitize verification errors before returning to clients
fn sanitize_verification_error(e: &zone_context::error::ContextError) -> String {
    use zone_context::error::ContextError;
    match e {
        ContextError::Auth(_) => "Authentication failed - check your credentials".to_string(),
        ContextError::CredentialsRequired(_) => {
            "Credentials are required for this source".to_string()
        }
        ContextError::RateLimited { .. } => "Rate limited - please try again later".to_string(),
        ContextError::Network(_) | ContextError::Timeout { .. } => {
            "Network error - source unreachable".to_string()
        }
        ContextError::PermissionDenied(_) => {
            "Permission denied - check access permissions".to_string()
        }
        ContextError::InvalidSourceConfig(msg) => format!("Invalid configuration: {}", msg),
        ContextError::SourceNotFound(_) => {
            "Resource not found - check your configuration".to_string()
        }
        _ => "Verification failed - please check your source configuration".to_string(),
    }
}

/// Check if user has read access to workspace
async fn check_workspace_read_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: Uuid,
) -> Result<Uuid, ServerError> {
    let user_id = auth.0.user_id().map_err(|e| {
        tracing::error!("Failed to get user ID: {}", e);
        ServerError::Unauthorized("Invalid user".to_string())
    })?;

    match crate::db::workspace_members::can_read(state.db(), workspace_id, user_id).await {
        Ok(true) => Ok(user_id),
        Ok(false) => Err(ServerError::Forbidden(
            "Access denied to workspace".to_string(),
        )),
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            Err(ServerError::Internal("Internal server error".to_string()))
        }
    }
}

/// Check if user has write access to workspace
async fn check_workspace_write_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: Uuid,
) -> Result<Uuid, ServerError> {
    let user_id = auth.0.user_id().map_err(|e| {
        tracing::error!("Failed to get user ID: {}", e);
        ServerError::Unauthorized("Invalid user".to_string())
    })?;

    match crate::db::workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => Ok(user_id),
        Ok(false) => Err(ServerError::Forbidden(
            "Access denied to workspace".to_string(),
        )),
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            Err(ServerError::Internal("Internal server error".to_string()))
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
    last_verified_at: Option<chrono::NaiveDateTime>,
    workspace_id: Option<Uuid>,
    /// Indexing status
    index_status: sources::IndexStatus,
    /// Last indexed timestamp
    last_indexed_at: Option<chrono::NaiveDateTime>,
    /// Number of indexed items
    indexed_items_count: Option<i64>,
}

/// Verification result response
#[derive(Debug, Serialize)]
pub struct VerificationResponse {
    pub verified: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SourceResponse {
    /// Create a SourceResponse from a SourceRow with index status
    async fn from_row(state: &AppState, row: sources::SourceRow) -> Self {
        // Fetch index status
        let index_info = sources::get_source_index_status(state.db(), row.id)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to get index status for source {}: {}", row.id, e);
                sources::SourceIndexStatus {
                    status: sources::IndexStatus::Pending,
                    last_indexed_at: None,
                    indexed_items_count: None,
                }
            });

        Self {
            id: row.id,
            name: row.name,
            source_type: row.source_type,
            config: row.config,
            description: row.description,
            url: row.url,
            is_active: row.is_active.unwrap_or(true),
            last_error: row.last_error,
            last_verified_at: row.last_verified_at,
            workspace_id: row.workspace_id,
            index_status: index_info.status,
            last_indexed_at: index_info.last_indexed_at,
            indexed_items_count: index_info.indexed_items_count,
        }
    }

    /// Create multiple SourceResponses from SourceRows with index status
    async fn from_rows(state: &AppState, rows: Vec<sources::SourceRow>) -> Vec<Self> {
        let mut responses = Vec::with_capacity(rows.len());
        for row in rows {
            responses.push(Self::from_row(state, row).await);
        }
        responses
    }
}

/// Query parameters for listing sources
#[derive(Debug, Deserialize)]
pub struct ListSourcesQuery {
    source_type: Option<String>,
    is_active: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// Create source request
#[derive(Deserialize)]
pub struct CreateSourceRequest {
    name: String,
    source_type: String,
    config: serde_json::Value,
    description: Option<String>,
    url: Option<String>,
    credentials: Option<String>,
}

impl std::fmt::Debug for CreateSourceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateSourceRequest")
            .field("name", &self.name)
            .field("source_type", &self.source_type)
            .field("config", &self.config)
            .field("description", &self.description)
            .field("url", &self.url)
            .field(
                "credentials",
                &self.credentials.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Update source request
#[derive(Deserialize)]
pub struct UpdateSourceRequest {
    name: Option<String>,
    config: Option<serde_json::Value>,
    description: Option<String>,
    url: Option<String>,
    is_active: Option<bool>,
    credentials: Option<String>,
}

impl std::fmt::Debug for UpdateSourceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateSourceRequest")
            .field("name", &self.name)
            .field("config", &self.config)
            .field("description", &self.description)
            .field("url", &self.url)
            .field("is_active", &self.is_active)
            .field(
                "credentials",
                &self.credentials.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// GET /api/workspaces/:workspace_id/sources
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListSourcesQuery>,
) -> impl IntoResponse {
    // Check access
    if let Err(e) = check_workspace_read_access(&state, &auth, workspace_id).await {
        return e.into_response();
    }

    // Apply defaults for pagination
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    match sources::list_sources(
        state.db(),
        workspace_id,
        query.source_type.as_deref(),
        query.is_active,
        limit,
        offset,
    )
    .await
    {
        Ok(items) => {
            let responses = SourceResponse::from_rows(&state, items).await;
            Json(responses).into_response()
        }
        Err(e) => {
            tracing::error!(workspace_id = %workspace_id, error = %e, "Database error listing sources");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/workspaces/:workspace_id/sources
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(req): Json<CreateSourceRequest>,
) -> impl IntoResponse {
    // Check access
    let user_id = match check_workspace_write_access(&state, &auth, workspace_id).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    // Validate name length
    if req.name.is_empty() || req.name.len() > MAX_NAME_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Name must be between 1 and {} characters",
                MAX_NAME_LENGTH
            ))),
        )
            .into_response();
    }

    // Validate source_type
    if !ALLOWED_SOURCE_TYPES.contains(&req.source_type.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Invalid source_type. Must be one of: {}",
                ALLOWED_SOURCE_TYPES.join(", ")
            ))),
        )
            .into_response();
    }

    // Validate config size
    let config_bytes = serde_json::to_vec(&req.config).unwrap_or_default();
    if config_bytes.len() > MAX_CONFIG_SIZE {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Config size exceeds maximum of {} bytes",
                MAX_CONFIG_SIZE
            ))),
        )
            .into_response();
    }

    // Encrypt credentials before storing
    let credentials_encrypted = match req.credentials.as_ref() {
        Some(creds) => match crate::crypto::encrypt(state.encryption_key(), creds) {
            Ok(encrypted) => Some(encrypted),
            Err(e) => {
                tracing::error!("Encryption failed: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        },
        None => None,
    };

    let source_row = match sources::create_source(
        state.db(),
        workspace_id,
        &req.name,
        &req.source_type,
        req.config,
        req.description.as_deref(),
        req.url.as_deref(),
        credentials_encrypted.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(workspace_id = %workspace_id, name = %req.name, source_type = %req.source_type, error = %e, "Database error creating source");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Spawn background indexing if source is active
    if source_row.is_active.unwrap_or(true) {
        crate::workers::indexing::spawn_index_source(
            state.clone(),
            source_row.id,
            workspace_id,
            user_id,
            false, // is_update = false (initial index)
        );
    }

    let response = SourceResponse::from_row(&state, source_row).await;
    (StatusCode::CREATED, Json(response)).into_response()
}

/// GET /api/workspaces/:workspace_id/sources/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    // Check access
    if let Err(e) = check_workspace_read_access(&state, &auth, workspace_id).await {
        return e.into_response();
    }

    match sources::get_source(state.db(), id, workspace_id).await {
        Ok(Some(source)) => {
            let response = SourceResponse::from_row(&state, source).await;
            Json(response).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Source not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %e, "Database error getting source");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// PUT /api/workspaces/:workspace_id/sources/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateSourceRequest>,
) -> impl IntoResponse {
    // Check access
    let user_id = match check_workspace_write_access(&state, &auth, workspace_id).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    // Get old source for comparison
    let old_source = match sources::get_source(state.db(), id, workspace_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Source not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %e, "Database error getting source for update");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Update the source metadata
    let updated_source = match sources::update_source(
        state.db(),
        id,
        workspace_id,
        req.name.as_deref(),
        req.config.clone(),
        req.description.as_deref(),
        req.url.as_deref(),
        req.is_active,
    )
    .await
    {
        Ok(Some(source)) => source,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Source not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %e, "Database error updating source");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Track if credentials changed
    let mut credentials_changed = false;

    // If credentials provided, encrypt and update them
    if let Some(creds) = &req.credentials {
        let encrypted = match crate::crypto::encrypt(state.encryption_key(), creds) {
            Ok(encrypted) => encrypted,
            Err(e) => {
                tracing::error!("Encryption failed: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        };

        if let Err(e) = sources::update_credentials(state.db(), id, workspace_id, &encrypted).await
        {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %e, "Database error updating credentials");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }

        credentials_changed = true;
    }

    // Check if re-index needed
    let config_changed = req.config.is_some()
        && crate::workers::indexing::config_changed(&old_source.config, &updated_source.config);

    let needs_reindex =
        (config_changed || credentials_changed) && updated_source.is_active.unwrap_or(true);

    if needs_reindex {
        crate::workers::indexing::spawn_index_source(
            state.clone(),
            id,
            workspace_id,
            user_id,
            true, // is_update = true (re-index)
        );
    }

    let response = SourceResponse::from_row(&state, updated_source).await;
    Json(response).into_response()
}

/// DELETE /api/workspaces/:workspace_id/sources/:id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    // Check access
    if let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await {
        return e.into_response();
    }

    match sources::delete_source(state.db(), id, workspace_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Source not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %e, "Database error deleting source");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/workspaces/:workspace_id/sources/:id/verify
pub async fn verify(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    // Check access
    if let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await {
        return e.into_response();
    }

    // Get the source from database
    let source_row = match sources::get_source(state.db(), id, workspace_id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Source not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %e, "Database error getting source for verification");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Get the adapter registry
    let adapter_registry = match state.adapter_registry() {
        Some(registry) => registry,
        None => {
            tracing::error!("Adapter registry not available");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse::new("Verification service unavailable")),
            )
                .into_response();
        }
    };

    // Get the adapter for this source type
    let adapter = match adapter_registry.get(&source_row.source_type) {
        Some(adapter) => adapter,
        None => {
            let error_msg = format!("No adapter for source type: {}", source_row.source_type);
            // Update error in database
            if let Err(db_err) =
                sources::update_verification(state.db(), id, workspace_id, Some(&error_msg)).await
            {
                tracing::error!(source_id = %id, error = %db_err, "Failed to update verification status");
            }
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(&error_msg)),
            )
                .into_response();
        }
    };

    // Convert SourceRow to zone_core::Source
    // We need to parse the source_type string into zone_core::SourceType enum
    let source_type = match source_row.source_type.as_str() {
        "github" => zone_core::SourceType::GitHub,
        "gitlab" => zone_core::SourceType::GitLab,
        "filesystem" => zone_core::SourceType::Filesystem,
        "notion" => zone_core::SourceType::Notion,
        "text" => zone_core::SourceType::Text,
        _ => {
            let error_msg = format!("Unknown source type: {}", source_row.source_type);
            if let Err(db_err) =
                sources::update_verification(state.db(), id, workspace_id, Some(&error_msg)).await
            {
                tracing::error!(source_id = %id, error = %db_err, "Failed to update verification status");
            }
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(&error_msg)),
            )
                .into_response();
        }
    };

    let mut config = source_row.config.clone();

    // Decrypt and inject credentials if present
    if let Some(encrypted_creds) = &source_row.credentials_encrypted {
        match crate::crypto::decrypt(state.encryption_key(), encrypted_creds) {
            Ok(decrypted) => {
                if let Some(config_obj) = config.as_object_mut() {
                    // Use source-type-specific credential field names
                    let cred_field = match source_row.source_type.as_str() {
                        "github" | "gitlab" => "token",
                        "notion" => "api_key",
                        _ => "token",
                    };
                    config_obj.insert(cred_field.to_string(), serde_json::Value::String(decrypted));
                }
            }
            Err(e) => {
                tracing::error!(source_id = %id, error = %e, "Failed to decrypt credentials");
                let error_msg = "Failed to decrypt credentials";
                if let Err(db_err) =
                    sources::update_verification(state.db(), id, workspace_id, Some(error_msg))
                        .await
                {
                    tracing::error!(source_id = %id, error = %db_err, "Failed to update verification status");
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(error_msg)),
                )
                    .into_response();
            }
        }
    }

    let source = zone_core::Source {
        id: source_row.id,
        name: source_row.name.clone(),
        source_type,
        category: source_type.category(),
        config,
        is_active: source_row.is_active.unwrap_or(true),
        last_synced_at: source_row
            .last_verified_at
            .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc)),
        created_at: source_row
            .created_at
            .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc))
            .unwrap_or_else(chrono::Utc::now),
        updated_at: source_row
            .updated_at
            .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc))
            .unwrap_or_else(chrono::Utc::now),
    };

    // Call the adapter's verify method
    match adapter.verify(&source).await {
        Ok(()) => {
            // Verification succeeded - update timestamp, clear error
            match sources::update_verification(state.db(), id, workspace_id, None).await {
                Ok(_) => Json(VerificationResponse {
                    verified: true,
                    message: "Source verified successfully".to_string(),
                    error: None,
                })
                .into_response(),
                Err(e) => {
                    tracing::error!(source_id = %id, error = %e, "Failed to update verification status");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new(
                            "Verification succeeded but failed to update status",
                        )),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            let internal_msg = e.to_string();
            tracing::warn!(source_id = %id, error = %internal_msg, "Source verification failed");
            if let Err(db_err) =
                sources::update_verification(state.db(), id, workspace_id, Some(&internal_msg))
                    .await
            {
                tracing::error!(source_id = %id, error = %db_err, "Failed to update verification status");
            }

            // Sanitize error for client - don't leak internal details
            let user_msg = sanitize_verification_error(&e);

            Json(VerificationResponse {
                verified: false,
                message: user_msg,
                error: None, // Remove raw error from response
            })
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

/// POST /api/workspaces/:workspace_id/sources/:source_id/reindex
/// Manually trigger re-indexing of a source
pub async fn reindex(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, source_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    // Verify access
    let user_id = match check_workspace_write_access(&state, &auth, workspace_id).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    // Check for in-progress indexing
    let index_status = sources::get_source_index_status(state.db(), source_id).await;
    if let Ok(status) = &index_status {
        if matches!(status.status, sources::IndexStatus::Indexing) {
            return (
                StatusCode::CONFLICT,
                Json(ErrorResponse::new("Source is already being indexed")),
            )
                .into_response();
        }
    }

    // Verify source exists
    match sources::get_source(state.db(), source_id, workspace_id).await {
        Ok(Some(_)) => {
            // Spawn re-index
            crate::workers::indexing::spawn_index_source(
                state.clone(),
                source_id,
                workspace_id,
                user_id,
                true, // force re-index
            );

            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "message": "Re-indexing started",
                    "source_id": source_id
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Source not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(source_id = %source_id, workspace_id = %workspace_id, error = %e, "Database error getting source for reindex");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
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
            name: "filesystem".to_string(),
            display_name: "Local Directory".to_string(),
            category: "filesystem".to_string(),
            description: "Local filesystem (self-hosted only)".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "base_path": {"type": "string", "description": "Absolute path to project root"},
                    "allow_writes": {"type": "boolean", "description": "Allow write operations", "default": true}
                },
                "required": ["base_path"]
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
