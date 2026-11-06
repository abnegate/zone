//! Source endpoints

use abnegate_vcs::pull_request::{ApplicationError, PullRequestError, Repository};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::common::Timestamps;
use super::error::ErrorResponse;
use crate::auth::AuthUser;
use crate::db::github_installations::{self, InstallationStatus, InstallationSummary};
use crate::db::sources::{self, GITHUB, SourceChanges, UpdateError};
use crate::error::ServerError;
use crate::services::github_app;
use crate::state::AppState;

/// Maximum name length
const MAX_NAME_LENGTH: usize = 256;

/// Maximum config size in bytes
const MAX_CONFIG_SIZE: usize = 65536;

const INTERNAL_ERROR: &str = "Internal server error";

const SOURCE_NOT_FOUND: &str = "Source not found";

const CREDENTIALS_AND_INSTALLATION: &str =
    "Send either credentials or github_installation_id, not both.";

const INSTALLATION_REQUIRES_GITHUB: &str = "Only GitHub sources can use a GitHub connection.";

const CONNECTION_LOST: &str = "The GitHub connection was removed or suspended. Reconnect GitHub.";

const CREDENTIALS_UNREADABLE: &str = "The source's credentials could not be read.";

const UNREACHABLE: &str = "Zone can't reach this repository through that GitHub connection.";

const GITHUB_UNAVAILABLE: &str = "GitHub could not be reached.";

const APP_REJECTED: &str = "The GitHub App connection is broken. Reconnect GitHub.";

const NOTION: &str = "notion";

const TOKEN_FIELD: &str = "token";

const API_KEY_FIELD: &str = "api_key";

fn internal() -> ServerError {
    ServerError::Internal(INTERNAL_ERROR.to_string())
}

fn not_found() -> ServerError {
    ServerError::NotFound(SOURCE_NOT_FOUND.to_string())
}

fn encrypt_credentials(state: &AppState, credentials: &str) -> Result<String, ServerError> {
    crate::crypto::encrypt(state.encryption_key(), credentials).map_err(|error| {
        tracing::error!("Encryption failed: {}", error);
        internal()
    })
}

/// Refuses an installation a source in `workspace_id` may not be backed by.
async fn validate_installation(
    state: &AppState,
    workspace_id: Uuid,
    source_type: &str,
    installation: Uuid,
    with_credentials: bool,
) -> Result<(), ServerError> {
    if with_credentials {
        return Err(ServerError::BadRequest(
            CREDENTIALS_AND_INSTALLATION.to_string(),
        ));
    }
    if source_type != GITHUB {
        return Err(ServerError::BadRequest(
            INSTALLATION_REQUIRES_GITHUB.to_string(),
        ));
    }
    match github_installations::attachable(state.db(), workspace_id, installation).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(ServerError::BadRequest(
            github_installations::UNATTACHABLE.to_string(),
        )),
        Err(error) => {
            tracing::error!(workspace_id = %workspace_id, installation = %installation, error = %error, "Database error checking GitHub installation");
            Err(internal())
        }
    }
}

/// Refuses an installation whose token cannot read the repository `config` names.
async fn validate_reachable(
    state: &AppState,
    installation: Uuid,
    config: &serde_json::Value,
) -> Result<(), ServerError> {
    let repository =
        configured_repository(config).map_err(|error| repository_refusal(installation, error))?;
    let service = state.config().pull_requests().map_err(|error| {
        tracing::error!(error = %error, "The GitHub API origin is not usable");
        internal()
    })?;
    let credential = state
        .github()
        .installation(state.db(), installation)
        .await
        .map_err(|error| installation_refusal(installation, error))?;

    let service = &service;
    let repository = &repository;
    state
        .github()
        .retrying(state.db(), credential, |token| async move {
            service.get_default_branch(repository, &token).await
        })
        .await
        .map(|_| ())
        .map_err(|error| repository_refusal(installation, error))
}

