use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::DateTime;
use reqwest::Url;
use serde_json::{Value, json};
use sqlx::PgPool;
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;
use wiremock::{MockServer, ResponseTemplate};
use zone_server::db::github_apps::{self, AppRow, OwnerType};
use zone_server::db::github_connection_states::{self, Purpose};
use zone_server::db::github_installations::{
    self, AccountType, InstallationRow, RepositorySelection,
};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::services::github_app::manifest::{MANIFEST_CALLBACK_PATH, SETUP_CALLBACK_PATH};

use crate::common::github_app::{
    self, GITHUB_WEB_URL, Installed, ListedBranch, ListedRepository, Registration, TEST_PRIVATE_KEY,
};
use crate::common::{TestClient, TestResponse, test_email, test_password};

const SETTINGS_PATH: &str = "/settings/organization/github";
const OUTCOME_PARAMETER: &str = "github";
const ORGANIZATION_PARAMETER: &str = "organization";
const STATE_PARAMETER: &str = "state";
const CODE_PARAMETER: &str = "code";
const INSTALLATION_PARAMETER: &str = "installation_id";
const SETUP_ACTION_PARAMETER: &str = "setup_action";
const INSTALL: &str = "install";
const REQUEST: &str = "request";
const NEW_INSTALLATION_PATH: &str = "installations/new";
const NO_STORE: &str = "no-store";
const NO_REFERRER: &str = "no-referrer";
const MINTED_TOKEN_PREFIX: &str = "ghs_";
const JWT_PREFIX: &str = "eyJ";
const PEM_MARKER: &str = "PRIVATE KEY";
const CAPTURED_TARGETS: &str = "zone_server=trace,tower_http=trace";
const BEARER_PREFIX: &str = "Bearer ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Installed,
    Requested,
    Sync,
    Expired,
    Failed,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Requested => "requested",
            Self::Sync => "sync",
            Self::Expired => "expired",
            Self::Failed => "failed",
        }
    }
}

struct Person {
    token: String,
    user: Uuid,
}

struct Tenant {
    token: String,
    user: Uuid,
    organization: Uuid,
}

impl Tenant {
    fn github(&self) -> String {
        format!("/api/organizations/{}/github", self.organization)
    }
}

/// One client whose GitHub is a mock server answering only what a test
/// mounts, so nothing reaches github.com and an unexpected call fails loudly.
struct Harness {
    client: TestClient,
    github: MockServer,
}

impl Harness {
    async fn new() -> Self {
        let github = MockServer::start().await;
        let client = TestClient::with_config(github_app::config_for(&github)).await;
        Self { client, github }
    }

    fn pool(&self) -> &PgPool {
        self.client.state().db()
    }

    fn app_base_url(&self) -> &str {
        self.client
            .state()
            .config()
            .app_base_url
            .trim_end_matches('/')
    }

