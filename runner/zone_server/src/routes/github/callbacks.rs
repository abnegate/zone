mod failure;

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{AccountType, InstallationId, RegisteredApplication};
use axum::extract::{Query, State};
use axum::http::header::{ALLOW, CACHE_CONTROL, LOCATION, REFERRER_POLICY};
use axum::http::{HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::connection::AuditedApp;
use super::outcome::{Outcome, location};
use crate::db::audit::{actions, resources};
use crate::db::github_apps::{self, AppRow, NewApp, OwnerType};
use crate::db::github_connection_states::{self, ConsumedState, Purpose};
use crate::db::organization_members::{self, OrgRole};
use crate::db::users;
use crate::routes::common::{AuditEvent, audit};
use crate::services::github_app::address::addressable;
use crate::services::github_app::error::Error as GitHubError;
use crate::services::github_app::installations;
use crate::services::github_app::manifest;
use crate::services::login::callback::page::{NO_REFERRER, NO_STORE};
use crate::state::AppState;
use failure::Failure;

const GET: HeaderValue = HeaderValue::from_static("GET");

#[derive(Debug, Default, Deserialize)]
struct ManifestParameters {
    code: Option<SecretValue>,
    state: Option<SecretValue>,
}

#[derive(Debug, Default, Deserialize)]
struct SetupParameters {
    installation_id: Option<String>,
    setup_action: Option<SetupAction>,
    state: Option<SecretValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SetupAction {
    Install,
    Update,
    Request,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize)]
struct ConnectedInstallation<'a> {
    account_login: &'a str,
    installation_id: i64,
}

/// A `HEAD` would otherwise run the `GET` handler and spend the state without
/// the browser ever following the redirect.
pub async fn refuse() -> Response {
    let mut response = uncached(StatusCode::METHOD_NOT_ALLOWED);
    response.headers_mut().insert(ALLOW, GET);
    response
}

pub async fn manifest(State(state): State<AppState>, uri: Uri) -> Response {
    let parameters: ManifestParameters = received(&uri);
    let base = &state.config().app_base_url;
    let consumed = match consume(&state, parameters.state.as_ref(), Purpose::Registration).await {
        Ok(Some(consumed)) => consumed,
        Ok(None) => return redirect(&location(base, Outcome::Expired, None)),
        Err(error) => {
            tracing::error!(%error, "The GitHub manifest callback could not read its state");
            return redirect(&location(base, Outcome::Failed, None));
        }
    };
    let failed = location(base, Outcome::Failed, Some(consumed.organization_id));

    let application = match convert(&state, &consumed, parameters.code.as_ref()).await {
        Ok(application) => application,
        Err(failure) => {
            tracing::warn!(
                organization = %consumed.organization_id,
                error = %failure,
                "GitHub could not register an App from Zone's manifest"
            );
            return redirect(&failed);
        }
    };

    match register(&state, &consumed, &application).await {
        Ok(installation) => redirect(&installation),
        Err(failure) => {
            tracing::warn!(
                organization = %consumed.organization_id,
                slug = %application.slug,
                html_url = %application.html_url,
                error = %failure,
                "GitHub registered an App Zone could not connect, so it stays on GitHub until deleted there"
            );
            redirect(&failed)
        }
    }
}

pub async fn setup(State(state): State<AppState>, uri: Uri) -> Response {
    let parameters: SetupParameters = received(&uri);
    let base = &state.config().app_base_url;
    let consumed = match consume(&state, parameters.state.as_ref(), Purpose::Installation).await {
        Ok(consumed) => consumed,
        Err(error) => {
            tracing::error!(%error, "The GitHub setup callback could not read its state");
            return redirect(&location(base, Outcome::Failed, None));
        }
    };
    let organization = consumed.as_ref().map(|consumed| consumed.organization_id);

    if parameters.setup_action == Some(SetupAction::Request) {
        return redirect(&location(base, Outcome::Requested, organization));
    }
    let Some(consumed) = consumed else {
        return redirect(&location(base, Outcome::Sync, None));
    };

    let outcome = match install(&state, &consumed, parameters.installation_id.as_deref()).await {
        Ok(()) => Outcome::Installed,
        Err(failure) => {
            tracing::warn!(
                organization = %consumed.organization_id,
                error = %failure,
                "The GitHub App installation could not be connected"
            );
            Outcome::Failed
        }
    };
    redirect(&location(base, outcome, organization))
}