fn configured_repository(config: &serde_json::Value) -> Result<Repository, PullRequestError> {
    let field = |name: &str| {
        config
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    };
    Repository::new(field("owner"), field("repo"))
}

fn unreachable() -> ServerError {
    ServerError::BadRequest(UNREACHABLE.to_string())
}

fn unavailable() -> ServerError {
    ServerError::BadGateway(GITHUB_UNAVAILABLE.to_string())
}

fn refused_by_github(status: u16) -> bool {
    StatusCode::from_u16(status)
        .is_ok_and(|status| status.is_client_error() && status != StatusCode::TOO_MANY_REQUESTS)
}

/// What a failure to resolve a GitHub credential means to the person who asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialFailure {
    Disconnected,
    AppRejected,
    Refused,
    Unavailable,
    Unreadable,
    Internal,
}

impl CredentialFailure {
    fn of(error: &github_app::Error) -> Self {
        match error {
            github_app::Error::Revoked
            | github_app::Error::Suspended
            | github_app::Error::NotConnected => Self::Disconnected,
            github_app::Error::Application(ApplicationError::Status(status))
                if *status == StatusCode::UNAUTHORIZED.as_u16() =>
            {
                Self::AppRejected
            }
            github_app::Error::Application(ApplicationError::Status(status))
                if refused_by_github(*status) =>
            {
                Self::Refused
            }
            github_app::Error::Application(
                ApplicationError::Status(_)
                | ApplicationError::Transport(_)
                | ApplicationError::Response,
            ) => Self::Unavailable,
            github_app::Error::Credential | github_app::Error::Unsealing => Self::Unreadable,
            _ => Self::Internal,
        }
    }
}

fn installation_refusal(installation: Uuid, error: github_app::Error) -> ServerError {
    match CredentialFailure::of(&error) {
        CredentialFailure::Disconnected => {
            ServerError::BadRequest(github_installations::UNATTACHABLE.to_string())
        }
        CredentialFailure::AppRejected => {
            tracing::warn!(installation = %installation, error = %error, "GitHub refused the GitHub App's credentials");
            ServerError::BadRequest(APP_REJECTED.to_string())
        }
        CredentialFailure::Refused => unreachable(),
        CredentialFailure::Unavailable => {
            tracing::warn!(installation = %installation, error = %error, "GitHub could not mint an installation token");
            unavailable()
        }
        CredentialFailure::Unreadable | CredentialFailure::Internal => {
            tracing::error!(installation = %installation, error = %error, "An installation token could not be minted");
            internal()
        }
    }
}

/// The status and message a verification answers, and records, when the
/// source's credential cannot be resolved.
fn verification_refusal(error: &github_app::Error) -> (StatusCode, &'static str) {
    match CredentialFailure::of(error) {
        CredentialFailure::Disconnected => (StatusCode::BAD_REQUEST, CONNECTION_LOST),
        CredentialFailure::AppRejected => (StatusCode::BAD_REQUEST, APP_REJECTED),
        CredentialFailure::Refused => (StatusCode::BAD_REQUEST, UNREACHABLE),
        CredentialFailure::Unavailable => (StatusCode::BAD_GATEWAY, GITHUB_UNAVAILABLE),
        CredentialFailure::Unreadable => {
            (StatusCode::INTERNAL_SERVER_ERROR, CREDENTIALS_UNREADABLE)
        }
        CredentialFailure::Internal => (StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_ERROR),
    }
}

fn repository_refusal(installation: Uuid, error: PullRequestError) -> ServerError {
    match error {
        PullRequestError::NotFound
        | PullRequestError::Forbidden
        | PullRequestError::AuthenticationFailed
        | PullRequestError::InvalidRepositoryName => unreachable(),
        PullRequestError::Http(error) => {
            tracing::warn!(installation = %installation, error = %error.without_url(), "GitHub could not be reached to read a repository");
            unavailable()
        }
        error => {
            tracing::warn!(installation = %installation, error = %error, "GitHub could not read a repository");
            unavailable()
        }
    }
}