    async fn person(&self) -> Person {
        let registered = self
            .client
            .post_json(
                "/api/auth/register",
                &json!({ "email": test_email(), "password": test_password() }),
            )
            .await
            .json_value();
        let token = registered["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("register returns an access token, got {registered}"))
            .to_string();
        let user = registered["user"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("register returns the user, got {registered}"));
        Person { token, user }
    }

    /// A fresh person and the organization they created, which they own.
    async fn tenant(&self) -> Tenant {
        let Person { token, user } = self.person().await;
        let organization = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "GitHub Org", "slug": format!("github-org-{}", github_app::opaque()) }),
                &token,
            )
            .await
            .json_value()["organization"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("an organization");
        Tenant {
            token,
            user,
            organization,
        }
    }

    async fn joined(&self, tenant: &Tenant, role: OrgRole) -> Person {
        let person = self.person().await;
        organization_members::add_member(
            self.pool(),
            tenant.organization,
            person.user,
            role,
            Some(tenant.user),
        )
        .await
        .expect("the person joins the organization");
        person
    }

    async fn issue(
        &self,
        organization: Uuid,
        user: Uuid,
        purpose: Purpose,
        app: Option<Uuid>,
    ) -> String {
        github_connection_states::issue(self.pool(), organization, user, purpose, app)
            .await
            .expect("a state is issued")
            .state
            .expose()
            .to_string()
    }

    async fn registration_state(&self, tenant: &Tenant) -> String {
        self.issue(
            tenant.organization,
            tenant.user,
            Purpose::Registration,
            None,
        )
        .await
    }

    async fn installation_state(&self, tenant: &Tenant, app: Uuid) -> String {
        self.issue(
            tenant.organization,
            tenant.user,
            Purpose::Installation,
            Some(app),
        )
        .await
    }

    async fn still_consumable(&self, state: &str, purpose: Purpose) -> bool {
        github_connection_states::consume(self.pool(), state, purpose)
            .await
            .expect("the state is readable")
            .is_some()
    }

    async fn seed_app(&self, tenant: &Tenant) -> AppRow {
        github_app::seed_app(
            self.client.state(),
            tenant.organization,
            github_app::app_id(),
            &github_app::login("acme"),
        )
        .await
    }

    async fn apps(&self, organization: Uuid) -> Vec<AppRow> {
        github_apps::list(self.pool(), organization)
            .await
            .expect("the organization's apps are readable")
    }

    async fn installations(&self, organization: Uuid) -> Vec<InstallationRow> {
        github_installations::list(self.pool(), organization)
            .await
            .expect("the organization's installations are readable")
    }

    async fn manifest_callback(&self, parameters: &[(&str, &str)]) -> TestResponse {
        self.client
            .get(&format!("{MANIFEST_CALLBACK_PATH}?{}", query(parameters)))
            .await
    }

    async fn setup_callback(&self, parameters: &[(&str, &str)]) -> TestResponse {
        self.client
            .get(&format!("{SETUP_CALLBACK_PATH}?{}", query(parameters)))
            .await
    }

    async fn register(&self, tenant: &Tenant, owner: &str) -> String {
        let response = self
            .client
            .post_json_auth(
                &format!("{}/apps", tenant.github()),
                &json!({ "owner": owner }),
                &tenant.token,
            )
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()["state"]
            .as_str()
            .expect("registration hands back its state")
            .to_string()
    }

    async fn connection(&self, tenant: &Tenant) -> Value {
        let response = self.client.get_auth(&tenant.github(), &tenant.token).await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    /// A POST the way the frontend sends it to #5 and #6: a JSON content type
    /// and no body at all.
    async fn post_empty(&self, uri: &str, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("a request");
        self.client.send_request(request).await
    }

    async fn assert_github_untouched(&self) {
        let received = self.github.received_requests().await.unwrap_or_default();
        assert!(
            received.is_empty(),
            "nothing may be asked of GitHub, but {} requests were: {:?}",
            received.len(),
            received
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>()
        );
    }

    /// `response` sends the browser back to the organization's GitHub settings
    /// with `outcome`, naming `organization` only when a state was consumed.
    fn assert_lands(&self, response: &TestResponse, outcome: Outcome, organization: Option<Uuid>) {
        let url = location(response);
        let base = Url::parse(self.app_base_url()).expect("the app base URL parses");
        assert_eq!(
            url.origin(),
            base.origin(),
            "the browser returns to Zone: {url}"
        );
        assert_eq!(url.path(), SETTINGS_PATH, "{url}");

        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs.first().map(|(name, _)| name.as_str()),
            Some(OUTCOME_PARAMETER),
            "the outcome opens the query: {url}"
        );
        let mut expected =
            BTreeMap::from([(OUTCOME_PARAMETER.to_string(), outcome.as_str().to_string())]);
        if let Some(organization) = organization {
            expected.insert(ORGANIZATION_PARAMETER.to_string(), organization.to_string());
        }
        let received: BTreeMap<String, String> = pairs.iter().cloned().collect();
        assert_eq!(received, expected, "{url}");
        assert_eq!(
            pairs.len(),
            expected.len(),
            "no parameter appears twice: {url}"
        );
    }
}

fn query(parameters: &[(&str, &str)]) -> String {
    parameters
        .iter()
        .map(|(name, value)| format!("{name}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn location(response: &TestResponse) -> Url {
    response.assert_status(StatusCode::SEE_OTHER);
    let location = response
        .header(header::LOCATION.as_str())
        .expect("a redirect names where it goes");
    Url::parse(location).unwrap_or_else(|error| panic!("{location} is absolute ({error})"))
}

/// The installation state in `url`, which must be App `html_url`'s new
/// installation page carrying that state and nothing else.
fn installation_page_state(url: &Url, html_url: &str) -> String {
    let page = Url::parse(&format!("{html_url}/{NEW_INSTALLATION_PATH}"))
        .expect("the installation page parses");
    assert_eq!(
        (url.origin(), url.path()),
        (page.origin(), page.path()),
        "{url} is the App's new installation page"
    );
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    match pairs.as_slice() {
        [(name, state)] if name == STATE_PARAMETER && !state.is_empty() => state.clone(),
        _ => panic!("{url} carries the installation state and nothing else"),
    }
}

fn assert_installing(response: &TestResponse, html_url: &str) -> String {
    installation_page_state(&location(response), html_url)
}

fn assert_uncached(response: &TestResponse, label: &str) {
    assert_eq!(
        response.header(header::CACHE_CONTROL.as_str()),
        Some(NO_STORE),
        "{label} is never cached"
    );
    assert_eq!(
        response.header(header::REFERRER_POLICY.as_str()),
        Some(NO_REFERRER),
        "{label} sends no referrer onward"
    );
}

/// `value` without its `created_at`, which must be an RFC 3339 timestamp.
fn unstamped(value: &Value) -> Value {
    let mut value = value.clone();
    let created_at = value
        .as_object_mut()
        .and_then(|object| object.remove("created_at"))
        .unwrap_or_else(|| panic!("{value} carries created_at"));
    let stamp = created_at.as_str().unwrap_or_default();
    assert!(
        DateTime::parse_from_rfc3339(stamp).is_ok(),
        "created_at {created_at} is RFC 3339"
    );
    value
}

fn uuid_of(value: &Value) -> Uuid {
    value
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("{value} is a uuid"))
}

fn repository_json(installation: Uuid, repository: &ListedRepository) -> Value {
    json!({
        "installation_id": installation,
        "id": repository.id,
        "owner": repository.owner,
        "name": repository.name,
        "full_name": repository.full_name(),
        "private": repository.private,
        "archived": repository.archived,
        "default_branch": repository.default_branch,
        "html_url": repository.html_url(),
    })
}

fn pem_body() -> &'static str {
    TEST_PRIVATE_KEY
        .lines()
        .nth(1)
        .expect("the test key has a body")
}

