use abnegate_vcs::pull_request::InstallationId;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{OrgAdmin, OrgMember};
use crate::db::audit::{actions, resources};
use crate::db::github_apps::{self, AppRow, OwnerType};
use crate::db::github_connection_states::{self, Purpose};
use crate::db::github_installations::{self, InstallationRow};
use crate::db::organizations;
use crate::error::ServerError;
use crate::routes::common::{AuditEvent, audit};
use crate::services::github_app::login::Login;
use crate::services::github_app::manifest::{self, Manifest};
use crate::state::AppState;

const UNREADABLE: &str =
    "Send a JSON body with an owner: a GitHub organization login, or null for your own account.";
const INVALID_OWNER: &str = "The owner must be a GitHub organization login: letters, digits and single hyphens, at most 39 characters, not starting or ending with a hyphen.";
const OWNER_CONNECTED: &str = "This organization already connected that GitHub account.";
const PERSONAL_CONNECTED: &str =
    "You already connected your own GitHub account to this organization.";
const ORGANIZATION_NOT_FOUND: &str = "Organization not found";
const APP_NOT_FOUND: &str = "GitHub App not found";

#[derive(Debug, Serialize)]
pub struct App {
    id: Uuid,
    app_id: i64,
    slug: String,
    name: String,
    owner_login: String,
    owner_type: &'static str,
    html_url: String,
    settings_url: String,
    created_at: DateTime<Utc>,
}

