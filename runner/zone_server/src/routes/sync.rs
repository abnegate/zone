//! External sync configuration endpoints
//!
//! A project can be pointed at a GitHub repository or a Linear project: which
//! provider, which direction, where, and the secret its webhook deliveries are
//! signed with. Inbound webhooks work: a delivery signed with that secret
//! updates the task it is linked to, and a new issue from the configured
//! repository or project becomes a non-agentic task. Outbound sync is not
//! implemented, so every configuration answers with `status: configured` and
//! no `last_synced_at`, and the console says so rather than pretending items
//! have moved out.
//!
//! GitHub signs deliveries with whatever secret the repository's webhook is
//! given, so Zone issues one when the configuration is created. Linear issues
//! its own signing secret, so a Linear configuration waits for it to be set.

use std::ops::RangeInclusive;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::crypto;
use crate::db::sync_config::{self, SyncConfigRow};
use crate::db::{projects, workspace_members};
use crate::error::ServerError;
use crate::state::AppState;
use crate::utils::crypto::generate_token;

/// The providers `sync_configs.provider` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    Linear,
}

impl Provider {
    pub const ALL: [Self; 2] = [Self::GitHub, Self::Linear];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Linear => "linear",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == value)
    }

    /// Whether Zone issues the secret deliveries are signed with, rather than
    /// the provider.
    pub fn issues_zone_secret(self) -> bool {
        match self {
            Self::GitHub => true,
            Self::Linear => false,
        }
    }
}

/// The directions items may move in.
pub const DIRECTIONS: [&str; 3] = ["inbound", "outbound", "bidirectional"];

/// What a configuration reports until a sync engine has run it.
pub const STATUS_CONFIGURED: &str = "configured";

/// How many characters a webhook secret set by hand may have.
pub const WEBHOOK_SECRET_LENGTH: RangeInclusive<usize> = 16..=256;

const UNIQUE_VIOLATION: &str = "23505";

#[derive(Debug, Deserialize)]
pub struct CreateSyncConfigRequest {
    provider: String,
    direction: String,
    external_repo_url: Option<String>,
    external_project_id: Option<String>,
}

/// A secret to verify deliveries with; without one, Zone generates it.
#[derive(Deserialize)]
pub struct WebhookSecretRequest {
    secret: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncConfigData {
    id: Uuid,
    project_id: Uuid,
    provider: String,
    direction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_repo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_project_id: Option<String>,
    is_active: bool,
    created_at: String,
    status: &'static str,
    last_synced_at: Option<String>,
    webhook_path: String,
    webhook_secret_configured: bool,
}

/// A configuration, and the webhook secret Zone just generated for it, which
/// is shown this once and never listed.
#[derive(Serialize)]
pub struct SyncConfigResponse {
    config: SyncConfigData,
    webhook_secret: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncConfigsListResponse {
    configs: Vec<SyncConfigData>,
}

/// A webhook secret, and whether the caller already knows it.
enum WebhookSecret {
    Generated(String),
    Supplied(String),
}

impl WebhookSecret {
    fn generate() -> Self {
        Self::Generated(generate_token())
    }

    fn supplied(secret: &str) -> Result<Self, ServerError> {
        let secret = secret.trim();
        if !WEBHOOK_SECRET_LENGTH.contains(&secret.chars().count()) {
            return Err(ServerError::BadRequest(format!(
                "A webhook secret must be {} to {} characters",
                WEBHOOK_SECRET_LENGTH.start(),
                WEBHOOK_SECRET_LENGTH.end()
            )));
        }
        Ok(Self::Supplied(secret.to_string()))
    }

    fn value(&self) -> &str {
        match self {
            Self::Generated(secret) | Self::Supplied(secret) => secret,
        }
    }

    fn encrypt(&self, state: &AppState) -> Result<String, ServerError> {
        crypto::encrypt(state.encryption_key(), self.value())
            .map_err(|_| ServerError::Internal("Failed to encrypt webhook secret".to_string()))
    }