async fn installation_summaries(
    state: &AppState,
    installations: &[Uuid],
) -> Result<HashMap<Uuid, InstallationSummary>, ServerError> {
    if installations.is_empty() {
        return Ok(HashMap::new());
    }
    let summaries = github_installations::summaries(state.db(), installations)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "Failed to load GitHub installation summaries");
            internal()
        })?;
    Ok(summaries
        .into_iter()
        .map(|summary| (summary.id, summary))
        .collect())
}

/// The kinds this server can verify and fetch, sorted, straight from the
/// adapter registry so nothing offers a kind that would never verify.
fn source_kinds(state: &AppState) -> Option<Vec<String>> {
    state.adapter_registry().map(|registry| {
        let mut kinds = registry.registered_types();
        kinds.sort();
        kinds
    })
}

fn unknown_kind_message(kind: &str, kinds: &[String]) -> String {
    format!(
        "Invalid source_type \"{}\". Must be one of: {}",
        kind,
        kinds.join(", ")
    )
}

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

/// Category for a persisted source_type string
fn category_for_source_type(source_type: &str) -> &'static str {
    match source_type {
        GITHUB | "gitlab" | "filesystem" => "file",
        "ical" => "calendar",
        "imap" => "mail",
        "discord" | "slack" => "chat",
        "web" => "web",
        "text" => "text",
        _ => "file",
    }
}

/// Prefer an explicit URL, otherwise derive one from source config
fn derive_source_url(
    source_type: &str,
    config: &serde_json::Value,
    explicit: Option<&str>,
) -> Option<String> {
    if let Some(url) = explicit.filter(|value| !value.is_empty()) {
        return Some(url.to_string());
    }

    match source_type {
        GITHUB => {
            let owner = config
                .get("owner")?
                .as_str()
                .filter(|value| !value.is_empty())?;
            let repo = config
                .get("repo")?
                .as_str()
                .filter(|value| !value.is_empty())?;
            Some(format!("https://github.com/{}/{}", owner, repo))
        }
        "gitlab" => {
            let project_id = config
                .get("project_id")?
                .as_str()
                .filter(|value| !value.is_empty())?;
            let host = config
                .get("host")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("https://gitlab.com")
                .trim_end_matches('/');
            Some(format!("{}/{}", host, project_id))
        }
        "web" | "ical" => config
            .get("url")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        _ => None,
    }
}

/// Source response
#[derive(Debug, Serialize)]
pub struct SourceResponse {
    id: Uuid,
    name: String,
    source_type: String,
    category: String,
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
    has_credentials: bool,
    github_installation_id: Option<Uuid>,
    github_installation_status: Option<InstallationStatus>,
    github_account_login: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

/// Verification result response
#[derive(Debug, Serialize)]
pub struct VerificationResponse {
    pub verified: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Sources list response
#[derive(Debug, Serialize)]
pub struct SourcesListResponse {
    sources: Vec<SourceResponse>,
}

/// Single source response wrapper
#[derive(Debug, Serialize)]
pub struct SingleSourceResponse {
    source: SourceResponse,
}

/// Source types list response
#[derive(Debug, Serialize)]
pub struct SourceTypesListResponse {
    types: Vec<SourceTypeInfo>,
}

impl SourceResponse {
    async fn from_row(state: &AppState, row: sources::SourceRow) -> Result<Self, ServerError> {
        let summaries =
            installation_summaries(state, row.github_installation_id.as_slice()).await?;
        Ok(Self::build(state, row, &summaries).await)
    }

    /// Loads every installation the rows name in one query.
    async fn from_rows(
        state: &AppState,
        rows: Vec<sources::SourceRow>,
    ) -> Result<Vec<Self>, ServerError> {
        let mut installations: Vec<Uuid> = rows
            .iter()
            .filter_map(|row| row.github_installation_id)
            .collect();
        installations.sort_unstable();
        installations.dedup();
        let summaries = installation_summaries(state, &installations).await?;

        let mut responses = Vec::with_capacity(rows.len());
        for row in rows {
            responses.push(Self::build(state, row, &summaries).await);
        }
        Ok(responses)
    }