impl App {
    fn of(app: AppRow, github_web_url: &str) -> Self {
        let settings_url = settings_url(&app, github_web_url);
        Self {
            id: app.id,
            app_id: app.app_id,
            slug: app.slug,
            name: app.name,
            owner_login: app.owner_login,
            owner_type: app.owner_type.as_str(),
            html_url: app.html_url,
            settings_url,
            created_at: app.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Installation {
    id: Uuid,
    github_app_id: Uuid,
    installation_id: i64,
    account_login: String,
    account_type: &'static str,
    repository_selection: &'static str,
    html_url: String,
    suspended_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<InstallationRow> for Installation {
    fn from(installation: InstallationRow) -> Self {
        Self {
            id: installation.id,
            github_app_id: installation.github_app_id,
            installation_id: installation.installation_id,
            account_login: installation.account_login,
            account_type: installation.account_type.as_str(),
            repository_selection: installation.repository_selection.as_str(),
            html_url: installation.html_url,
            suspended_at: installation.suspended_at,
            revoked_at: installation.revoked_at,
            created_at: installation.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Connection {
    apps: Vec<App>,
    installations: Vec<Installation>,
}

#[derive(Debug, Deserialize)]
pub struct RegistrationRequest {
    owner: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Registration {
    action: String,
    manifest: String,
    state: String,
}

#[derive(Debug, Serialize)]
pub(super) struct AuditedApp<'a> {
    owner_login: &'a str,
    slug: &'a str,
}

impl<'a> From<&'a AppRow> for AuditedApp<'a> {
    fn from(app: &'a AppRow) -> Self {
        Self {
            owner_login: &app.owner_login,
            slug: &app.slug,
        }
    }
}

/// An organization App's settings live under the organization, so a stored
/// login GitHub would not accept falls back to the user path rather than
/// hiding the App.
fn settings_url(app: &AppRow, github_web_url: &str) -> String {
    let organization = match app.owner_type {
        OwnerType::User => None,
        OwnerType::Organization => {
            let login = Login::parse(&app.owner_login);
            if login.is_none() {
                tracing::debug!(
                    app = %app.id,
                    organization = %app.organization_id,
                    "A GitHub App's stored owner is not a valid login, so its settings link uses the user path"
                );
            }
            login
        }
    };
    manifest::settings_url(github_web_url, organization.as_ref(), &app.slug)
}

pub async fn get(
    member: OrgMember,
    State(state): State<AppState>,
) -> Result<Json<Connection>, ServerError> {
    let db = state.db();
    let (apps, installations) = tokio::try_join!(
        github_apps::list(db, member.org_id),
        github_installations::list(db, member.org_id),
    )?;
    let github_web_url = &state.config().github_web_url;

    Ok(Json(Connection {
        apps: apps
            .into_iter()
            .map(|app| App::of(app, github_web_url))
            .collect(),
        installations: installations.into_iter().map(Installation::from).collect(),
    }))
}

pub async fn register(
    admin: OrgAdmin,
    State(state): State<AppState>,
    request: Result<Json<RegistrationRequest>, JsonRejection>,
) -> Result<Json<Registration>, ServerError> {
    let Json(request) = request.map_err(|_| ServerError::BadRequest(UNREADABLE.to_string()))?;
    let owner = request
        .owner
        .map(|owner| {
            Login::parse(&owner).ok_or_else(|| ServerError::BadRequest(INVALID_OWNER.to_string()))
        })
        .transpose()?;
    let db = state.db();

    let connected = match &owner {
        Some(login) => github_apps::for_owner(db, admin.org_id, login.as_str())
            .await?
            .map(|_| OWNER_CONNECTED),
        None => github_apps::list(db, admin.org_id)
            .await?
            .iter()
            .any(|app| app.owner_type == OwnerType::User && app.created_by == Some(admin.user_id))
            .then_some(PERSONAL_CONNECTED),
    };
    if let Some(conflict) = connected {
        return Err(ServerError::Conflict(conflict.to_string()));
    }

    let organization = organizations::get_organization(db, admin.org_id)
        .await?
        .ok_or_else(|| ServerError::NotFound(ORGANIZATION_NOT_FOUND.to_string()))?;
    let issued = github_connection_states::issue(
        db,
        admin.org_id,
        admin.user_id,
        Purpose::Registration,
        None,
    )
    .await?;
    let config = state.config();
    let registration_state = issued.state.expose();

    Ok(Json(Registration {
        action: manifest::registration_url(
            &config.github_web_url,
            owner.as_ref(),
            registration_state,
        ),
        manifest: Manifest::new(&config.app_base_url, &organization.slug).to_json(),
        state: registration_state.to_string(),
    }))
}

pub async fn delete_app(
    admin: OrgAdmin,
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<StatusCode, ServerError> {
    let Path((_, app_id)) = path.map_err(|_| app_not_found())?;
    let db = state.db();
    let app = github_apps::get(db, admin.org_id, app_id)
        .await?
        .ok_or_else(app_not_found)?;
    let installations = github_installations::list_for_app(db, admin.org_id, app.id).await?;

    if !github_apps::delete(db, admin.org_id, app.id).await? {
        return Err(app_not_found());
    }

    let provider = state.github();
    for installation in &installations {
        if let Ok(number) = u64::try_from(installation.installation_id) {
            provider.invalidate(InstallationId::new(number));
        }
        provider.catalog().forget(installation.id);
    }

    audit(
        db,
        AuditEvent {
            organization_id: Some(admin.org_id),
            workspace_id: None,
            actor_id: admin.user_id,
            actor_email: &admin.email,
            action: actions::GITHUB_APP_REMOVED,
            resource_type: resources::GITHUB_APP,
            resource_id: Some(app.id),
            old_values: serde_json::to_value(AuditedApp::from(&app)).ok(),
            new_values: None,
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

fn app_not_found() -> ServerError {
    ServerError::NotFound(APP_NOT_FOUND.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;
    use crate::db::github_installations::{AccountType, RepositorySelection};

    const WEB: &str = "https://github.com";

    fn app(owner_login: &str, owner_type: OwnerType) -> AppRow {
        let now = Utc::now();
        AppRow {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            app_id: 42,
            slug: "zone-acme-x7k2".to_string(),
            name: "Zone acme x7k2".to_string(),
            owner_login: owner_login.to_string(),
            owner_type,
            html_url: "https://github.com/apps/zone-acme-x7k2".to_string(),
            client_id: "Iv1.client".to_string(),
            client_secret: "ENC[v1:client-secret]".to_string(),
            private_key: "ENC[v1:private-key]".to_string(),
            webhook_secret: Some("ENC[v1:webhook-secret]".to_string()),
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn installation(revoked_at: Option<DateTime<Utc>>) -> InstallationRow {
        let now = Utc::now();
        InstallationRow {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            github_app_id: Uuid::new_v4(),
            installation_id: 7,
            account_login: "acme".to_string(),
            account_type: AccountType::Enterprise,
            repository_selection: RepositorySelection::Selected,
            html_url: "https://github.com/organizations/acme/settings/installations/7".to_string(),
            suspended_at: None,
            revoked_at,
            created_by: Some(Uuid::new_v4()),
            created_at: now,
            updated_at: now,
        }
    }

    fn keys(value: &Value) -> BTreeSet<&str> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn an_app_serializes_exactly_the_contract_keys() {
        let row = app("acme", OwnerType::Organization);
        let rendered = serde_json::to_value(App::of(row.clone(), WEB)).unwrap();

        assert_eq!(
            keys(&rendered),
            BTreeSet::from([
                "id",
                "app_id",
                "slug",
                "name",
                "owner_login",
                "owner_type",
                "html_url",
                "settings_url",
                "created_at",
            ])
        );
        assert_eq!(rendered["id"], row.id.to_string());
        assert_eq!(rendered["app_id"], 42);
        assert_eq!(rendered["owner_type"], "Organization");
        assert!(
            DateTime::parse_from_rfc3339(rendered["created_at"].as_str().unwrap()).is_ok(),
            "{rendered}"
        );
        let text = rendered.to_string();
        for secret in [
            "client-secret",
            "private-key",
            "webhook-secret",
            "Iv1.client",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }

    #[test]
    fn an_installation_serializes_exactly_the_contract_keys() {
        let row = installation(None);
        let rendered = serde_json::to_value(Installation::from(row.clone())).unwrap();

        assert_eq!(
            keys(&rendered),
            BTreeSet::from([
                "id",
                "github_app_id",
                "installation_id",
                "account_login",
                "account_type",
                "repository_selection",
                "html_url",
                "suspended_at",
                "revoked_at",
                "created_at",
            ])
        );
        assert_eq!(rendered["github_app_id"], row.github_app_id.to_string());
        assert_eq!(rendered["installation_id"], 7);
        assert_eq!(rendered["account_type"], "Enterprise");
        assert_eq!(rendered["repository_selection"], "selected");
        assert_eq!(rendered["suspended_at"], Value::Null);
        assert_eq!(rendered["revoked_at"], Value::Null);
    }

    #[test]
    fn a_revoked_installation_says_when() {
        let revoked = Utc::now();
        let rendered =
            serde_json::to_value(Installation::from(installation(Some(revoked)))).unwrap();
        let stamp = rendered["revoked_at"].as_str().expect("a timestamp");
        assert_eq!(
            DateTime::parse_from_rfc3339(stamp).unwrap(),
            revoked.fixed_offset()
        );
    }

    #[test]
    fn an_organization_apps_settings_are_under_the_organization() {
        assert_eq!(
            settings_url(
                &app("Acme-Labs", OwnerType::Organization),
                "https://ghe.local/"
            ),
            "https://ghe.local/organizations/Acme-Labs/settings/apps/zone-acme-x7k2"
        );
    }

    #[test]
    fn a_personal_apps_settings_are_under_the_user() {
        assert_eq!(
            settings_url(&app("octocat", OwnerType::User), WEB),
            "https://github.com/settings/apps/zone-acme-x7k2"
        );
    }

    #[test]
    fn an_unparseable_organization_login_falls_back_to_the_user_path() {
        let mut row = app("not a login", OwnerType::Organization);
        row.slug = "zone app".to_string();
        assert_eq!(
            settings_url(&row, WEB),
            "https://github.com/settings/apps/zone%20app"
        );
    }
}