/// What the callback was sent, or nothing at all when the query cannot be
/// read, so a malformed redirect lands like a missing state.
fn received<T: DeserializeOwned + Default>(uri: &Uri) -> T {
    Query::<T>::try_from_uri(uri)
        .map(|Query(parameters)| parameters)
        .unwrap_or_default()
}

async fn consume(
    state: &AppState,
    value: Option<&SecretValue>,
    purpose: Purpose,
) -> Result<Option<ConsumedState>, sqlx::Error> {
    match value.filter(|value| !value.is_empty()) {
        Some(value) => github_connection_states::consume(state.db(), value.expose(), purpose).await,
        None => Ok(None),
    }
}

async fn convert(
    state: &AppState,
    consumed: &ConsumedState,
    code: Option<&SecretValue>,
) -> Result<RegisteredApplication, Failure> {
    let code = code
        .filter(|code| !code.is_empty())
        .ok_or(Failure::MissingCode)?;
    administering(state, consumed).await?;

    Ok(state
        .github()
        .applications()?
        .convert_manifest(code.expose())
        .await
        .map_err(GitHubError::from)?)
}

async fn register(
    state: &AppState,
    consumed: &ConsumedState,
    application: &RegisteredApplication,
) -> Result<String, Failure> {
    let app = store(state, consumed, application).await?;
    recorded(
        state,
        consumed,
        actions::GITHUB_APP_REGISTERED,
        resources::GITHUB_APP,
        app.id,
        serde_json::to_value(AuditedApp::from(&app)).ok(),
    )
    .await;

    let issued = github_connection_states::issue(
        state.db(),
        consumed.organization_id,
        consumed.user_id,
        Purpose::Installation,
        Some(app.id),
    )
    .await?;
    Ok(manifest::installation_url(
        &app.html_url,
        issued.state.expose(),
    ))
}

async fn store(
    state: &AppState,
    consumed: &ConsumedState,
    application: &RegisteredApplication,
) -> Result<AppRow, Failure> {
    let owner_type = match application.owner.kind {
        AccountType::User => OwnerType::User,
        AccountType::Organization => OwnerType::Organization,
        _ => return Err(Failure::UnknownOwnerType),
    };
    let html_url = addressable(&application.html_url)
        .map(String::from)
        .ok_or(Failure::Unaddressable)?;
    let app_id = i64::try_from(application.id.get()).map_err(|_| Failure::OutOfRange)?;
    let provider = state.github();
    let client_secret = provider.seal(&application.client_secret)?;
    let private_key = provider.seal(&application.private_key)?;
    let webhook_secret = application
        .webhook_secret
        .as_ref()
        .map(|secret| provider.seal(secret))
        .transpose()?;

    Ok(github_apps::insert(
        state.db(),
        &NewApp {
            organization_id: consumed.organization_id,
            app_id,
            slug: &application.slug,
            name: &application.name,
            owner_login: &application.owner.login,
            owner_type,
            html_url: &html_url,
            client_id: &application.client_id,
            client_secret: &client_secret,
            private_key: &private_key,
            webhook_secret: webhook_secret.as_deref(),
            created_by: Some(consumed.user_id),
        },
    )
    .await?)
}

async fn install(
    state: &AppState,
    consumed: &ConsumedState,
    installation_id: Option<&str>,
) -> Result<(), Failure> {
    let app_id = consumed.github_app_id.ok_or(Failure::Unbound)?;
    let app = github_apps::get(state.db(), consumed.organization_id, app_id)
        .await?
        .ok_or(Failure::MissingApp)?;
    let number = installation_id
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|number| *number > 0)
        .map(InstallationId::new)
        .ok_or(Failure::MissingInstallation)?;
    administering(state, consumed).await?;

    let provider = state.github();
    let jwt = provider.application_jwt_for(&app)?;
    let installation = provider
        .applications()?
        .fetch_installation(&jwt, number)
        .await
        .map_err(GitHubError::from)?;
    let row =
        installations::record(state.db(), &app, &installation, Some(consumed.user_id)).await?;
    provider.invalidate(number);
    provider.catalog().forget(row.id);

    recorded(
        state,
        consumed,
        actions::GITHUB_INSTALLATION_CONNECTED,
        resources::GITHUB_INSTALLATION,
        row.id,
        serde_json::to_value(ConnectedInstallation {
            account_login: &row.account_login,
            installation_id: row.installation_id,
        })
        .ok(),
    )
    .await;
    Ok(())
}

