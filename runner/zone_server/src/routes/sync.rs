//! External sync configuration endpoints
//!
//! A project can be pointed at a GitHub repository or a Linear project. What is
//! stored here is the configuration alone: which provider, which direction, and
//! where. Nothing runs a sync yet, so every configuration answers with
//! `status: configured` and no `last_synced_at`, and the console says so rather
//! than pretending items have moved.

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
use crate::db::sync_config::{self, SyncConfigRow};
use crate::db::{projects, workspace_members};
use crate::error::ServerError;
use crate::state::AppState;

/// The providers `sync_configs.provider` accepts.
pub const PROVIDERS: [&str; 2] = ["github", "linear"];

/// The directions items may move in.
pub const DIRECTIONS: [&str; 3] = ["inbound", "outbound", "bidirectional"];

/// What a configuration reports until a sync engine has run it.
pub const STATUS_CONFIGURED: &str = "configured";

const UNIQUE_VIOLATION: &str = "23505";

#[derive(Debug, Deserialize)]
pub struct CreateSyncConfigRequest {
    provider: String,
    direction: String,
    external_repo_url: Option<String>,
    external_project_id: Option<String>,
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
}

#[derive(Debug, Serialize)]
pub struct SyncConfigResponse {
    config: SyncConfigData,
}

#[derive(Debug, Serialize)]
pub struct SyncConfigsListResponse {
    configs: Vec<SyncConfigData>,
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

/// The configuration a request asks for, or why it cannot be stored.
fn validate(req: &CreateSyncConfigRequest) -> Result<serde_json::Value, ServerError> {
    if !PROVIDERS.contains(&req.provider.as_str()) {
        return Err(ServerError::BadRequest(format!(
            "Invalid provider \"{}\". Must be one of: {}",
            req.provider,
            PROVIDERS.join(", ")
        )));
    }
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
    match req.provider.as_str() {
        "github" if repo_url.is_none() => {
            return Err(ServerError::BadRequest(
                "A GitHub sync needs external_repo_url".to_string(),
            ));
        }
        "linear" if project_id.is_none() => {
            return Err(ServerError::BadRequest(
                "A Linear sync needs external_project_id".to_string(),
            ));
        }
        _ => {}
    }
    Ok(json!({
        "direction": req.direction,
        "external_repo_url": repo_url,
        "external_project_id": project_id,
    }))
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
    let config = validate(&req)?;
    if sync_config::get_sync_config_by_project_provider(state.db(), id, &req.provider)
        .await?
        .is_some()
    {
        return Err(already_configured(&req.provider));
    }

    let row = sync_config::create_sync_config(state.db(), id, &req.provider, true, config, None)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                already_configured(&req.provider)
            } else {
                ServerError::from(error)
            }
        })?;
    Ok((
        StatusCode::CREATED,
        Json(SyncConfigResponse {
            config: SyncConfigData::from(row),
        }),
    )
        .into_response())
}

/// DELETE /api/projects/:id/sync/:config_id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, config_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ServerError> {
    authorize(&state, &auth, id, true).await?;
    let missing = || ServerError::NotFound("Sync configuration not found".to_string());
    let row = sync_config::get_sync_config(state.db(), config_id)
        .await?
        .filter(|row| row.project_id == id)
        .ok_or_else(missing)?;
    if sync_config::delete_sync_config(state.db(), row.id).await? {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(missing())
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

    #[test]
    fn a_configuration_keeps_the_direction_and_target_the_form_offered() {
        let config = validate(&request("github", "outbound")).unwrap();
        assert_eq!(config["direction"], "outbound");
        assert_eq!(
            config["external_repo_url"],
            "https://github.com/acme/project"
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
    fn a_configuration_answers_as_configured_and_never_synced() {
        let id = Uuid::new_v4();
        let data = SyncConfigData::from(SyncConfigRow {
            id,
            project_id: Uuid::new_v4(),
            provider: "linear".to_string(),
            enabled: true,
            config: json!({ "direction": "inbound", "external_project_id": "LIN-1" }),
            webhook_secret_encrypted: None,
            created_at: Some(
                chrono::NaiveDate::from_ymd_opt(2026, 9, 20)
                    .unwrap()
                    .and_hms_opt(4, 13, 23)
                    .unwrap(),
            ),
            updated_at: None,
        });
        assert_eq!(data.status, STATUS_CONFIGURED);
        assert_eq!(data.last_synced_at, None);
        assert_eq!(data.direction, "inbound");
        assert_eq!(data.external_project_id.as_deref(), Some("LIN-1"));
        assert_eq!(data.external_repo_url, None);
        assert_eq!(data.created_at, "2026-09-20T04:13:23.000Z");
        assert_eq!(data.webhook_path, format!("/api/webhooks/sync/{id}/linear"));
    }
}
