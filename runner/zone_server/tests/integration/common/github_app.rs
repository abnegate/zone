//! A GitHub App and its installation, seeded straight into the database, for
//! tests of the routes that attach sources and projects to an installation,
//! and a stand-in GitHub that answers what the connection routes ask of it.

pub use zone_server::services::github_app::testing::TEST_PRIVATE_KEY;

use std::sync::Arc;

use abnegate_secret::{SecretValue, encrypt_value};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate, Times};
use zone_server::config::Config;
use zone_server::db::github_apps::{self, AppRow, NewApp, OwnerType};
use zone_server::db::github_installations::{
    self, AccountType, InstallationUpsert, RepositorySelection,
};
use zone_server::services::github_app::Provider;
use zone_server::state::AppState;

use super::test_config;

pub const APP_ID: i64 = 424_242;

/// GitHub's web origin in the test config, which only ever appears in URLs
/// handed to the browser.
pub const GITHUB_WEB_URL: &str = "https://github.com";

const COMMIT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const TOKEN_LIFETIME_HOURS: i64 = 1;

/// A GitHub installation number no other test uses, since the database holds
/// each of GitHub's installations once.
pub fn installation_id() -> i64 {
    rand::random_range(1..i64::MAX)
}

/// A GitHub App number for a freshly registered App.
pub fn app_id() -> i64 {
    rand::random_range(1..i64::from(i32::MAX))
}

/// A GitHub login no other test uses, starting with `prefix`.
pub fn login(prefix: &str) -> String {
    format!("{prefix}-{}", &opaque()[..12])
}

/// A fresh opaque value of the kind GitHub hands out as a code or a secret.
pub fn opaque() -> String {
    Uuid::new_v4().simple().to_string()
}

pub struct Seeded {
    pub app: Uuid,
    pub installation: Uuid,
    pub installation_id: i64,
    pub account_login: String,
}

/// An App GitHub numbers `app_id`, registered by `organization_id` for
/// `owner_login`, its secrets sealed under the state's key the way
/// `Provider::seal` seals them, with [`TEST_PRIVATE_KEY`] as its key.
pub async fn seed_app(
    state: &AppState,
    organization_id: Uuid,
    app_id: i64,
    owner_login: &str,
) -> AppRow {
    let seal = |secret: &str| {
        encrypt_value(&SecretValue::new(secret), state.master_key()).expect("a test secret seals")
    };
    let private_key = seal(TEST_PRIVATE_KEY);
    let client_secret = seal("throwaway-client-secret");
    let html_url = format!("{GITHUB_WEB_URL}/apps/{owner_login}");

    github_apps::insert(
        state.db(),
        &NewApp {
            organization_id,
            app_id,
            slug: owner_login,
            name: owner_login,
            owner_login,
            owner_type: OwnerType::Organization,
            html_url: &html_url,
            client_id: "Iv1.throwaway",
            client_secret: &client_secret,
            private_key: &private_key,
            webhook_secret: None,
            created_by: None,
        },
    )
    .await
    .expect("the organization registers its App")
}

/// An App registered by `organization_id` and one active installation of it,
/// on an account with a fresh login.
pub async fn seed_installation(
    state: &AppState,
    organization_id: Uuid,
    installation_id: i64,
) -> Seeded {
    let login = login("zone");
    let app = seed_app(state, organization_id, APP_ID, &login).await;

    let installation = github_installations::upsert(
        state.db(),
        &InstallationUpsert {
            organization_id,
            github_app_id: app.id,
            installation_id,
            account_login: &login,
            account_type: AccountType::Organization,
            repository_selection: RepositorySelection::All,
            html_url: &format!(
                "{GITHUB_WEB_URL}/organizations/{login}/settings/installations/{installation_id}"
            ),
            suspended_at: None,
            created_by: None,
        },
    )
    .await
    .expect("the App is installed on the account");

    Seeded {
        app: app.id,
        installation: installation.id,
        installation_id,
        account_login: login,
    }
}

/// `server` confirms, once, that installation `installation_id` no longer
/// exists while the App itself is still answered for.
pub async fn uninstalled(server: &MockServer, installation_id: i64) {
    Mock::given(method("GET"))
        .and(path(format!("/app/installations/{installation_id}")))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/app/installations"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(server)
        .await;
}

/// A provider that mints installation tokens from `server` instead of GitHub.
pub fn issuer_provider(server: &MockServer, state: &AppState) -> Provider {
    Provider::new(&config_for(server), Arc::clone(state.master_key()))
}

/// The test config with every GitHub API call sent to `server`.
pub fn config_for(server: &MockServer) -> Config {
    let mut config = test_config();
    config.github_api_url = server.uri();
    config.github_api_allows_http = true;
    config
}