    async fn build(
        state: &AppState,
        row: sources::SourceRow,
        summaries: &HashMap<Uuid, InstallationSummary>,
    ) -> Self {
        let index_info = sources::get_source_index_status(state.db(), row.id)
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(source_id = %row.id, error = %error, "Failed to get index status for source");
                sources::SourceIndexStatus {
                    status: sources::IndexStatus::Pending,
                    last_indexed_at: None,
                    indexed_items_count: None,
                }
            });

        let url = derive_source_url(&row.source_type, &row.config, row.url.as_deref());
        let category = category_for_source_type(&row.source_type).to_string();
        let installation = row.github_installation_id.and_then(|id| summaries.get(&id));

        Self {
            id: row.id,
            name: row.name,
            source_type: row.source_type,
            category,
            config: row.config,
            description: row.description,
            url,
            is_active: row.is_active.unwrap_or(true),
            last_error: row.last_error,
            last_verified_at: row.last_verified_at,
            workspace_id: row.workspace_id,
            index_status: index_info.status,
            last_indexed_at: index_info.last_indexed_at,
            indexed_items_count: index_info.indexed_items_count,
            has_credentials: row.credentials_encrypted.is_some(),
            github_installation_id: row.github_installation_id,
            github_installation_status: installation.map(InstallationSummary::status),
            github_account_login: installation.map(|summary| summary.account_login.clone()),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
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
    github_installation_id: Option<Uuid>,
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
            .field("github_installation_id", &self.github_installation_id)
            .finish()
    }
}

/// Update source request. `github_installation_id` is absent to leave the
/// source as it is, `null` to detach the installation, or an id to attach it.
#[derive(Deserialize)]
pub struct UpdateSourceRequest {
    name: Option<String>,
    config: Option<serde_json::Value>,
    description: Option<String>,
    url: Option<String>,
    is_active: Option<bool>,
    credentials: Option<String>,
    #[serde(default, deserialize_with = "crate::db::actions::patch")]
    github_installation_id: Option<Option<Uuid>>,
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
            .field("github_installation_id", &self.github_installation_id)
            .finish()
    }
}

/// GET /api/workspaces/:workspace_id/sources
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListSourcesQuery>,
) -> Result<Response, ServerError> {
    check_workspace_read_access(&state, &auth, workspace_id).await?;

    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let rows = sources::list_sources(
        state.db(),
        workspace_id,
        query.source_type.as_deref(),
        query.is_active,
        limit,
        offset,
    )
    .await
    .map_err(|error| {
        tracing::error!(workspace_id = %workspace_id, error = %error, "Database error listing sources");
        internal()
    })?;
    let responses = SourceResponse::from_rows(&state, rows).await?;
    Ok(Json(SourcesListResponse { sources: responses }).into_response())
}