#[tokio::test]
async fn flow_manifest_install_sync_repositories_branches_uninstall() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let github = tenant.github();
    let owner = github_app::login("acme");

    let response = harness
        .client
        .post_json_auth(
            &format!("{github}/apps"),
            &json!({ "owner": owner }),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let registered = response.json_value();
    let state = registered["state"]
        .as_str()
        .expect("registration hands back its state")
        .to_string();
    let action = Url::parse(registered["action"].as_str().expect("an action"))
        .expect("the action is absolute");
    let new_app = Url::parse(&format!(
        "{GITHUB_WEB_URL}/organizations/{owner}/settings/apps/new"
    ))
    .expect("the new app page parses");
    assert_eq!(
        (action.origin(), action.path()),
        (new_app.origin(), new_app.path()),
        "an organization's App is registered on that organization's new app page"
    );
    assert_eq!(
        action.query_pairs().into_owned().collect::<Vec<_>>(),
        vec![(STATE_PARAMETER.to_string(), state.clone())],
        "the action carries the registration state"
    );
    let manifest: Value =
        serde_json::from_str(registered["manifest"].as_str().expect("a manifest"))
            .expect("the manifest is JSON");
    let app_base_url = harness.app_base_url();
    assert_eq!(manifest["url"], json!(app_base_url));
    assert_eq!(
        manifest["redirect_url"],
        json!(format!("{app_base_url}{MANIFEST_CALLBACK_PATH}"))
    );
    assert_eq!(
        manifest["setup_url"],
        json!(format!("{app_base_url}{SETUP_CALLBACK_PATH}"))
    );
    assert_eq!(manifest["public"], json!(false));
    assert!(manifest.get("hook_attributes").is_none(), "{manifest}");

    let registration = Registration::new(&owner, AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;
    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;
    let installation_state = assert_installing(&response, &registration.html_url());
    assert_uncached(&response, "the manifest callback");
    assert_ne!(
        installation_state, state,
        "installing gets a state of its own"
    );

    let apps = harness.apps(tenant.organization).await;
    let [app] = apps.as_slice() else {
        panic!(
            "the conversion registers exactly one App, got {}",
            apps.len()
        );
    };
    assert_eq!(
        (
            app.app_id,
            app.slug.as_str(),
            app.owner_login.as_str(),
            app.owner_type,
            app.html_url.as_str(),
            app.created_by,
        ),
        (
            registration.app_id,
            registration.slug.as_str(),
            owner.as_str(),
            OwnerType::Organization,
            registration.html_url().as_str(),
            Some(tenant.user),
        )
    );
    let provider = harness.client.state().github();
    let webhook_secret = app
        .webhook_secret
        .as_deref()
        .expect("the webhook secret is kept");
    for (label, sealed, plaintext) in [
        (
            "client secret",
            app.client_secret.as_str(),
            registration.client_secret.as_str(),
        ),
        ("private key", app.private_key.as_str(), TEST_PRIVATE_KEY),
        (
            "webhook secret",
            webhook_secret,
            registration.webhook_secret.as_str(),
        ),
    ] {
        assert!(
            !sealed.contains(plaintext) && !sealed.contains(PEM_MARKER),
            "the {label} is stored sealed"
        );
        assert_eq!(
            provider
                .open(sealed)
                .unwrap_or_else(|_| panic!("the {label} opens under Zone's key"))
                .expose(),
            plaintext,
            "the {label} is GitHub's"
        );
    }

    let connection = harness.connection(&tenant).await;
    assert_eq!(
        connection["apps"].as_array().map(Vec::len),
        Some(1),
        "{connection}"
    );
    assert_eq!(
        unstamped(&connection["apps"][0]),
        json!({
            "id": app.id,
            "app_id": registration.app_id,
            "slug": registration.slug,
            "name": registration.name,
            "owner_login": owner,
            "owner_type": OwnerType::Organization.as_str(),
            "html_url": registration.html_url(),
            "settings_url": format!(
                "{GITHUB_WEB_URL}/organizations/{owner}/settings/apps/{}",
                registration.slug
            ),
        })
    );
    assert_eq!(connection["installations"], json!([]));

    let installation_id = github_app::installation_id();
    let installed = Installed::new(installation_id, registration.app_id, &owner);
    github_app::fetching_installation(
        &harness.github,
        registration.app_id,
        installation_id,
        installed.answer(),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &installation_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Installed, Some(tenant.organization));
    assert_uncached(&response, "the setup callback");

    let connection = harness.connection(&tenant).await;
    assert_eq!(
        connection["installations"].as_array().map(Vec::len),
        Some(1),
        "{connection}"
    );
    let installation = uuid_of(&connection["installations"][0]["id"]);
    assert_eq!(
        unstamped(&connection["installations"][0]),
        json!({
            "id": installation,
            "github_app_id": app.id,
            "installation_id": installation_id,
            "account_login": owner,
            "account_type": AccountType::Organization.as_str(),
            "repository_selection": RepositorySelection::Selected.as_str(),
            "html_url": installed.html_url(),
            "suspended_at": null,
            "revoked_at": null,
        })
    );

    let response = harness
        .post_empty(
            &format!("{github}/apps/{}/installations", app.id),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let url = Url::parse(
        response.json_value()["url"]
            .as_str()
            .expect("an installation url"),
    )
    .expect("the installation url is absolute");
    let reinstallation_state = installation_page_state(&url, &registration.html_url());
    assert_ne!(reinstallation_state, installation_state);

    let mut resynced = Installed::new(installation_id, registration.app_id, &owner);
    resynced.repository_selection = RepositorySelection::All;
    github_app::listing_installations(&harness.github, registration.app_id, &[resynced], 1).await;
    let response = harness
        .post_empty(&format!("{github}/installations/sync"), &tenant.token)
        .await;
    response.assert_status(StatusCode::NO_CONTENT);
    let connection = harness.connection(&tenant).await;
    assert_eq!(
        connection["installations"][0]["repository_selection"],
        json!(RepositorySelection::All.as_str()),
        "sync records what GitHub says now: {connection}"
    );
    assert_eq!(connection["installations"][0]["revoked_at"], Value::Null);

    let token = format!("{MINTED_TOKEN_PREFIX}{}", github_app::opaque());
    github_app::issuing(
        &harness.github,
        registration.app_id,
        installation_id,
        &token,
        1,
    )
    .await;
    let mut gadgets = ListedRepository::new(8, &owner, "Gadgets");
    gadgets.private = false;
    gadgets.default_branch = "trunk".to_string();
    let repositories = [ListedRepository::new(7, &owner, "widgets"), gadgets];
    github_app::listing_repositories(&harness.github, &token, &repositories, 1).await;
    let catalog_installations = json!([{
        "id": installation,
        "account_login": owner,
        "html_url": installed.html_url(),
        "error": null,
    }]);

    let response = harness
        .client
        .get_auth(
            &format!("{github}/repositories?page=1&per_page=50"),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({
            "repositories": [
                repository_json(installation, &repositories[1]),
                repository_json(installation, &repositories[0]),
            ],
            "installations": catalog_installations,
            "total": 2,
            "next_page": null,
        }),
        "every repository, sorted case-insensitively by name"
    );

    let response = harness
        .client
        .get_auth(
            &format!("{github}/repositories?query=+WID+&page=1&per_page=50"),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({
            "repositories": [repository_json(installation, &repositories[0])],
            "installations": catalog_installations,
            "total": 1,
            "next_page": null,
        }),
        "the trimmed query matches case-insensitively, from the cached catalog"
    );

    github_app::listing_branches(
        &harness.github,
        &token,
        &owner,
        "widgets",
        &[
            ListedBranch::new("main", true),
            ListedBranch::new("feature/picker", false),
        ],
        1,
    )
    .await;
    let response = harness
        .client
        .get_auth(
            &format!(
                "{github}/installations/{installation}/repositories/{owner}/widgets/branches?page=1"
            ),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({
            "branches": [
                { "name": "main", "protected": true },
                { "name": "feature/picker", "protected": false },
            ],
            "next_page": null,
        })
    );

    github_app::deleting_installation(
        &harness.github,
        registration.app_id,
        installation_id,
        ResponseTemplate::new(204),
        1,
    )
    .await;
    let response = harness
        .client
        .delete_auth(
            &format!("{github}/installations/{installation}?uninstall=true"),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::NO_CONTENT);
    let connection = harness.connection(&tenant).await;
    assert_eq!(connection["installations"], json!([]), "{connection}");
    assert_eq!(
        connection["apps"].as_array().map(Vec::len),
        Some(1),
        "{connection}"
    );

    let response = harness
        .client
        .delete_auth(&format!("{github}/apps/{}", app.id), &tenant.token)
        .await;
    response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        harness.connection(&tenant).await,
        json!({ "apps": [], "installations": [] })
    );
    assert!(harness.apps(tenant.organization).await.is_empty());
}

#[tokio::test]
async fn callback_manifest_replayed_state_expires() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let state = harness.registration_state(&tenant).await;
    let registration = Registration::new(&github_app::login("acme"), AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;
    let parameters = [(CODE_PARAMETER, code.as_str()), (STATE_PARAMETER, &state)];

    let first = harness.manifest_callback(&parameters).await;
    assert_installing(&first, &registration.html_url());

    let replayed = harness.manifest_callback(&parameters).await;
    harness.assert_lands(&replayed, Outcome::Expired, None);
    assert_eq!(
        harness.apps(tenant.organization).await.len(),
        1,
        "the replay registers nothing more"
    );
}

#[tokio::test]
async fn callback_manifest_missing_state_expires() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let code = github_app::opaque();
    let unknown = github_app::opaque();
    let installing = harness
        .issue(
            tenant.organization,
            tenant.user,
            Purpose::Installation,
            None,
        )
        .await;

    for (label, parameters) in [
        ("no state", vec![(CODE_PARAMETER, code.as_str())]),
        (
            "an unknown state",
            vec![(CODE_PARAMETER, code.as_str()), (STATE_PARAMETER, &unknown)],
        ),
        (
            "an installation state",
            vec![
                (CODE_PARAMETER, code.as_str()),
                (STATE_PARAMETER, &installing),
            ],
        ),
    ] {
        let response = harness.manifest_callback(&parameters).await;
        harness.assert_lands(&response, Outcome::Expired, None);
        assert_uncached(&response, label);
    }

    assert!(harness.apps(tenant.organization).await.is_empty());
    harness.assert_github_untouched().await;
    assert!(
        harness
            .still_consumable(&installing, Purpose::Installation)
            .await,
        "a state for another purpose is left alone"
    );
}