/// Matches a request signed with a JWT that App `0` issued, so a test proves
/// the route spoke for the App it was meant to.
pub struct ApplicationJwt(pub i64);

impl Match for ApplicationJwt {
    fn matches(&self, request: &Request) -> bool {
        request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|jwt| jwt.split('.').nth(1))
            .and_then(|payload| URL_SAFE_NO_PAD.decode(payload).ok())
            .and_then(|claims| serde_json::from_slice::<Value>(&claims).ok())
            .is_some_and(|claims| claims["iss"] == json!(self.0.to_string()))
    }
}

/// What GitHub hands back when it converts a manifest into an App. Its key is
/// [`TEST_PRIVATE_KEY`], so every JWT Zone signs with it later verifies.
pub struct Registration {
    pub app_id: i64,
    pub slug: String,
    pub name: String,
    pub owner_login: String,
    pub owner_type: AccountType,
    pub client_id: String,
    pub client_secret: String,
    pub webhook_secret: String,
}

impl Registration {
    pub fn new(owner_login: &str, owner_type: AccountType) -> Self {
        let suffix = &opaque()[..4];
        Self {
            app_id: app_id(),
            slug: format!("zone-{}-{suffix}", owner_login.to_lowercase()),
            name: format!("Zone {owner_login} {suffix}"),
            owner_login: owner_login.to_string(),
            owner_type,
            client_id: format!("Iv1.{}", &opaque()[..16]),
            client_secret: format!("client-secret-{}", opaque()),
            webhook_secret: format!("webhook-secret-{}", opaque()),
        }
    }

    pub fn html_url(&self) -> String {
        format!("{GITHUB_WEB_URL}/apps/{}", self.slug)
    }

    pub fn conversion(&self) -> Value {
        json!({
            "id": self.app_id,
            "slug": self.slug,
            "node_id": "MDxOkludGVncmF0aW9uMQ==",
            "name": self.name,
            "owner": {
                "login": self.owner_login,
                "id": 1,
                "type": self.owner_type.as_str(),
                "site_admin": false,
            },
            "description": "",
            "external_url": "http://localhost:3000",
            "html_url": self.html_url(),
            "created_at": "2026-11-06T00:00:00Z",
            "updated_at": "2026-11-06T00:00:00Z",
            "permissions": { "metadata": "read", "contents": "write" },
            "events": [],
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "webhook_secret": self.webhook_secret,
            "pem": TEST_PRIVATE_KEY,
        })
    }

    pub fn answer(&self) -> ResponseTemplate {
        ResponseTemplate::new(201).set_body_json(self.conversion())
    }
}

/// One installation of an App, as GitHub describes it.
pub struct Installed {
    pub installation_id: i64,
    pub app_id: i64,
    pub account_login: String,
    pub account_type: AccountType,
    pub repository_selection: RepositorySelection,
}

impl Installed {
    pub fn new(installation_id: i64, app_id: i64, account_login: &str) -> Self {
        Self {
            installation_id,
            app_id,
            account_login: account_login.to_string(),
            account_type: AccountType::Organization,
            repository_selection: RepositorySelection::Selected,
        }
    }

    pub fn html_url(&self) -> String {
        match self.account_type {
            AccountType::Organization => format!(
                "{GITHUB_WEB_URL}/organizations/{}/settings/installations/{}",
                self.account_login, self.installation_id
            ),
            _ => format!(
                "{GITHUB_WEB_URL}/settings/installations/{}",
                self.installation_id
            ),
        }
    }

    pub fn row(&self) -> Value {
        json!({
            "id": self.installation_id,
            "account": {
                "login": self.account_login,
                "id": 1,
                "type": self.account_type.as_str(),
                "site_admin": false,
            },
            "access_tokens_url": format!(
                "https://api.github.com/app/installations/{}/access_tokens",
                self.installation_id
            ),
            "repositories_url": "https://api.github.com/installation/repositories",
            "html_url": self.html_url(),
            "app_id": self.app_id,
            "target_id": 1,
            "target_type": self.account_type.as_str(),
            "permissions": { "metadata": "read", "contents": "write" },
            "events": [],
            "repository_selection": self.repository_selection.as_str(),
            "created_at": "2026-11-06T00:00:00Z",
            "updated_at": "2026-11-06T00:00:00Z",
            "app_slug": "zone",
            "suspended_at": null,
            "suspended_by": null,
        })
    }

    pub fn answer(&self) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(self.row())
    }
}

/// A repository an installation can reach, as GitHub lists it.
pub struct ListedRepository {
    pub id: u64,
    pub owner: String,
    pub name: String,
    pub private: bool,
    pub archived: bool,
    pub default_branch: String,
}