/// POST /api/workspaces/:workspace_id/sources
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CreateSourceRequest>,
) -> Result<Response, ServerError> {
    let user_id = check_workspace_write_access(&state, &auth, workspace_id).await?;

    if request.name.is_empty() || request.name.len() > MAX_NAME_LENGTH {
        return Err(ServerError::BadRequest(format!(
            "Name must be between 1 and {} characters",
            MAX_NAME_LENGTH
        )));
    }

    let Some(kinds) = source_kinds(&state) else {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse::new("Source kinds unavailable")),
        )
            .into_response());
    };
    if !kinds.contains(&request.source_type) {
        return Err(ServerError::BadRequest(unknown_kind_message(
            &request.source_type,
            &kinds,
        )));
    }

    let config_bytes = serde_json::to_vec(&request.config).unwrap_or_default();
    if config_bytes.len() > MAX_CONFIG_SIZE {
        return Err(ServerError::BadRequest(format!(
            "Config size exceeds maximum of {} bytes",
            MAX_CONFIG_SIZE
        )));
    }

    if let Some(installation) = request.github_installation_id {
        validate_installation(
            &state,
            workspace_id,
            &request.source_type,
            installation,
            request.credentials.is_some(),
        )
        .await?;
    }

    let credentials_encrypted = request
        .credentials
        .as_deref()
        .map(|credentials| encrypt_credentials(&state, credentials))
        .transpose()?;

    let url = derive_source_url(
        &request.source_type,
        &request.config,
        request.url.as_deref(),
    );

    let source = sources::create_source(
        state.db(),
        sources::NewSource {
            workspace_id,
            name: &request.name,
            source_type: &request.source_type,
            config: request.config,
            description: request.description.as_deref(),
            url: url.as_deref(),
            credentials_encrypted: credentials_encrypted.as_deref(),
            github_installation_id: request.github_installation_id,
        },
    )
    .await
    .map_err(|error| {
        tracing::error!(workspace_id = %workspace_id, name = %request.name, source_type = %request.source_type, error = %error, "Database error creating source");
        internal()
    })?;

    if source.is_active.unwrap_or(true) {
        crate::workers::indexing::spawn_index_source(
            state.clone(),
            source.id,
            workspace_id,
            user_id,
            false, // is_update = false (initial index)
        );
    }

    let response = SourceResponse::from_row(&state, source).await?;
    Ok((
        StatusCode::CREATED,
        Json(SingleSourceResponse { source: response }),
    )
        .into_response())
}

/// GET /api/workspaces/:workspace_id/sources/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ServerError> {
    check_workspace_read_access(&state, &auth, workspace_id).await?;

    let source = sources::get_source(state.db(), id, workspace_id)
        .await
        .map_err(|error| {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %error, "Database error getting source");
            internal()
        })?
        .ok_or_else(not_found)?;
    let response = SourceResponse::from_row(&state, source).await?;
    Ok(Json(SingleSourceResponse { source: response }).into_response())
}