#[tokio::test]
async fn callback_manifest_duplicate_owner_fails() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let existing = harness.seed_app(&tenant).await;
    let state = harness.registration_state(&tenant).await;
    let registration = Registration::new(
        &existing.owner_login.to_uppercase(),
        AccountType::Organization,
    );
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;

    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    assert_uncached(&response, "a duplicate owner");
    let apps = harness.apps(tenant.organization).await;
    assert_eq!(
        apps.iter()
            .map(|app| (app.id, app.app_id))
            .collect::<Vec<_>>(),
        vec![(existing.id, existing.app_id)],
        "the App already connected for that owner is the only one"
    );
}

/// The runtime is current-thread so every task the router spawns logs through
/// the subscriber set as this thread's default.
#[tokio::test(flavor = "current_thread")]
async fn callback_manifest_orphaned_app_is_logged_for_deletion() {
    let (captured, _capturing) = capturing();
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let existing = harness.seed_app(&tenant).await;
    let state = harness.registration_state(&tenant).await;
    let registration = Registration::new(&existing.owner_login, AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;

    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    let logs = captured.text();
    for (label, identity) in [
        ("slug", registration.slug.clone()),
        ("address", registration.html_url()),
    ] {
        assert!(
            logs.contains(&identity),
            "the orphaned App's {label} is logged so an operator can delete it: {logs}"
        );
    }
    for (label, secret) in [
        ("the manifest code", code.as_str()),
        ("the registration state", state.as_str()),
        ("the client secret", registration.client_secret.as_str()),
        ("the webhook secret", registration.webhook_secret.as_str()),
        ("the private key's body", pem_body()),
    ] {
        assert!(!logs.contains(secret), "{label} reached the logs");
    }
}

#[tokio::test]
async fn callback_manifest_enterprise_owner_fails() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let state = harness.registration_state(&tenant).await;
    let registration = Registration::new(&github_app::login("acme"), AccountType::Enterprise);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;

    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    assert!(harness.apps(tenant.organization).await.is_empty());
}