    /// The secret to show the caller: one Zone generated, never one they
    /// supplied.
    fn shown(self) -> Option<String> {
        match self {
            Self::Generated(secret) => Some(secret),
            Self::Supplied(_) => None,
        }
    }
}

/// The path a provider's webhooks are received on for one configuration.
pub fn webhook_path(id: Uuid, provider: &str) -> String {
    format!("/api/webhooks/sync/{id}/{provider}")
}

impl From<SyncConfigRow> for SyncConfigData {
    fn from(row: SyncConfigRow) -> Self {
        let field = |name: &str| {
            row.config
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        Self {
            webhook_path: webhook_path(row.id, &row.provider),
            direction: field("direction").unwrap_or_else(|| "bidirectional".to_string()),
            external_repo_url: field("external_repo_url"),
            external_project_id: field("external_project_id"),
            webhook_secret_configured: row.webhook_secret_encrypted.is_some(),
            id: row.id,
            project_id: row.project_id,
            provider: row.provider,
            is_active: row.enabled,
            created_at: row
                .created_at
                .map(|at| at.and_utc().to_rfc3339_opts(SecondsFormat::Millis, true))
                .unwrap_or_default(),
            status: STATUS_CONFIGURED,
            last_synced_at: None,
        }
    }
}

fn user_id(auth: &AuthUser) -> Result<Uuid, ServerError> {
    Uuid::parse_str(&auth.0.sub)
        .map_err(|_| ServerError::Unauthorized("Invalid user ID in token".to_string()))
}

fn project_not_found() -> ServerError {
    ServerError::NotFound("Project not found".to_string())
}

fn config_not_found() -> ServerError {
    ServerError::NotFound("Sync configuration not found".to_string())
}

/// Confirm the caller is a member of project `id`'s workspace; with `write`,
/// a member who may change it.
async fn authorize(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
    write: bool,
) -> Result<(), ServerError> {
    let user_id = user_id(auth)?;
    let workspace_id = projects::get_project(state.db(), id)
        .await?
        .and_then(|project| project.workspace_id)
        .ok_or_else(project_not_found)?;
    if !workspace_members::is_member(state.db(), user_id, workspace_id).await? {
        return Err(project_not_found());
    }
    if write && !workspace_members::can_write(state.db(), workspace_id, user_id).await? {
        return Err(ServerError::Forbidden(
            "Workspace write access required".to_string(),
        ));
    }
    Ok(())
}

/// Configuration `config_id`, provided it belongs to project `id`.
async fn owned_config(
    state: &AppState,
    id: Uuid,
    config_id: Uuid,
) -> Result<SyncConfigRow, ServerError> {
    sync_config::get_sync_config(state.db(), config_id)
        .await?
        .filter(|row| row.project_id == id)
        .ok_or_else(config_not_found)
}

/// The provider and configuration a request asks for, or why they cannot be
/// stored.
fn validate(req: &CreateSyncConfigRequest) -> Result<(Provider, serde_json::Value), ServerError> {
    let provider = Provider::parse(&req.provider).ok_or_else(|| {
        ServerError::BadRequest(format!(
            "Invalid provider \"{}\". Must be one of: {}",
            req.provider,
            Provider::ALL.map(Provider::as_str).join(", ")
        ))
    })?;
    if !DIRECTIONS.contains(&req.direction.as_str()) {
        return Err(ServerError::BadRequest(format!(
            "Invalid direction \"{}\". Must be one of: {}",
            req.direction,
            DIRECTIONS.join(", ")
        )));
    }
    let repo_url = req
        .external_repo_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty());
    let project_id = req
        .external_project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    match provider {
        Provider::GitHub if repo_url.is_none() => {
            return Err(ServerError::BadRequest(
                "A GitHub sync needs external_repo_url".to_string(),
            ));
        }
        Provider::Linear if project_id.is_none() => {
            return Err(ServerError::BadRequest(
                "A Linear sync needs external_project_id".to_string(),
            ));
        }
        _ => {}
    }
    Ok((
        provider,
        json!({
            "direction": req.direction,
            "external_repo_url": repo_url,
            "external_project_id": project_id,
        }),
    ))
}

fn already_configured(provider: &str) -> ServerError {
    ServerError::Conflict(format!(
        "A {provider} sync is already configured for this project; remove it first"
    ))
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some(UNIQUE_VIOLATION)
    )
}

/// GET /api/projects/:id/sync
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    authorize(&state, &auth, id, false).await?;
    let rows = sync_config::list_sync_configs(state.db(), id).await?;
    Ok(Json(SyncConfigsListResponse {
        configs: rows.into_iter().map(SyncConfigData::from).collect(),
    })
    .into_response())
}