/// PUT /api/workspaces/:workspace_id/sources/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateSourceRequest>,
) -> Result<Response, ServerError> {
    let user_id = check_workspace_write_access(&state, &auth, workspace_id).await?;

    let previous = sources::get_source(state.db(), id, workspace_id)
        .await
        .map_err(|error| {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %error, "Database error getting source for update");
            internal()
        })?
        .ok_or_else(not_found)?;

    if let Some(Some(installation)) = request.github_installation_id {
        validate_installation(
            &state,
            workspace_id,
            &previous.source_type,
            installation,
            request.credentials.is_some(),
        )
        .await?;
        validate_reachable(
            &state,
            installation,
            request.config.as_ref().unwrap_or(&previous.config),
        )
        .await?;
    }

    let credentials_encrypted = request
        .credentials
        .as_deref()
        .map(|credentials| encrypt_credentials(&state, credentials))
        .transpose()?;

    let current = sources::update_source(
        state.db(),
        id,
        workspace_id,
        SourceChanges {
            name: request.name.as_deref(),
            config: request.config.clone(),
            description: request.description.as_deref(),
            url: request.url.as_deref(),
            is_active: request.is_active,
            github_installation_id: request.github_installation_id,
            credentials_encrypted: credentials_encrypted.as_deref(),
        },
    )
    .await
    .map_err(|error| match error {
        UpdateError::NotFound => not_found(),
        UpdateError::Unattachable => {
            ServerError::BadRequest(github_installations::UNATTACHABLE.to_string())
        }
        UpdateError::Database(error) => {
            tracing::error!(source_id = %id, workspace_id = %workspace_id, error = %error, "Database error updating source");
            internal()
        }
    })?;

    let config_changed = request.config.is_some()
        && crate::workers::indexing::config_changed(&previous.config, &current.config);
    let credentials_changed = current.credentials_encrypted != previous.credentials_encrypted
        || current.github_installation_id != previous.github_installation_id;

    if (config_changed || credentials_changed) && current.is_active.unwrap_or(true) {
        crate::workers::indexing::spawn_index_source(
            state.clone(),
            id,
            workspace_id,
            user_id,
            true, // is_update = true (re-index)
        );
    }

    let response = SourceResponse::from_row(&state, current).await?;
    Ok(Json(SingleSourceResponse { source: response }).into_response())
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
            let mut kinds = adapter_registry.registered_types();
            kinds.sort();
            let error_msg = unknown_kind_message(&source_row.source_type, &kinds);
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
        GITHUB => zone_core::SourceType::GitHub,
        "gitlab" => zone_core::SourceType::GitLab,
        "filesystem" => zone_core::SourceType::Filesystem,
        NOTION => zone_core::SourceType::Notion,
        "text" => zone_core::SourceType::Text,
        "web" => zone_core::SourceType::Web,
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

    let credential = match state.github().source(state.db(), &source_row).await {
        Ok(credential) => credential,
        Err(error) => {
            let (status, message) = verification_refusal(&error);
            if status.is_server_error() {
                tracing::error!(source_id = %id, %error, "Failed to resolve credentials");
            } else {
                tracing::warn!(source_id = %id, %error, "The source's GitHub connection refused verification");
            }
            if let Err(failure) =
                sources::update_verification(state.db(), id, workspace_id, Some(message)).await
            {
                tracing::error!(source_id = %id, error = %failure, "Failed to update verification status");
            }
            return (status, Json(ErrorResponse::new(message))).into_response();
        }
    };
    if let Some(credential) = credential
        && let Some(object) = config.as_object_mut()
    {
        let field = match source_type {
            zone_core::SourceType::Notion => API_KEY_FIELD,
            _ => TOKEN_FIELD,
        };
        object.insert(
            field.to_string(),
            serde_json::Value::String(credential.secret().expose().to_string()),
        );
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

/// One kind the console may offer: `id` is the adapter's kind, `name` the
/// label, `category` one of the console's source categories.
#[derive(Debug, Serialize)]
pub struct SourceTypeInfo {
    id: String,
    name: String,
    category: String,
    enabled: bool,
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

    // The index status is keyed by source alone, so it must not be read for a
    // source this workspace does not own -- a conflict would answer for another
    // tenant's source before the check below refuses it.
    match sources::get_source(state.db(), source_id, workspace_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Source not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(source_id = %source_id, workspace_id = %workspace_id, error = %e, "Database error getting source for reindex");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    let index_status = sources::get_source_index_status(state.db(), source_id).await;
    if let Ok(status) = &index_status
        && matches!(status.status, sources::IndexStatus::Indexing)
    {
        return (
            StatusCode::CONFLICT,
            Json(ErrorResponse::new("Source is already being indexed")),
        )
            .into_response();
    }

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

/// Everything known about a kind besides whether an adapter for it is registered.
fn describe_kind(kind: &str) -> SourceTypeInfo {
    let (name, category, description, config_schema) = match kind {
        GITHUB => (
            "GitHub",
            "file",
            "GitHub repository integration",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string", "description": "Repository owner"},
                    "repo": {"type": "string", "description": "Repository name"},
                    "branch": {"type": "string", "description": "Default branch"}
                },
                "required": ["owner", "repo"]
            }),
        ),
        "gitlab" => (
            "GitLab",
            "file",
            "GitLab project integration",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Project path or numeric id"},
                    "host": {"type": "string", "description": "GitLab host for self-hosted instances"},
                    "branch": {"type": "string", "description": "Default branch"}
                },
                "required": ["project_id"]
            }),
        ),
        "filesystem" => (
            "Local Directory",
            "file",
            "Local filesystem (self-hosted only)",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "base_path": {"type": "string", "description": "Absolute path to project root"},
                    "allow_writes": {"type": "boolean", "description": "Allow write operations", "default": true}
                },
                "required": ["base_path"]
            }),
        ),
        "web" => (
            "Web URL",
            "web",
            "Fetch content from a URL",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to fetch"}
                },
                "required": ["url"]
            }),
        ),
        "text" => (
            "Text",
            "text",
            "Inline text content",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "The text"},
                    "label": {"type": "string", "description": "A label for the text"}
                },
                "required": ["content"]
            }),
        ),
        other => (
            other,
            "file",
            "",
            serde_json::json!({"type": "object", "properties": {}}),
        ),
    };
    SourceTypeInfo {
        id: kind.to_string(),
        name: name.to_string(),
        category: category.to_string(),
        enabled: true,
        description: description.to_string(),
        config_schema,
    }
}