#[tokio::test]
async fn callback_manifest_demoted_user_fails() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let admin = harness.joined(&tenant, OrgRole::Admin).await;
    let state = harness
        .issue(tenant.organization, admin.user, Purpose::Registration, None)
        .await;
    organization_members::update_member_role(
        harness.pool(),
        tenant.organization,
        admin.user,
        OrgRole::Member,
    )
    .await
    .expect("the admin is demoted");
    let code = github_app::opaque();
    let registration = Registration::new(&github_app::login("acme"), AccountType::Organization);
    github_app::converting(&harness.github, &code, registration.answer(), 0).await;

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;

    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    assert!(harness.apps(tenant.organization).await.is_empty());
    harness.assert_github_untouched().await;
}

#[tokio::test]
async fn callback_setup_missing_state_syncs_and_writes_nothing() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.seed_app(&tenant).await;
    let outstanding = harness.installation_state(&tenant, app.id).await;
    let registering = harness.registration_state(&tenant).await;
    let unknown = github_app::opaque();
    let installation_id = github_app::installation_id().to_string();

    for (label, state) in [
        ("no state", None),
        ("an unknown state", Some(unknown.as_str())),
        ("a registration state", Some(registering.as_str())),
    ] {
        let mut parameters = vec![
            (INSTALLATION_PARAMETER, installation_id.as_str()),
            (SETUP_ACTION_PARAMETER, INSTALL),
        ];
        parameters.extend(state.map(|state| (STATE_PARAMETER, state)));
        let response = harness.setup_callback(&parameters).await;
        harness.assert_lands(&response, Outcome::Sync, None);
        assert_uncached(&response, label);
    }

    assert!(harness.installations(tenant.organization).await.is_empty());
    harness.assert_github_untouched().await;
    assert!(
        harness
            .still_consumable(&outstanding, Purpose::Installation)
            .await,
        "the organization's outstanding installation state is untouched"
    );
    assert!(
        harness
            .still_consumable(&registering, Purpose::Registration)
            .await,
        "a state for another purpose is left alone"
    );
}

#[tokio::test]
async fn callback_setup_replayed_state_syncs() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.seed_app(&tenant).await;
    let state = harness.installation_state(&tenant, app.id).await;
    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        app.app_id,
        installation_id,
        Installed::new(installation_id, app.app_id, &app.owner_login).answer(),
        1,
    )
    .await;
    let installation = installation_id.to_string();
    let parameters = [
        (INSTALLATION_PARAMETER, installation.as_str()),
        (SETUP_ACTION_PARAMETER, INSTALL),
        (STATE_PARAMETER, &state),
    ];

    let first = harness.setup_callback(&parameters).await;
    harness.assert_lands(&first, Outcome::Installed, Some(tenant.organization));

    let replayed = harness.setup_callback(&parameters).await;
    harness.assert_lands(&replayed, Outcome::Sync, None);
    let installations = harness.installations(tenant.organization).await;
    assert_eq!(
        installations
            .iter()
            .map(|row| (row.github_app_id, row.installation_id))
            .collect::<Vec<_>>(),
        vec![(app.id, installation_id)],
        "the replay records nothing more"
    );
}