/// POST /api/projects/:id/sync
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateSyncConfigRequest>,
) -> Result<Response, ServerError> {
    authorize(&state, &auth, id, true).await?;
    let (provider, config) = validate(&req)?;
    let provider_name = provider.as_str();
    if sync_config::get_sync_config_by_project_provider(state.db(), id, provider_name)
        .await?
        .is_some()
    {
        return Err(already_configured(provider_name));
    }

    let secret = provider.issues_zone_secret().then(WebhookSecret::generate);
    let encrypted = secret
        .as_ref()
        .map(|secret| secret.encrypt(&state))
        .transpose()?;
    let row = sync_config::create_sync_config(
        state.db(),
        id,
        provider_name,
        true,
        config,
        encrypted.as_deref(),
    )
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            already_configured(provider_name)
        } else {
            ServerError::from(error)
        }
    })?;
    Ok((
        StatusCode::CREATED,
        Json(SyncConfigResponse {
            config: SyncConfigData::from(row),
            webhook_secret: secret.and_then(WebhookSecret::shown),
        }),
    )
        .into_response())
}

/// PUT /api/projects/:id/sync/:config_id/webhook-secret
///
/// Sets the secret deliveries are verified with: the one supplied, or a new
/// one Zone generates. Either replaces the old, whose signatures stop
/// verifying.
pub async fn set_webhook_secret(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, config_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<WebhookSecretRequest>,
) -> Result<Response, ServerError> {
    authorize(&state, &auth, id, true).await?;
    let secret = match req.secret.as_deref() {
        Some(supplied) => WebhookSecret::supplied(supplied)?,
        None => WebhookSecret::generate(),
    };
    let row = owned_config(&state, id, config_id).await?;
    let encrypted = secret.encrypt(&state)?;
    let row = sync_config::update_sync_config(state.db(), row.id, None, None, Some(&encrypted))
        .await?
        .ok_or_else(config_not_found)?;
    Ok(Json(SyncConfigResponse {
        config: SyncConfigData::from(row),
        webhook_secret: secret.shown(),
    })
    .into_response())
}