/// GET /api/sources/types
///
/// Exactly the kinds a registered adapter can verify, so the console cannot
/// offer one the server would refuse.
pub async fn list_types(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    let types = source_kinds(&state)
        .unwrap_or_default()
        .iter()
        .map(|kind| describe_kind(kind))
        .collect();
    Json(SourceTypesListResponse { types })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use axum::http::StatusCode;

    use super::{
        APP_REJECTED, CONNECTION_LOST, CREDENTIALS_UNREADABLE, GITHUB_UNAVAILABLE, INTERNAL_ERROR,
        UNREACHABLE, UpdateSourceRequest, configured_repository, installation_refusal,
        repository_refusal, verification_refusal,
    };
    use crate::db::github_installations::UNATTACHABLE;
    use crate::error::ServerError;
    use crate::services::github_app;
    use abnegate_vcs::pull_request::{ApplicationError, PullRequestError};

    fn installation_of(body: serde_json::Value) -> Option<Option<Uuid>> {
        serde_json::from_value::<UpdateSourceRequest>(body)
            .expect("the update request deserializes")
            .github_installation_id
    }

    #[test]
    fn an_absent_installation_leaves_the_source_as_it_is() {
        assert_eq!(installation_of(json!({ "name": "renamed" })), None);
    }

    #[test]
    fn a_null_installation_detaches_it() {
        assert_eq!(
            installation_of(json!({ "github_installation_id": null })),
            Some(None)
        );
    }

    #[test]
    fn an_installation_id_attaches_it() {
        let installation = Uuid::new_v4();
        assert_eq!(
            installation_of(json!({ "github_installation_id": installation })),
            Some(Some(installation))
        );
    }

    #[test]
    fn a_malformed_installation_id_is_refused() {
        let parsed = serde_json::from_value::<UpdateSourceRequest>(
            json!({ "github_installation_id": "not-a-uuid" }),
        );
        assert!(parsed.is_err(), "a non-uuid installation must not parse");
    }

    #[test]
    fn debug_redacts_credentials_but_names_the_installation() {
        let installation = Uuid::new_v4();
        let request = serde_json::from_value::<UpdateSourceRequest>(json!({
            "credentials": "ghp_secret",
            "github_installation_id": installation,
        }))
        .expect("the update request deserializes");
        let rendered = format!("{request:?}");
        assert!(
            !rendered.contains("ghp_secret"),
            "credentials leaked: {rendered}"
        );
        assert!(
            rendered.contains(&installation.to_string()),
            "installation missing: {rendered}"
        );
    }

    fn refused(message: &str) -> ServerError {
        ServerError::BadRequest(message.to_string())
    }

    fn unavailable() -> ServerError {
        ServerError::BadGateway(GITHUB_UNAVAILABLE.to_string())
    }

    fn assert_maps(actual: ServerError, expected: ServerError) {
        assert_eq!(actual.to_string(), expected.to_string());
    }

    #[test]
    fn github_refusing_the_repository_is_unreachable() {
        for error in [
            PullRequestError::NotFound,
            PullRequestError::Forbidden,
            PullRequestError::AuthenticationFailed,
            PullRequestError::InvalidRepositoryName,
        ] {
            assert_maps(
                repository_refusal(Uuid::new_v4(), error),
                refused(UNREACHABLE),
            );
        }
    }

    #[test]
    fn github_failing_to_answer_for_the_repository_is_a_bad_gateway() {
        for error in [
            PullRequestError::RateLimited,
            PullRequestError::GitHubApi("GitHub API returned 502 Bad Gateway".to_string()),
        ] {
            assert_maps(repository_refusal(Uuid::new_v4(), error), unavailable());
        }
    }

    #[test]
    fn an_installation_zone_may_not_use_is_unattachable() {
        for error in [
            github_app::Error::Revoked,
            github_app::Error::Suspended,
            github_app::Error::NotConnected,
        ] {
            assert_maps(
                installation_refusal(Uuid::new_v4(), error),
                refused(UNATTACHABLE),
            );
        }
    }

    #[test]
    fn github_refusing_the_mint_is_unreachable() {
        for status in [400, 422] {
            assert_maps(
                installation_refusal(
                    Uuid::new_v4(),
                    github_app::Error::Application(ApplicationError::Status(status)),
                ),
                refused(UNREACHABLE),
            );
        }
    }

    #[test]
    fn github_failing_to_mint_is_a_bad_gateway() {
        for error in [
            ApplicationError::Status(429),
            ApplicationError::Status(502),
            ApplicationError::Transport("connection refused".to_string()),
            ApplicationError::Response,
        ] {
            assert_maps(
                installation_refusal(Uuid::new_v4(), github_app::Error::Application(error)),
                unavailable(),
            );
        }
    }

    #[test]
    fn github_refusing_the_apps_credentials_asks_to_reconnect() {
        assert_maps(
            installation_refusal(
                Uuid::new_v4(),
                github_app::Error::Application(ApplicationError::Status(401)),
            ),
            refused(APP_REJECTED),
        );
    }

    #[test]
    fn a_verification_tells_github_failing_apart_from_an_unreadable_credential() {
        let cases = [
            (
                github_app::Error::Revoked,
                StatusCode::BAD_REQUEST,
                CONNECTION_LOST,
            ),
            (
                github_app::Error::Application(ApplicationError::Status(401)),
                StatusCode::BAD_REQUEST,
                APP_REJECTED,
            ),
            (
                github_app::Error::Application(ApplicationError::Status(422)),
                StatusCode::BAD_REQUEST,
                UNREACHABLE,
            ),
            (
                github_app::Error::Application(ApplicationError::Status(502)),
                StatusCode::BAD_GATEWAY,
                GITHUB_UNAVAILABLE,
            ),
            (
                github_app::Error::Application(ApplicationError::Transport(
                    "connection refused".to_string(),
                )),
                StatusCode::BAD_GATEWAY,
                GITHUB_UNAVAILABLE,
            ),
            (
                github_app::Error::Credential,
                StatusCode::INTERNAL_SERVER_ERROR,
                CREDENTIALS_UNREADABLE,
            ),
            (
                github_app::Error::Unsealing,
                StatusCode::INTERNAL_SERVER_ERROR,
                CREDENTIALS_UNREADABLE,
            ),
            (
                github_app::Error::Database(sqlx::Error::PoolTimedOut),
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_ERROR,
            ),
        ];
        for (error, status, message) in cases {
            let rendered = error.to_string();
            assert_eq!(
                verification_refusal(&error),
                (status, message),
                "{rendered}"
            );
        }
    }

    #[test]
    fn a_broken_app_key_is_an_internal_error() {
        assert!(matches!(
            installation_refusal(
                Uuid::new_v4(),
                github_app::Error::Application(ApplicationError::PrivateKey),
            ),
            ServerError::Internal(_)
        ));
    }

    #[test]
    fn a_config_without_an_owner_and_repo_names_no_repository() {
        for config in [
            json!({}),
            json!({ "owner": "acme" }),
            json!({ "owner": "acme", "repo": "../widgets" }),
            json!("acme/widgets"),
        ] {
            assert!(
                configured_repository(&config).is_err(),
                "{config} must not name a repository"
            );
        }
        let repository = configured_repository(&json!({ "owner": "acme", "repo": "widgets" }))
            .expect("owner and repo name a repository");
        assert_eq!(repository.to_string(), "acme/widgets");
    }
}