#[tokio::test]
async fn callback_setup_request_is_requested() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.seed_app(&tenant).await;
    let state = harness.installation_state(&tenant, app.id).await;

    let response = harness
        .setup_callback(&[(SETUP_ACTION_PARAMETER, REQUEST), (STATE_PARAMETER, &state)])
        .await;
    harness.assert_lands(&response, Outcome::Requested, Some(tenant.organization));
    assert_uncached(&response, "a request with a state");

    let response = harness
        .setup_callback(&[(SETUP_ACTION_PARAMETER, REQUEST)])
        .await;
    harness.assert_lands(&response, Outcome::Requested, None);
    assert_uncached(&response, "a request without a state");

    assert!(harness.installations(tenant.organization).await.is_empty());
    harness.assert_github_untouched().await;
}

#[tokio::test]
async fn callback_setup_foreign_application_fails() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.seed_app(&tenant).await;
    let state = harness.installation_state(&tenant, app.id).await;
    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        app.app_id,
        installation_id,
        Installed::new(installation_id, app.app_id + 1, &app.owner_login).answer(),
        1,
    )
    .await;

    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &state),
        ])
        .await;

    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    assert!(
        harness.installations(tenant.organization).await.is_empty(),
        "an installation of another App is not recorded"
    );
}

#[tokio::test]
async fn callback_setup_installation_of_another_org_fails() {
    let harness = Harness::new().await;
    let victim = harness.tenant().await;
    let attacker = harness.tenant().await;
    let installation_id = github_app::installation_id();
    let seeded =
        github_app::seed_installation(harness.client.state(), victim.organization, installation_id)
            .await;
    let app = harness.seed_app(&attacker).await;
    let state = harness.installation_state(&attacker, app.id).await;
    github_app::fetching_installation(
        &harness.github,
        app.app_id,
        installation_id,
        Installed::new(installation_id, app.app_id, &app.owner_login).answer(),
        1,
    )
    .await;

    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &state),
        ])
        .await;

    harness.assert_lands(&response, Outcome::Failed, Some(attacker.organization));
    assert!(
        harness
            .installations(attacker.organization)
            .await
            .is_empty()
    );
    let kept = github_installations::get(harness.pool(), victim.organization, seeded.installation)
        .await
        .expect("the victim's installation is readable")
        .expect("the victim's installation is still there");
    assert_eq!(
        (
            kept.github_app_id,
            kept.account_login.as_str(),
            kept.revoked_at,
        ),
        (seeded.app, seeded.account_login.as_str(), None),
        "the victim's installation is untouched"
    );
}

#[tokio::test]
async fn callback_head_is_refused_and_keeps_state() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let state = harness.registration_state(&tenant).await;
    let registration = Registration::new(&github_app::login("acme"), AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;
    let manifest = format!(
        "{MANIFEST_CALLBACK_PATH}?{}",
        query(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
    );

    let refused = harness.client.head(&manifest).await;
    refused.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    assert_uncached(&refused, "a refused HEAD of the manifest callback");
    assert!(harness.apps(tenant.organization).await.is_empty());

    let installation_state = assert_installing(
        &harness.client.get(&manifest).await,
        &registration.html_url(),
    );

    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        registration.app_id,
        installation_id,
        Installed::new(
            installation_id,
            registration.app_id,
            &registration.owner_login,
        )
        .answer(),
        1,
    )
    .await;
    let setup = format!(
        "{SETUP_CALLBACK_PATH}?{}",
        query(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &installation_state),
        ])
    );

    let refused = harness.client.head(&setup).await;
    refused.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    assert_uncached(&refused, "a refused HEAD of the setup callback");
    assert!(harness.installations(tenant.organization).await.is_empty());

    harness.assert_lands(
        &harness.client.get(&setup).await,
        Outcome::Installed,
        Some(tenant.organization),
    );
}

