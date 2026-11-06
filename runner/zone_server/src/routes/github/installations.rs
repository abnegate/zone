use std::num::NonZeroU32;

use abnegate_vcs::pull_request::{ApplicationError, Installation, InstallationId};
use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::StatusCode,
};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::OrgAdmin;
use crate::db::audit::{actions, resources};
use crate::db::github_apps::{self, AppRow};
use crate::db::github_connection_states::{self, Purpose};
use crate::db::github_installations::{self, InstallationRow};
use crate::error::ServerError;
use crate::routes::common::{AuditEvent, audit};
use crate::services::github_app::manifest::installation_url;
use crate::services::github_app::{Error, PAGE_SIZE, Provider, Unrecorded, installations};
use crate::state::AppState;

const UNREACHABLE: &str = "GitHub could not be reached.";
const UNAVAILABLE: &str = "The GitHub connection could not be used.";
const APP_NOT_FOUND: &str = "GitHub App not found.";
const INSTALLATION_NOT_FOUND: &str = "GitHub installation not found.";
const INVALID_UNINSTALL: &str = "uninstall must be true or false.";

#[derive(Debug, Serialize)]
pub struct InstallationUrl {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct Removal {
    #[serde(default)]
    pub uninstall: bool,
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
struct Tally {
    listed: usize,
    recorded: usize,
    unrecorded: usize,
    revoked: u64,
}

impl Tally {
    fn add(&mut self, other: Self) {
        self.listed += other.listed;
        self.recorded += other.recorded;
        self.unrecorded += other.unrecorded;
        self.revoked += other.revoked;
    }
}

#[derive(Debug, Serialize)]
struct Synced {
    apps: usize,
    failed: usize,
    #[serde(flatten)]
    tally: Tally,
}

#[derive(Debug, Serialize)]
struct RemovedInstallation<'a> {
    account_login: &'a str,
}

#[derive(Debug, Serialize)]
struct Removed {
    uninstalled: bool,
}

pub async fn create(
    State(state): State<AppState>,
    admin: OrgAdmin,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<InstallationUrl>, ServerError> {
    let Path((_, app_id)) = path.map_err(|_| app_not_found())?;
    let app = github_apps::get(state.db(), admin.org_id, app_id)
        .await?
        .ok_or_else(app_not_found)?;
    let issued = github_connection_states::issue(
        state.db(),
        admin.org_id,
        admin.user_id,
        Purpose::Installation,
        Some(app.id),
    )
    .await?;

    Ok(Json(InstallationUrl {
        url: installation_url(&app.html_url, issued.state.expose()),
    }))
}

/// Every App is synced even when another's listing fails; only an App whose
/// listing finished revokes what GitHub no longer lists.
pub async fn sync(
    State(state): State<AppState>,
    admin: OrgAdmin,
) -> Result<StatusCode, ServerError> {
    let apps = github_apps::list(state.db(), admin.org_id).await?;
    if apps.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }

    let outcomes = join_all(
        apps.iter()
            .map(|app| synchronize(&state, app, admin.user_id)),
    )
    .await;

    let mut tally = Tally::default();
    let mut failures = Vec::new();
    for (app, outcome) in apps.iter().zip(outcomes) {
        match outcome {
            Ok(synced) => tally.add(synced),
            Err(error) => failures.push(refused(error, app)),
        }
    }

    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(admin.org_id),
            workspace_id: None,
            actor_id: admin.user_id,
            actor_email: &admin.email,
            action: actions::GITHUB_INSTALLATIONS_SYNCED,
            resource_type: resources::GITHUB_INSTALLATION,
            resource_id: None,
            old_values: None,
            new_values: serde_json::to_value(Synced {
                apps: apps.len(),
                failed: failures.len(),
                tally,
            })
            .ok(),
        },
    )
    .await;

    match failures
        .into_iter()
        .max_by_key(|failure| matches!(failure, ServerError::Internal(_)))
    {
        Some(failure) => Err(failure),
        None => Ok(StatusCode::NO_CONTENT),
    }
}