impl ListedRepository {
    pub fn new(id: u64, owner: &str, name: &str) -> Self {
        Self {
            id,
            owner: owner.to_string(),
            name: name.to_string(),
            private: true,
            archived: false,
            default_branch: "main".to_string(),
        }
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn html_url(&self) -> String {
        format!("{GITHUB_WEB_URL}/{}", self.full_name())
    }

    pub fn row(&self) -> Value {
        json!({
            "id": self.id,
            "node_id": "MDEwOlJlcG9zaXRvcnkxMjk2MjY5",
            "name": self.name,
            "full_name": self.full_name(),
            "owner": { "login": self.owner, "id": 1, "type": "Organization", "site_admin": false },
            "private": self.private,
            "archived": self.archived,
            "disabled": false,
            "fork": false,
            "html_url": self.html_url(),
            "default_branch": self.default_branch,
            "visibility": if self.private { "private" } else { "public" },
        })
    }
}

/// A branch of a repository, as GitHub lists it.
pub struct ListedBranch {
    pub name: String,
    pub protected: bool,
}

impl ListedBranch {
    pub fn new(name: &str, protected: bool) -> Self {
        Self {
            name: name.to_string(),
            protected,
        }
    }

    pub fn row(&self) -> Value {
        json!({
            "name": self.name,
            "commit": {
                "sha": COMMIT_SHA,
                "url": format!("https://api.github.com/repos/acme/widgets/commits/{COMMIT_SHA}"),
            },
            "protected": self.protected,
        })
    }
}

/// GitHub answers the conversion of the manifest `code` with `answer`.
pub async fn converting(
    server: &MockServer,
    code: &str,
    answer: ResponseTemplate,
    times: impl Into<Times>,
) {
    Mock::given(method("POST"))
        .and(path(format!("/app-manifests/{code}/conversions")))
        .respond_with(answer)
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub answers App `app_id`'s request for installation `installation_id`
/// with `answer`.
pub async fn fetching_installation(
    server: &MockServer,
    app_id: i64,
    installation_id: i64,
    answer: ResponseTemplate,
    times: impl Into<Times>,
) {
    Mock::given(method("GET"))
        .and(path(format!("/app/installations/{installation_id}")))
        .and(ApplicationJwt(app_id))
        .respond_with(answer)
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub lists `installations`, all on the first page, to App `app_id`.
pub async fn listing_installations(
    server: &MockServer,
    app_id: i64,
    installations: &[Installed],
    times: impl Into<Times>,
) {
    let rows: Vec<Value> = installations.iter().map(Installed::row).collect();
    Mock::given(method("GET"))
        .and(path("/app/installations"))
        .and(query_param("page", "1"))
        .and(ApplicationJwt(app_id))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows))
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub mints `token` for App `app_id`'s installation `installation_id`.
pub async fn issuing(
    server: &MockServer,
    app_id: i64,
    installation_id: i64,
    token: &str,
    times: impl Into<Times>,
) {
    let expires_at = (Utc::now() + Duration::hours(TOKEN_LIFETIME_HOURS))
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    Mock::given(method("POST"))
        .and(path(format!(
            "/app/installations/{installation_id}/access_tokens"
        )))
        .and(ApplicationJwt(app_id))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token": token,
            "expires_at": expires_at,
        })))
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub lists `repositories`, all on the first page, to whoever holds `token`.
pub async fn listing_repositories(
    server: &MockServer,
    token: &str,
    repositories: &[ListedRepository],
    times: impl Into<Times>,
) {
    let rows: Vec<Value> = repositories.iter().map(ListedRepository::row).collect();
    Mock::given(method("GET"))
        .and(path("/installation/repositories"))
        .and(query_param("page", "1"))
        .and(header("authorization", format!("Bearer {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": rows.len(),
            "repository_selection": "selected",
            "repositories": rows,
        })))
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub lists `branches` of `owner/repository`, all on the first page, to
/// whoever holds `token`.
pub async fn listing_branches(
    server: &MockServer,
    token: &str,
    owner: &str,
    repository: &str,
    branches: &[ListedBranch],
    times: impl Into<Times>,
) {
    let rows: Vec<Value> = branches.iter().map(ListedBranch::row).collect();
    Mock::given(method("GET"))
        .and(path(format!("/repos/{owner}/{repository}/branches")))
        .and(query_param("page", "1"))
        .and(header("authorization", format!("Bearer {token}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows))
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub answers App `app_id`'s request to delete installation
/// `installation_id` with `answer`.
pub async fn deleting_installation(
    server: &MockServer,
    app_id: i64,
    installation_id: i64,
    answer: ResponseTemplate,
    times: impl Into<Times>,
) {
    Mock::given(method("DELETE"))
        .and(path(format!("/app/installations/{installation_id}")))
        .and(ApplicationJwt(app_id))
        .respond_with(answer)
        .expect(times)
        .mount(server)
        .await;
}