#[tokio::test]
async fn callback_headers_on_every_outcome() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.seed_app(&tenant).await;
    let mut responses = Vec::new();

    let state = harness.registration_state(&tenant).await;
    let registration = Registration::new(&github_app::login("acme"), AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;
    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;
    assert_installing(&response, &registration.html_url());
    responses.push((
        "the manifest callback sending the browser to install",
        response,
    ));

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &github_app::opaque())])
        .await;
    harness.assert_lands(&response, Outcome::Expired, None);
    responses.push(("the manifest callback expiring", response));

    let state = harness.registration_state(&tenant).await;
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, ResponseTemplate::new(422), 1).await;
    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &state)])
        .await;
    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    responses.push(("the manifest callback failing", response));

    let state = harness.registration_state(&tenant).await;
    let response = harness
        .client
        .head(&format!(
            "{MANIFEST_CALLBACK_PATH}?{}",
            query(&[
                (CODE_PARAMETER, &github_app::opaque()),
                (STATE_PARAMETER, &state)
            ])
        ))
        .await;
    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    responses.push(("the manifest callback refusing HEAD", response));

    let state = harness.installation_state(&tenant, app.id).await;
    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        app.app_id,
        installation_id,
        Installed::new(installation_id, app.app_id, &app.owner_login).answer(),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Installed, Some(tenant.organization));
    responses.push(("the setup callback installing", response));

    let response = harness
        .setup_callback(&[(SETUP_ACTION_PARAMETER, REQUEST)])
        .await;
    harness.assert_lands(&response, Outcome::Requested, None);
    responses.push(("the setup callback requesting", response));

    let response = harness
        .setup_callback(&[
            (
                INSTALLATION_PARAMETER,
                &github_app::installation_id().to_string(),
            ),
            (SETUP_ACTION_PARAMETER, INSTALL),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Sync, None);
    responses.push(("the setup callback asking for a sync", response));

    let state = harness.installation_state(&tenant, app.id).await;
    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        app.app_id,
        installation_id,
        ResponseTemplate::new(500),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    responses.push(("the setup callback failing", response));

    let state = harness.installation_state(&tenant, app.id).await;
    let response = harness
        .client
        .head(&format!(
            "{SETUP_CALLBACK_PATH}?{}",
            query(&[
                (
                    INSTALLATION_PARAMETER,
                    &github_app::installation_id().to_string()
                ),
                (SETUP_ACTION_PARAMETER, INSTALL),
                (STATE_PARAMETER, &state),
            ])
        ))
        .await;
    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    responses.push(("the setup callback refusing HEAD", response));

    for (label, response) in &responses {
        assert_uncached(response, label);
        if response.status == StatusCode::METHOD_NOT_ALLOWED {
            assert!(
                response.header(header::LOCATION.as_str()).is_none(),
                "{label} redirects nowhere"
            );
        }
    }
}

#[tokio::test]
async fn security_location_never_carries_secrets() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let owner = github_app::login("acme");
    let registering = harness.register(&tenant, &owner).await;
    let registration = Registration::new(&owner, AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;
    let mut locations = Vec::new();

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &registering)])
        .await;
    let installation_state = assert_installing(&response, &registration.html_url());
    locations.push(location(&response));

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &registering)])
        .await;
    harness.assert_lands(&response, Outcome::Expired, None);
    locations.push(location(&response));

    let refused_state = harness.registration_state(&tenant).await;
    let refused_code = github_app::opaque();
    github_app::converting(
        &harness.github,
        &refused_code,
        ResponseTemplate::new(422).set_body_json(json!({
            "message": format!("code {refused_code} is not valid"),
            "client_secret": registration.client_secret,
        })),
        1,
    )
    .await;
    let response = harness
        .manifest_callback(&[
            (CODE_PARAMETER, &refused_code),
            (STATE_PARAMETER, &refused_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    locations.push(location(&response));

    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        registration.app_id,
        installation_id,
        Installed::new(installation_id, registration.app_id, &owner).answer(),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &installation_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Installed, Some(tenant.organization));
    locations.push(location(&response));

    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &installation_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Sync, None);
    locations.push(location(&response));

    let app = harness
        .apps(tenant.organization)
        .await
        .into_iter()
        .next()
        .expect("the App was registered");
    let requesting_state = harness.installation_state(&tenant, app.id).await;
    let response = harness
        .setup_callback(&[
            (SETUP_ACTION_PARAMETER, REQUEST),
            (STATE_PARAMETER, &requesting_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Requested, Some(tenant.organization));
    locations.push(location(&response));

    let foreign_state = harness.installation_state(&tenant, app.id).await;
    let foreign_installation = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        registration.app_id,
        foreign_installation,
        Installed::new(foreign_installation, registration.app_id + 1, &owner).answer(),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &foreign_installation.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &foreign_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));
    locations.push(location(&response));

    let secrets = [
        ("the manifest code", code.as_str()),
        ("the refused manifest code", refused_code.as_str()),
        ("the client secret", registration.client_secret.as_str()),
        ("the webhook secret", registration.webhook_secret.as_str()),
        ("the private key", PEM_MARKER),
        ("the private key's body", pem_body()),
        ("an installation token", MINTED_TOKEN_PREFIX),
        ("a JWT", JWT_PREFIX),
        ("the registration state", registering.as_str()),
        ("the refused registration state", refused_state.as_str()),
        ("the requesting state", requesting_state.as_str()),
        ("the foreign installation's state", foreign_state.as_str()),
    ];
    let zone = Url::parse(harness.app_base_url())
        .expect("the app base URL parses")
        .origin();
    for url in &locations {
        let rendered = url.as_str();
        for (label, secret) in secrets {
            assert!(
                !rendered.contains(secret),
                "{label} leaked into a Location: {rendered}"
            );
        }
        let names: Vec<String> = url
            .query_pairs()
            .map(|(name, _)| name.into_owned())
            .collect();
        match url.origin() == zone {
            true => assert!(
                names.iter().all(
                    |name| [OUTCOME_PARAMETER, ORGANIZATION_PARAMETER].contains(&name.as_str())
                ),
                "a redirect back to Zone carries only the outcome: {rendered}"
            ),
            false => assert_eq!(
                names,
                vec![STATE_PARAMETER.to_string()],
                "a redirect to GitHub carries only the installation state: {rendered}"
            ),
        }
    }
    assert_eq!(
        locations
            .iter()
            .filter(|url| url.as_str().contains(&installation_state))
            .count(),
        1,
        "the installation state appears only in the redirect to GitHub"
    );
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the capture is readable")).into_owned()
    }
}

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("the capture is writable")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for Captured {
    type Writer = Captured;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

/// Every log line the server writes on this thread while the guard lives.
fn capturing() -> (Captured, DefaultGuard) {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(CAPTURED_TARGETS))
        .with_writer(captured.clone())
        .with_ansi(false)
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

/// Every JWT Zone signed and sent to `github`.
async fn sent_jwts(github: &MockServer) -> Vec<String> {
    github
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|request| request.headers.get(header::AUTHORIZATION))
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.strip_prefix(BEARER_PREFIX))
        .filter(|credential| credential.starts_with(JWT_PREFIX))
        .map(String::from)
        .collect()
}