/// Uninstalling asks GitHub first, so a refusal keeps the installation in Zone.
pub async fn delete(
    State(state): State<AppState>,
    admin: OrgAdmin,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    removal: Result<Query<Removal>, QueryRejection>,
) -> Result<StatusCode, ServerError> {
    let Path((_, id)) = path.map_err(|_| installation_not_found())?;
    let Query(removal) =
        removal.map_err(|_| ServerError::BadRequest(INVALID_UNINSTALL.to_string()))?;
    let installation = github_installations::get(state.db(), admin.org_id, id)
        .await?
        .ok_or_else(installation_not_found)?;

    if removal.uninstall {
        uninstall(&state, &installation).await?;
    }
    if !github_installations::delete(state.db(), admin.org_id, id).await? {
        return Err(installation_not_found());
    }

    let provider = state.github();
    if let Some(number) = number(&installation) {
        provider.invalidate(number);
    }
    provider.catalog().forget(id);

    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(admin.org_id),
            workspace_id: None,
            actor_id: admin.user_id,
            actor_email: &admin.email,
            action: actions::GITHUB_INSTALLATION_REMOVED,
            resource_type: resources::GITHUB_INSTALLATION,
            resource_id: Some(id),
            old_values: serde_json::to_value(RemovedInstallation {
                account_login: &installation.account_login,
            })
            .ok(),
            new_values: serde_json::to_value(Removed {
                uninstalled: removal.uninstall,
            })
            .ok(),
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn synchronize(state: &AppState, app: &AppRow, user_id: Uuid) -> Result<Tally, Error> {
    let provider = state.github();
    let since = github_installations::clock(state.db()).await?;
    let listed = listed(provider, app).await?;
    let present: Vec<i64> = listed
        .iter()
        .filter_map(|installation| i64::try_from(installation.id.get()).ok())
        .collect();

    let mut tally = Tally {
        listed: listed.len(),
        ..Tally::default()
    };
    for installation in &listed {
        match installations::record(state.db(), app, installation, Some(user_id)).await {
            Ok(row) => {
                provider.catalog().forget(row.id);
                tally.recorded += 1;
            }
            Err(unrecorded) => {
                skipped(app, installation, &unrecorded);
                tally.unrecorded += 1;
            }
        }
    }
    tally.revoked =
        github_installations::revoke_missing(state.db(), app.id, &present, since).await?;

    Ok(tally)
}

async fn listed(provider: &Provider, app: &AppRow) -> Result<Vec<Installation>, Error> {
    let applications = provider.applications()?;
    let jwt = provider.application_jwt_for(app)?;
    let mut installations = Vec::new();
    let mut page = Some(NonZeroU32::MIN);
    while let Some(current) = page {
        let listed = applications
            .list_installations(&jwt, current, PAGE_SIZE)
            .await?;
        installations.extend(listed.items);
        page = listed.next;
    }

    Ok(installations)
}

fn skipped(app: &AppRow, installation: &Installation, unrecorded: &Unrecorded) {
    let installation_id = installation.id.get();
    match unrecorded {
        Unrecorded::Database(_) => tracing::error!(
            %unrecorded,
            organization_id = %app.organization_id,
            github_app_id = %app.id,
            installation_id,
            "A GitHub installation could not be stored while syncing"
        ),
        Unrecorded::ForeignApplication
        | Unrecorded::OutOfRange
        | Unrecorded::Unaddressable
        | Unrecorded::UnknownAccountType
        | Unrecorded::UnknownRepositorySelection
        | Unrecorded::Foreign => tracing::warn!(
            %unrecorded,
            organization_id = %app.organization_id,
            github_app_id = %app.id,
            installation_id,
            "Skipped a GitHub installation Zone cannot record while syncing"
        ),
    }
}

async fn uninstall(state: &AppState, installation: &InstallationRow) -> Result<(), ServerError> {
    let app = github_apps::get(
        state.db(),
        installation.organization_id,
        installation.github_app_id,
    )
    .await?
    .ok_or_else(installation_not_found)?;
    let provider = state.github();

    let uninstalled = async {
        let number = number(installation).ok_or(Error::NotConnected)?;
        let jwt = provider.application_jwt_for(&app)?;
        provider
            .applications()?
            .delete_installation(&jwt, number)
            .await?;
        Ok::<(), Error>(())
    }
    .await;

    uninstalled.map_err(|error| refused(error, &app))
}

fn number(installation: &InstallationRow) -> Option<InstallationId> {
    u64::try_from(installation.installation_id)
        .ok()
        .map(InstallationId::new)
}

fn app_not_found() -> ServerError {
    ServerError::NotFound(APP_NOT_FOUND.to_string())
}

fn installation_not_found() -> ServerError {
    ServerError::NotFound(INSTALLATION_NOT_FOUND.to_string())
}

fn unreachable(error: &Error) -> bool {
    matches!(
        error,
        Error::Application(
            ApplicationError::Status(_)
                | ApplicationError::Transport(_)
                | ApplicationError::Response
        )
    )
}

fn refused(error: Error, app: &AppRow) -> ServerError {
    if unreachable(&error) {
        tracing::warn!(
            %error,
            organization_id = %app.organization_id,
            github_app_id = %app.id,
            "GitHub refused a GitHub App installation request"
        );
        return ServerError::BadGateway(UNREACHABLE.to_string());
    }

    tracing::error!(
        %error,
        organization_id = %app.organization_id,
        github_app_id = %app.id,
        "A GitHub App installation request could not be made"
    );
    ServerError::Internal(UNAVAILABLE.to_string())
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;
    use chrono::Utc;

    use super::*;
    use crate::db::github_apps::OwnerType;

    fn app() -> AppRow {
        AppRow {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            app_id: 1,
            slug: "zone".to_string(),
            name: "Zone".to_string(),
            owner_login: "acme".to_string(),
            owner_type: OwnerType::Organization,
            html_url: "https://github.com/apps/zone".to_string(),
            client_id: "Iv1.client".to_string(),
            client_secret: String::new(),
            private_key: String::new(),
            webhook_secret: None,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn status(error: Error) -> StatusCode {
        refused(error, &app()).into_response().status()
    }

    #[test]
    fn github_answering_badly_or_not_at_all_is_a_bad_gateway() {
        for error in [
            ApplicationError::Status(500),
            ApplicationError::Status(403),
            ApplicationError::Transport("connection refused".to_string()),
            ApplicationError::Response,
        ] {
            assert_eq!(status(Error::from(error)), StatusCode::BAD_GATEWAY);
        }
    }

    #[test]
    fn a_configuration_or_credential_failure_is_an_internal_error() {
        for error in [
            Error::from(ApplicationError::InvalidOrigin),
            Error::from(ApplicationError::Client),
            Error::from(ApplicationError::PrivateKey),
            Error::from(ApplicationError::Signing),
            Error::Unsealing,
            Error::Credential,
            Error::NotConnected,
            Error::from(sqlx::Error::PoolTimedOut),
        ] {
            assert_eq!(status(error), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    #[test]
    fn the_sync_audit_records_every_count_side_by_side() {
        let synced = Synced {
            apps: 2,
            failed: 1,
            tally: Tally {
                listed: 5,
                recorded: 4,
                unrecorded: 1,
                revoked: 3,
            },
        };

        assert_eq!(
            serde_json::to_value(synced).expect("the audit serializes"),
            serde_json::json!({
                "apps": 2,
                "failed": 1,
                "listed": 5,
                "recorded": 4,
                "unrecorded": 1,
                "revoked": 3,
            })
        );
    }

    #[test]
    fn uninstall_defaults_to_false_and_refuses_anything_but_a_boolean() {
        let parsed = |query: &str| {
            Query::<Removal>::try_from_uri(&format!("/?{query}").parse().expect("a URI"))
                .map(|Query(removal)| removal.uninstall)
        };

        assert!(!parsed("").expect("absent is false"));
        assert!(!parsed("uninstall=false").expect("false parses"));
        assert!(parsed("uninstall=true").expect("true parses"));
        assert!(parsed("uninstall=maybe").is_err());
    }
}