/// DELETE /api/projects/:id/sync/:config_id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, config_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ServerError> {
    authorize(&state, &auth, id, true).await?;
    let row = owned_config(&state, id, config_id).await?;
    if sync_config::delete_sync_config(state.db(), row.id).await? {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(config_not_found())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(provider: &str, direction: &str) -> CreateSyncConfigRequest {
        CreateSyncConfigRequest {
            provider: provider.to_string(),
            direction: direction.to_string(),
            external_repo_url: Some("https://github.com/acme/project".to_string()),
            external_project_id: Some("LIN-1".to_string()),
        }
    }

    fn row(provider: Provider, webhook_secret_encrypted: Option<String>) -> SyncConfigRow {
        SyncConfigRow {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            provider: provider.as_str().to_string(),
            enabled: true,
            config: json!({ "direction": "inbound", "external_project_id": "LIN-1" }),
            webhook_secret_encrypted,
            created_at: Some(
                chrono::NaiveDate::from_ymd_opt(2026, 9, 20)
                    .unwrap()
                    .and_hms_opt(4, 13, 23)
                    .unwrap(),
            ),
            updated_at: None,
        }
    }

    #[test]
    fn a_configuration_keeps_the_direction_and_target_the_form_offered() {
        let (provider, config) = validate(&request("github", "outbound")).unwrap();
        assert_eq!(provider, Provider::GitHub);
        assert_eq!(config["direction"], "outbound");
        assert_eq!(
            config["external_repo_url"],
            "https://github.com/acme/project"
        );
    }

    #[test]
    fn an_unknown_provider_is_refused_with_the_ones_accepted() {
        let Err(ServerError::BadRequest(message)) = validate(&request("jira", "inbound")) else {
            panic!("an unknown provider is a bad request");
        };
        assert_eq!(
            message,
            "Invalid provider \"jira\". Must be one of: github, linear"
        );
    }

    #[test]
    fn a_provider_needs_its_own_kind_of_target() {
        let mut github = request("github", "inbound");
        github.external_repo_url = Some("  ".to_string());
        assert!(
            validate(&github).is_err(),
            "GitHub without a repository URL"
        );

        let mut linear = request("linear", "inbound");
        linear.external_project_id = None;
        assert!(validate(&linear).is_err(), "Linear without a project id");
    }

    #[test]
    fn only_github_is_issued_a_secret_by_zone() {
        assert!(Provider::GitHub.issues_zone_secret());
        assert!(!Provider::Linear.issues_zone_secret());
    }

    #[test]
    fn a_configuration_answers_as_configured_and_never_synced() {
        let row = row(Provider::Linear, None);
        let id = row.id;
        let data = SyncConfigData::from(row);
        assert_eq!(data.status, STATUS_CONFIGURED);
        assert_eq!(data.last_synced_at, None);
        assert_eq!(data.direction, "inbound");
        assert_eq!(data.external_project_id.as_deref(), Some("LIN-1"));
        assert_eq!(data.external_repo_url, None);
        assert_eq!(data.created_at, "2026-09-20T04:13:23.000Z");
        assert_eq!(data.webhook_path, format!("/api/webhooks/sync/{id}/linear"));
        assert!(!data.webhook_secret_configured);
    }

    #[test]
    fn a_configuration_says_whether_its_secret_is_set_without_showing_it() {
        let data = SyncConfigData::from(row(Provider::GitHub, Some("ciphertext".to_string())));
        assert!(data.webhook_secret_configured);
        let body = serde_json::to_string(&data).unwrap();
        assert!(
            !body.contains("ciphertext"),
            "the stored secret is never listed: {body}"
        );
    }

    #[test]
    fn a_supplied_secret_is_trimmed_and_held_to_its_length() {
        let WebhookSecret::Supplied(secret) =
            WebhookSecret::supplied("  linear-signing-secret-123 ").unwrap()
        else {
            panic!("a supplied secret stays supplied");
        };
        assert_eq!(secret, "linear-signing-secret-123");

        let too_long = "x".repeat(257);
        for refused in ["   ", "fifteen-chars!!", too_long.as_str()] {
            assert!(
                matches!(
                    WebhookSecret::supplied(refused),
                    Err(ServerError::BadRequest(_))
                ),
                "{} characters is refused",
                refused.len()
            );
        }
        assert!(WebhookSecret::supplied(&"x".repeat(16)).is_ok());
        assert!(WebhookSecret::supplied(&"x".repeat(256)).is_ok());
    }

    #[test]
    fn only_a_generated_secret_is_shown() {
        let generated = WebhookSecret::generate();
        let value = generated.value().to_string();
        assert_eq!(value.len(), 64);
        assert!(value.chars().all(|character| character.is_ascii_hexdigit()));
        assert_eq!(generated.shown(), Some(value));

        let supplied = WebhookSecret::supplied("linear-signing-secret-123").unwrap();
        assert_eq!(supplied.shown(), None);
    }
}