/// The runtime is current-thread so every task the router spawns logs through
/// the subscriber set as this thread's default.
#[tokio::test(flavor = "current_thread")]
async fn security_logs_never_carry_secrets() {
    let (captured, _capturing) = capturing();

    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let owner = github_app::login("acme");
    let registering = harness.register(&tenant, &owner).await;
    let registration = Registration::new(&owner, AccountType::Organization);
    let code = github_app::opaque();
    github_app::converting(&harness.github, &code, registration.answer(), 1).await;
    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &registering)])
        .await;
    let installation_state = assert_installing(&response, &registration.html_url());

    let installation_id = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        registration.app_id,
        installation_id,
        Installed::new(installation_id, registration.app_id, &owner).answer(),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (INSTALLATION_PARAMETER, &installation_id.to_string()),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &installation_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Installed, Some(tenant.organization));

    let token = format!("{MINTED_TOKEN_PREFIX}{}", github_app::opaque());
    github_app::issuing(
        &harness.github,
        registration.app_id,
        installation_id,
        &token,
        1,
    )
    .await;
    github_app::listing_repositories(
        &harness.github,
        &token,
        &[ListedRepository::new(7, &owner, "widgets")],
        1,
    )
    .await;
    let response = harness
        .client
        .get_auth(
            &format!("{}/repositories?page=1&per_page=50", tenant.github()),
            &tenant.token,
        )
        .await;
    response.assert_status(StatusCode::OK);

    let response = harness
        .manifest_callback(&[(CODE_PARAMETER, &code), (STATE_PARAMETER, &registering)])
        .await;
    harness.assert_lands(&response, Outcome::Expired, None);

    let refused_state = harness.registration_state(&tenant).await;
    let refused_code = github_app::opaque();
    github_app::converting(
        &harness.github,
        &refused_code,
        ResponseTemplate::new(422),
        1,
    )
    .await;
    let response = harness
        .manifest_callback(&[
            (CODE_PARAMETER, &refused_code),
            (STATE_PARAMETER, &refused_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));

    let app = harness
        .apps(tenant.organization)
        .await
        .into_iter()
        .next()
        .expect("the App was registered");
    let unreachable_state = harness.installation_state(&tenant, app.id).await;
    let unreachable_installation = github_app::installation_id();
    github_app::fetching_installation(
        &harness.github,
        registration.app_id,
        unreachable_installation,
        ResponseTemplate::new(500),
        1,
    )
    .await;
    let response = harness
        .setup_callback(&[
            (
                INSTALLATION_PARAMETER,
                &unreachable_installation.to_string(),
            ),
            (SETUP_ACTION_PARAMETER, INSTALL),
            (STATE_PARAMETER, &unreachable_state),
        ])
        .await;
    harness.assert_lands(&response, Outcome::Failed, Some(tenant.organization));

    let logs = captured.text();
    assert!(
        logs.contains(&format!("path={MANIFEST_CALLBACK_PATH}")),
        "the capture saw the manifest callback's request span, or this test proves nothing"
    );
    for query in [format!("{MANIFEST_CALLBACK_PATH}?"), "?code=".to_string()] {
        assert!(
            !logs.contains(&query),
            "a callback's query reached the logs"
        );
    }
    let jwts = sent_jwts(&harness.github).await;
    assert!(
        !jwts.is_empty(),
        "Zone sent GitHub an App JWT, or the JWT check proves nothing"
    );
    for jwt in &jwts {
        assert!(!logs.contains(jwt.as_str()), "an App JWT reached the logs");
    }
    for (label, secret) in [
        ("the manifest code", code.as_str()),
        ("the refused manifest code", refused_code.as_str()),
        ("the client secret", registration.client_secret.as_str()),
        ("the webhook secret", registration.webhook_secret.as_str()),
        ("the private key's body", pem_body()),
        ("the minted installation token", token.as_str()),
        ("the registration state", registering.as_str()),
        ("the installation state", installation_state.as_str()),
        ("the refused registration state", refused_state.as_str()),
        (
            "the unreachable installation's state",
            unreachable_state.as_str(),
        ),
    ] {
        assert!(!logs.contains(secret), "{label} reached the logs");
    }
}