/// The state outlives a change of role, so the admin who started the
/// connection must still be one when GitHub sends the browser back.
async fn administering(state: &AppState, consumed: &ConsumedState) -> Result<(), Failure> {
    organization_members::get_member(state.db(), consumed.organization_id, consumed.user_id)
        .await?
        .filter(|member| member.is_active && member.role >= OrgRole::Admin)
        .map(|_| ())
        .ok_or(Failure::NotAdmin)
}

/// Audits as the user who started the connection, skipping the entry when
/// that user is gone, since an entry must name its actor.
async fn recorded(
    state: &AppState,
    consumed: &ConsumedState,
    action: &str,
    resource_type: &str,
    resource_id: Uuid,
    new_values: Option<Value>,
) {
    let user = match users::get_user_by_id(state.db(), consumed.user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return,
        Err(error) => {
            tracing::error!(%error, action, "The GitHub connection's actor could not be read for its audit log");
            return;
        }
    };
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(consumed.organization_id),
            workspace_id: None,
            actor_id: consumed.user_id,
            actor_email: &user.email,
            action,
            resource_type,
            resource_id: Some(resource_id),
            old_values: None,
            new_values,
        },
    )
    .await;
}

fn redirect(target: &str) -> Response {
    let Ok(target) = HeaderValue::try_from(target) else {
        tracing::error!("A GitHub callback built a redirect that is not a valid header value");
        return uncached(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let mut response = uncached(StatusCode::SEE_OTHER);
    response.headers_mut().insert(LOCATION, target);
    response
}

fn uncached(status: StatusCode) -> Response {
    let mut response = status.into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static(NO_REFERRER));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(query: &str) -> Uri {
        format!("{}?{query}", manifest::SETUP_CALLBACK_PATH)
            .parse()
            .unwrap()
    }

    #[test]
    fn setup_parameters_read_what_github_sends() {
        let parameters: SetupParameters =
            received(&uri("installation_id=42&setup_action=install&state=a%2Bb"));
        assert_eq!(parameters.installation_id.as_deref(), Some("42"));
        assert_eq!(parameters.setup_action, Some(SetupAction::Install));
        assert_eq!(
            parameters.state.as_ref().map(SecretValue::expose),
            Some("a+b")
        );

        let requested: SetupParameters = received(&uri("setup_action=request"));
        assert_eq!(requested.setup_action, Some(SetupAction::Request));
        assert_eq!(requested.state, None);

        let unknown: SetupParameters = received(&uri("setup_action=uninstall"));
        assert_eq!(unknown.setup_action, Some(SetupAction::Unknown));
    }

    #[test]
    fn an_unreadable_query_reads_as_nothing_sent() {
        let parameters: ManifestParameters = received(&uri("state=one&state=two&code=x"));
        assert_eq!(parameters.state, None);
        assert_eq!(parameters.code, None);
    }

    #[test]
    fn manifest_parameters_read_what_github_sends() {
        let parameters: ManifestParameters = received(&uri("code=a%2Bb&state=c%2Fd"));
        assert_eq!(
            parameters.code.as_ref().map(SecretValue::expose),
            Some("a+b")
        );
        assert_eq!(
            parameters.state.as_ref().map(SecretValue::expose),
            Some("c/d")
        );
    }

    #[test]
    fn debug_never_shows_the_manifest_code_or_state() {
        let parameters: ManifestParameters =
            received(&uri("code=plaintext-code&state=plaintext-state"));
        let rendered = format!("{parameters:?}");
        assert!(!rendered.contains("plaintext-code"), "{rendered}");
        assert!(!rendered.contains("plaintext-state"), "{rendered}");
    }

    #[test]
    fn debug_never_shows_the_setup_state() {
        let parameters: SetupParameters = received(&uri(
            "installation_id=42&setup_action=install&state=plaintext-state",
        ));
        let rendered = format!("{parameters:?}");
        assert!(!rendered.contains("plaintext-state"), "{rendered}");
        assert!(rendered.contains("42"), "{rendered}");
    }

    #[test]
    fn every_response_is_uncached_and_sends_no_referrer() {
        for response in [
            redirect(&location("https://zone.example.com", Outcome::Sync, None)),
            redirect("https://zone.example.com/\n"),
            uncached(StatusCode::METHOD_NOT_ALLOWED),
        ] {
            assert_eq!(response.headers()[CACHE_CONTROL], NO_STORE);
            assert_eq!(response.headers()[REFERRER_POLICY], NO_REFERRER);
        }
    }

    #[tokio::test]
    async fn a_refused_head_names_the_method_allowed() {
        let response = refuse().await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers().get(ALLOW), Some(&GET));
        assert!(response.headers().get(LOCATION).is_none());
    }
}
