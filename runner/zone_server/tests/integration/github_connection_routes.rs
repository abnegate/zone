use std::sync::Mutex;

use crate::common::github_app::{self, APP_ID, ApplicationJwt, Seeded, TEST_PRIVATE_KEY};
use crate::common::{self, TestClient, TestResponse, test_email, test_password};
use abnegate_secret::SecretValue;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate, Times};
use zone_server::config::DEFAULT_GITHUB_WEB_URL;
use zone_server::db::github_apps::{self, AppRow, NewApp, OwnerType};
use zone_server::db::github_connection_states::{self, ConsumedState, Purpose};
use zone_server::db::github_installations::{
    self, AccountType, InstallationRow, InstallationUpsert, RepositorySelection,
};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::services::github_app::manifest::{MANIFEST_CALLBACK_PATH, SETUP_CALLBACK_PATH};
use zone_server::state::AppState;

const JSON: &str = "application/json";
const MOCK_AUTHORIZATION: &str = "authorization";
const NO_WORKSPACE: Uuid = Uuid::nil();
const BRANCH_PAGE_SIZE: usize = 100;
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// Unlike a bare `Option` field, one read through this must be present, so a
/// response that leaves the field out fails rather than reading as null.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum AppOwner {
    User,
    Organization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum Account {
    User,
    Organization,
    Enterprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Selection {
    All,
    Selected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Unlisted {
    Revoked,
    Suspended,
    Unavailable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct App {
    id: Uuid,
    app_id: i64,
    slug: String,
    name: String,
    owner_login: String,
    owner_type: AppOwner,
    html_url: String,
    settings_url: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Installation {
    id: Uuid,
    github_app_id: Uuid,
    installation_id: i64,
    account_login: String,
    account_type: Account,
    repository_selection: Selection,
    html_url: String,
    #[serde(deserialize_with = "present")]
    suspended_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "present")]
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    apps: Vec<App>,
    installations: Vec<Installation>,
}

impl Connection {
    fn app(&self, id: Uuid) -> &App {
        self.apps
            .iter()
            .find(|app| app.id == id)
            .unwrap_or_else(|| panic!("App {id} is listed: {:?}", self.apps))
    }

    fn installation(&self, id: Uuid) -> &Installation {
        self.installations
            .iter()
            .find(|installation| installation.id == id)
            .unwrap_or_else(|| panic!("installation {id} is listed: {:?}", self.installations))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    action: String,
    manifest: String,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallationUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    installation_id: Uuid,
    id: u64,
    owner: String,
    name: String,
    full_name: String,
    private: bool,
    archived: bool,
    default_branch: String,
    html_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogInstallation {
    id: Uuid,
    account_login: String,
    html_url: String,
    #[serde(deserialize_with = "present")]
    error: Option<Unlisted>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    repositories: Vec<Repository>,
    installations: Vec<CatalogInstallation>,
    total: u64,
    #[serde(deserialize_with = "present")]
    next_page: Option<u32>,
}

impl Catalog {
    fn full_names(&self) -> Vec<&str> {
        self.repositories
            .iter()
            .map(|repository| repository.full_name.as_str())
            .collect()
    }

    fn installation(&self, id: Uuid) -> &CatalogInstallation {
        self.installations
            .iter()
            .find(|installation| installation.id == id)
            .unwrap_or_else(|| panic!("installation {id} is listed: {:?}", self.installations))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Branch {
    name: String,
    protected: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Branches {
    branches: Vec<Branch>,
    #[serde(deserialize_with = "present")]
    next_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: String,
}

fn parsed<T: DeserializeOwned>(response: &TestResponse, status: StatusCode) -> T {
    response.assert_status(status);
    serde_json::from_slice(response.bytes()).unwrap_or_else(|error| {
        panic!(
            "the body matches the contract ({error}): {}",
            response.text()
        )
    })
}

fn assert_refused(response: &TestResponse, status: StatusCode) {
    let body: ErrorBody = parsed(response, status);
    assert!(
        !body.error.is_empty(),
        "the refusal says why: {}",
        response.text()
    );
}

fn application_id() -> i64 {
    rand::random_range(1..1_000_000_000)
}

fn minted_token(installation_id: i64) -> String {
    format!("ghs_routes_{installation_id}")
}

/// GitHub mints [`minted_token`] for App `app_id`'s installation `installation_id`.
async fn routes_minting(
    server: &MockServer,
    app_id: i64,
    installation_id: i64,
    times: impl Into<Times>,
) {
    let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
    Mock::given(method("POST"))
        .and(path(format!(
            "/app/installations/{installation_id}/access_tokens"
        )))
        .and(ApplicationJwt(app_id))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token": minted_token(installation_id),
            "expires_at": expires_at,
        })))
        .expect(times)
        .mount(server)
        .await;
}

async fn routes_refusing_mint(server: &MockServer, app_id: i64, installation_id: i64, status: u16) {
    Mock::given(method("POST"))
        .and(path(format!(
            "/app/installations/{installation_id}/access_tokens"
        )))
        .and(ApplicationJwt(app_id))
        .respond_with(ResponseTemplate::new(status))
        .expect(1)
        .mount(server)
        .await;
}

/// GitHub answers a listing of `installation_id`'s repositories, made with its
/// minted token, with `answer`.
async fn routes_listing_repositories(
    server: &MockServer,
    installation_id: i64,
    answer: ResponseTemplate,
    times: impl Into<Times>,
) {
    Mock::given(method("GET"))
        .and(path("/installation/repositories"))
        .and(header(
            MOCK_AUTHORIZATION,
            format!("Bearer {}", minted_token(installation_id)),
        ))
        .respond_with(answer)
        .expect(times)
        .mount(server)
        .await;
}

fn routes_repositories_page(rows: &[Value]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "total_count": rows.len(),
        "repositories": rows,
    }))
}

fn routes_listed_repository(id: u64, owner: &str, name: &str) -> Value {
    routes_listed_repository_at(
        id,
        owner,
        name,
        &format!("https://github.com/{owner}/{name}"),
    )
}

fn routes_listed_repository_at(id: u64, owner: &str, name: &str, html_url: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "full_name": format!("{owner}/{name}"),
        "owner": { "login": owner },
        "private": false,
        "archived": false,
        "default_branch": "main",
        "html_url": html_url,
    })
}

/// GitHub answers the App `app_id`'s listing of its installations with `answer`.
async fn routes_listing_app_installations(
    server: &MockServer,
    app_id: i64,
    answer: ResponseTemplate,
    times: impl Into<Times>,
) {
    Mock::given(method("GET"))
        .and(path("/app/installations"))
        .and(ApplicationJwt(app_id))
        .respond_with(answer)
        .expect(times)
        .mount(server)
        .await;
}

fn routes_listed_installation(installation_id: i64, app_id: i64, account: &str) -> Value {
    routes_listed_installation_at(
        installation_id,
        app_id,
        account,
        &format!(
            "https://github.com/organizations/{account}/settings/installations/{installation_id}"
        ),
    )
}

fn routes_listed_installation_at(
    installation_id: i64,
    app_id: i64,
    account: &str,
    html_url: &str,
) -> Value {
    json!({
        "id": installation_id,
        "app_id": app_id,
        "account": { "login": account, "type": "Organization" },
        "target_type": "Organization",
        "repository_selection": "selected",
        "html_url": html_url,
        "suspended_at": null,
    })
}

/// GitHub answers App `app_id`'s uninstall of `installation_id` with `status`.
async fn routes_deleting_installation(
    server: &MockServer,
    app_id: i64,
    installation_id: i64,
    status: u16,
    times: impl Into<Times>,
) {
    Mock::given(method("DELETE"))
        .and(path(format!("/app/installations/{installation_id}")))
        .and(ApplicationJwt(app_id))
        .respond_with(ResponseTemplate::new(status))
        .expect(times)
        .mount(server)
        .await;
}

/// GitHub answers page `page` of `acme/widgets`'s branches, asked for a
/// hundred at a time with `installation_id`'s minted token, with `answer`.
async fn routes_listing_branches(
    server: &MockServer,
    installation_id: i64,
    page: u32,
    answer: ResponseTemplate,
) {
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/branches"))
        .and(query_param("page", page.to_string()))
        .and(query_param("per_page", BRANCH_PAGE_SIZE.to_string()))
        .and(header(
            MOCK_AUTHORIZATION,
            format!("Bearer {}", minted_token(installation_id)),
        ))
        .respond_with(answer)
        .expect(1)
        .mount(server)
        .await;
}

fn routes_listed_branch(name: &str, protected: bool) -> Value {
    json!({ "name": name, "commit": { "sha": COMMIT }, "protected": protected })
}

struct Person {
    token: String,
    user: Uuid,
}

struct Tenant {
    owner: Person,
    organization: Uuid,
}

struct AppSpecification {
    app_id: i64,
    owner_login: String,
    owner_type: OwnerType,
    created_by: Option<Uuid>,
}

/// One client whose GitHub, installation tokens included, is a mock server
/// that answers only what a test mounts.
struct Harness {
    client: TestClient,
    github: MockServer,
    people: Mutex<Vec<Uuid>>,
}

impl Harness {
    async fn new() -> Self {
        let github = MockServer::start().await;
        let client = TestClient::with_config(github_app::config_for(&github)).await;
        Self {
            client,
            github,
            people: Mutex::default(),
        }
    }

    fn state(&self) -> &AppState {
        self.client.state()
    }

    fn pool(&self) -> &PgPool {
        self.state().db()
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
        self.people
            .lock()
            .expect("the people list is not poisoned")
            .push(user);
        Person { token, user }
    }

    /// A person and the organization they created, which they own.
    async fn tenant(&self) -> Tenant {
        let owner = self.person().await;
        let organization = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({
                    "name": "GitHub Routes",
                    "slug": format!("github-routes-{}", Uuid::new_v4().simple()),
                }),
                &owner.token,
            )
            .await
            .json_value()["organization"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("an organization");
        Tenant {
            owner,
            organization,
        }
    }

    async fn joined(&self, tenant: &Tenant, role: OrgRole) -> Person {
        let person = self.person().await;
        organization_members::add_member(self.pool(), tenant.organization, person.user, role, None)
            .await
            .expect("the person joins the organization");
        person
    }

    async fn member(&self, tenant: &Tenant) -> Person {
        self.joined(tenant, OrgRole::Member).await
    }

    async fn installation(&self, tenant: &Tenant) -> Seeded {
        github_app::seed_installation(
            self.state(),
            tenant.organization,
            github_app::installation_id(),
        )
        .await
    }

    async fn revoked_installation(&self, tenant: &Tenant) -> Seeded {
        let seeded = self.installation(tenant).await;
        assert!(
            github_installations::mark_revoked(self.pool(), seeded.installation)
                .await
                .expect("the installation is marked revoked"),
            "the seeded installation exists"
        );
        seeded
    }

    async fn suspended_installation(&self, tenant: &Tenant) -> Seeded {
        let seeded = self.installation(tenant).await;
        assert!(
            github_installations::mark_suspended(self.pool(), seeded.installation)
                .await
                .expect("the installation is marked suspended"),
            "the seeded installation exists"
        );
        seeded
    }

    fn sealed(&self, secret: &str) -> String {
        self.state()
            .github()
            .seal(&SecretValue::new(secret))
            .expect("a test secret seals")
    }

    async fn app(&self, tenant: &Tenant, specification: AppSpecification) -> AppRow {
        let slug = specification.owner_login.to_lowercase();
        github_apps::insert(
            self.pool(),
            &NewApp {
                organization_id: tenant.organization,
                app_id: specification.app_id,
                slug: &slug,
                name: &slug,
                owner_login: &specification.owner_login,
                owner_type: specification.owner_type,
                html_url: &format!("https://github.com/apps/{slug}"),
                client_id: "Iv1.throwaway",
                client_secret: &self.sealed("throwaway-client-secret"),
                private_key: &self.sealed(TEST_PRIVATE_KEY),
                webhook_secret: None,
                created_by: specification.created_by,
            },
        )
        .await
        .expect("the organization registers the App")
    }

    async fn organization_app(&self, tenant: &Tenant) -> AppRow {
        self.app(
            tenant,
            AppSpecification {
                app_id: application_id(),
                owner_login: github_app::login("acme"),
                owner_type: OwnerType::Organization,
                created_by: Some(tenant.owner.user),
            },
        )
        .await
    }

    async fn personal_app(&self, tenant: &Tenant, created_by: Uuid) -> AppRow {
        self.app(
            tenant,
            AppSpecification {
                app_id: application_id(),
                owner_login: github_app::login("octo"),
                owner_type: OwnerType::User,
                created_by: Some(created_by),
            },
        )
        .await
    }

    async fn installation_of(&self, app: &AppRow) -> InstallationRow {
        let installation_id = github_app::installation_id();
        github_installations::upsert(
            self.pool(),
            &InstallationUpsert {
                organization_id: app.organization_id,
                github_app_id: app.id,
                installation_id,
                account_login: &app.owner_login,
                account_type: AccountType::Organization,
                repository_selection: RepositorySelection::All,
                html_url: &format!(
                    "https://github.com/organizations/{}/settings/installations/{installation_id}",
                    app.owner_login
                ),
                suspended_at: None,
                created_by: None,
            },
        )
        .await
        .expect("the App is installed on the account")
    }

    async fn stored_app(&self, organization: Uuid, id: Uuid) -> Option<AppRow> {
        github_apps::get(self.pool(), organization, id)
            .await
            .expect("the App is readable")
    }

    async fn stored_installation(&self, organization: Uuid, id: Uuid) -> Option<InstallationRow> {
        github_installations::get(self.pool(), organization, id)
            .await
            .expect("the installation is readable")
    }

    async fn stored_installations(&self, organization: Uuid) -> Vec<InstallationRow> {
        github_installations::list(self.pool(), organization)
            .await
            .expect("the installations are readable")
    }

    async fn active(&self, organization: Uuid, id: Uuid) -> bool {
        self.stored_installation(organization, id)
            .await
            .is_some_and(|installation| installation.revoked_at.is_none())
    }

    async fn consumed(&self, state: &str, purpose: Purpose) -> Option<ConsumedState> {
        github_connection_states::consume(self.pool(), state, purpose)
            .await
            .expect("the state is readable")
    }

    async fn github_requests(&self) -> usize {
        self.github
            .received_requests()
            .await
            .unwrap_or_default()
            .len()
    }

    /// Every request carries `Content-Type: application/json`, as the frontend
    /// sends it even on a request with no body.
    async fn call(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Body,
    ) -> TestResponse {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(CONTENT_TYPE, JSON);
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        self.client
            .send_request(request.body(body).expect("a well-formed request"))
            .await
    }

    async fn get(&self, uri: &str, token: Option<&str>) -> TestResponse {
        self.call(Method::GET, uri, token, Body::empty()).await
    }

    async fn post(&self, uri: &str, token: Option<&str>) -> TestResponse {
        self.call(Method::POST, uri, token, Body::empty()).await
    }

    async fn post_json(&self, uri: &str, token: Option<&str>, body: &Value) -> TestResponse {
        self.call(Method::POST, uri, token, Body::from(body.to_string()))
            .await
    }

    async fn delete(&self, uri: &str, token: Option<&str>) -> TestResponse {
        self.call(Method::DELETE, uri, token, Body::empty()).await
    }

    async fn connection(&self, tenant: &Tenant, token: &str) -> Connection {
        parsed(
            &self.get(&connection_path(tenant), Some(token)).await,
            StatusCode::OK,
        )
    }

    async fn register(&self, tenant: &Tenant, body: &Value) -> TestResponse {
        self.post_json(&apps_path(tenant), Some(&tenant.owner.token), body)
            .await
    }

    async fn catalog(&self, tenant: &Tenant, query: &str) -> Catalog {
        parsed(
            &self
                .get(&repositories_path(tenant, query), Some(&tenant.owner.token))
                .await,
            StatusCode::OK,
        )
    }

    async fn branches(&self, tenant: &Tenant, installation: Uuid, query: &str) -> TestResponse {
        self.get(
            &branches_path(tenant, installation, query),
            Some(&tenant.owner.token),
        )
        .await
    }

    async fn discard(&self) {
        let people = self
            .people
            .lock()
            .expect("the people list is not poisoned")
            .clone();
        common::discard(self.pool(), NO_WORKSPACE, &people).await;
    }
}

fn connection_path(tenant: &Tenant) -> String {
    format!("/api/organizations/{}/github", tenant.organization)
}

fn apps_path(tenant: &Tenant) -> String {
    format!("{}/apps", connection_path(tenant))
}

fn app_path(tenant: &Tenant, app: Uuid) -> String {
    format!("{}/{app}", apps_path(tenant))
}

fn app_installations_path(tenant: &Tenant, app: Uuid) -> String {
    format!("{}/installations", app_path(tenant, app))
}

fn sync_path(tenant: &Tenant) -> String {
    format!("{}/installations/sync", connection_path(tenant))
}

fn installation_path(tenant: &Tenant, installation: Uuid, query: &str) -> String {
    format!(
        "{}/installations/{installation}{query}",
        connection_path(tenant)
    )
}

fn repositories_path(tenant: &Tenant, query: &str) -> String {
    format!("{}/repositories{query}", connection_path(tenant))
}

fn branches_path(tenant: &Tenant, installation: Uuid, query: &str) -> String {
    format!(
        "{}/installations/{installation}/repositories/acme/widgets/branches{query}",
        connection_path(tenant)
    )
}

fn state_of(url: &str) -> String {
    let (_, state) = url
        .split_once("state=")
        .unwrap_or_else(|| panic!("{url} carries a state"));
    urlencoding::decode(state)
        .expect("the state is percent-encoded UTF-8")
        .into_owned()
}

fn assert_each(responses: Vec<(&str, TestResponse)>, expected: StatusCode, caller: &str) {
    for (route, response) in responses {
        assert_eq!(
            response.status,
            expected,
            "{route} from {caller}: {}",
            response.text()
        );
    }
}

#[tokio::test]
async fn authorization_connection_without_token_is_unauthorized() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;

    assert_each(
        vec![
            ("#1", harness.get(&connection_path(&tenant), None).await),
            (
                "#2",
                harness
                    .post_json(&apps_path(&tenant), None, &json!({ "owner": null }))
                    .await,
            ),
            (
                "#8",
                harness.delete(&app_path(&tenant, seeded.app), None).await,
            ),
        ],
        StatusCode::UNAUTHORIZED,
        "a caller without a token",
    );
    assert!(
        harness
            .stored_app(tenant.organization, seeded.app)
            .await
            .is_some(),
        "the App is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_connection_another_organizations_admin_is_forbidden() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let token = Some(stranger.owner.token.as_str());

    assert_each(
        vec![
            ("#1", harness.get(&connection_path(&tenant), token).await),
            (
                "#2",
                harness
                    .post_json(&apps_path(&tenant), token, &json!({ "owner": null }))
                    .await,
            ),
            (
                "#8",
                harness.delete(&app_path(&tenant, seeded.app), token).await,
            ),
        ],
        StatusCode::FORBIDDEN,
        "another organization's admin",
    );
    assert!(
        harness
            .stored_app(tenant.organization, seeded.app)
            .await
            .is_some(),
        "the App is kept"
    );
    assert!(
        harness
            .active(tenant.organization, seeded.installation)
            .await,
        "the installation is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_connection_member_reads_but_cannot_change() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let member = harness.member(&tenant).await;
    let seeded = harness.installation(&tenant).await;
    let token = Some(member.token.as_str());

    let connection = harness.connection(&tenant, &member.token).await;
    assert_eq!(connection.app(seeded.app).id, seeded.app);
    assert_eq!(
        connection.installation(seeded.installation).id,
        seeded.installation
    );

    assert_each(
        vec![
            (
                "#2",
                harness
                    .post_json(&apps_path(&tenant), token, &json!({ "owner": "bad login" }))
                    .await,
            ),
            (
                "#8",
                harness.delete(&app_path(&tenant, seeded.app), token).await,
            ),
        ],
        StatusCode::FORBIDDEN,
        "a member, checked before the body",
    );
    assert!(
        harness
            .stored_app(tenant.organization, seeded.app)
            .await
            .is_some(),
        "the App is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_connection_foreign_app_is_not_found_and_kept() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let foreign = harness.installation(&stranger).await;

    harness
        .delete(&app_path(&tenant, foreign.app), Some(&tenant.owner.token))
        .await
        .assert_status(StatusCode::NOT_FOUND);

    assert!(
        harness
            .stored_app(stranger.organization, foreign.app)
            .await
            .is_some(),
        "the other organization's App is kept"
    );
    assert!(
        harness
            .active(stranger.organization, foreign.installation)
            .await,
        "the other organization's installation is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_lists_every_app_and_installation_in_the_contract_shape() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let organization = harness.installation(&tenant).await;
    let personal = harness.personal_app(&tenant, tenant.owner.user).await;
    let revoked = harness.installation_of(&personal).await;
    github_installations::mark_revoked(harness.pool(), revoked.id)
        .await
        .expect("the installation is marked revoked");
    let now = Utc::now();

    let connection = harness.connection(&tenant, &tenant.owner.token).await;

    assert_eq!(connection.apps.len(), 2, "both Apps are listed");
    let organization_app = connection.app(organization.app);
    assert_eq!(organization_app.app_id, github_app::APP_ID);
    assert_eq!(organization_app.slug, organization.account_login);
    assert_eq!(organization_app.name, organization.account_login);
    assert_eq!(organization_app.owner_login, organization.account_login);
    assert_eq!(organization_app.owner_type, AppOwner::Organization);
    assert_eq!(
        organization_app.html_url,
        format!("https://github.com/apps/{}", organization.account_login)
    );
    assert_eq!(
        organization_app.settings_url,
        format!(
            "{DEFAULT_GITHUB_WEB_URL}/organizations/{}/settings/apps/{}",
            organization.account_login, organization.account_login
        ),
        "an organization's App is configured under the organization"
    );
    assert!(organization_app.created_at <= now);

    let personal_app = connection.app(personal.id);
    assert_eq!(personal_app.app_id, personal.app_id);
    assert_eq!(personal_app.owner_type, AppOwner::User);
    assert_eq!(
        personal_app.settings_url,
        format!("{DEFAULT_GITHUB_WEB_URL}/settings/apps/{}", personal.slug),
        "a personal App is configured under the user's own settings"
    );

    assert_eq!(
        connection.installations.len(),
        2,
        "the revoked installation is listed too"
    );
    let active = connection.installation(organization.installation);
    assert_eq!(active.github_app_id, organization.app);
    assert_eq!(active.installation_id, organization.installation_id);
    assert_eq!(active.account_login, organization.account_login);
    assert_eq!(active.account_type, Account::Organization);
    assert_eq!(active.repository_selection, Selection::All);
    assert_eq!(
        active.html_url,
        format!(
            "https://github.com/organizations/{}/settings/installations/{}",
            organization.account_login, organization.installation_id
        )
    );
    assert_eq!(active.suspended_at, None);
    assert_eq!(active.revoked_at, None);
    assert!(active.created_at <= now);

    let gone = connection.installation(revoked.id);
    assert_eq!(gone.github_app_id, personal.id);
    assert!(gone.revoked_at.is_some(), "it reads as revoked");

    harness.discard().await;
}

#[tokio::test]
async fn connection_settings_url_of_an_unparseable_organization_login_uses_the_user_path() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness
        .app(
            &tenant,
            AppSpecification {
                app_id: application_id(),
                owner_login: "not a login".to_string(),
                owner_type: OwnerType::Organization,
                created_by: None,
            },
        )
        .await;

    let connection = harness.connection(&tenant, &tenant.owner.token).await;

    assert_eq!(
        connection.app(app.id).settings_url,
        format!(
            "{DEFAULT_GITHUB_WEB_URL}/settings/apps/{}",
            urlencoding::encode(&app.slug)
        ),
        "the App is listed, not skipped"
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_without_apps_is_empty() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    let connection = harness.connection(&tenant, &tenant.owner.token).await;

    assert!(connection.apps.is_empty());
    assert!(connection.installations.is_empty());

    harness.discard().await;
}

fn assert_manifest(manifest: &str, app_base_url: &str) {
    let manifest: Value = serde_json::from_str(manifest).expect("the manifest is JSON");
    let base = app_base_url.trim().trim_end_matches('/');
    assert_eq!(manifest["public"], json!(false), "the App is private");
    assert!(
        manifest.get("hook_attributes").is_none(),
        "the App has no webhook: {manifest}"
    );
    assert!(
        manifest.get("default_events").is_none(),
        "the App subscribes to no events: {manifest}"
    );
    assert_eq!(manifest["url"], json!(base));
    assert_eq!(
        manifest["redirect_url"],
        json!(format!("{base}{MANIFEST_CALLBACK_PATH}"))
    );
    assert_eq!(
        manifest["setup_url"],
        json!(format!("{base}{SETUP_CALLBACK_PATH}"))
    );
    assert!(
        manifest["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("Zone ")),
        "the App is named for Zone: {manifest}"
    );
}

#[tokio::test]
async fn connection_register_personal_app_issues_a_registration_state() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    let registration: Registration = parsed(
        &harness.register(&tenant, &json!({ "owner": null })).await,
        StatusCode::OK,
    );

    assert!(!registration.state.is_empty());
    assert_eq!(
        registration.action,
        format!(
            "{DEFAULT_GITHUB_WEB_URL}/settings/apps/new?state={}",
            urlencoding::encode(&registration.state)
        )
    );
    assert_manifest(
        &registration.manifest,
        &harness.state().config().app_base_url,
    );
    assert_eq!(
        harness
            .consumed(&registration.state, Purpose::Registration)
            .await,
        Some(ConsumedState {
            organization_id: tenant.organization,
            user_id: tenant.owner.user,
            github_app_id: None,
        }),
        "the state is a registration state for this organization and admin"
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_without_owner_field_is_personal() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    let registration: Registration =
        parsed(&harness.register(&tenant, &json!({})).await, StatusCode::OK);

    assert_eq!(
        registration.action,
        format!(
            "{DEFAULT_GITHUB_WEB_URL}/settings/apps/new?state={}",
            urlencoding::encode(&registration.state)
        )
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_organization_app_targets_the_organization() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let owner = github_app::login("Acme-Corp");

    let registration: Registration = parsed(
        &harness.register(&tenant, &json!({ "owner": owner })).await,
        StatusCode::OK,
    );

    assert_eq!(
        registration.action,
        format!(
            "{DEFAULT_GITHUB_WEB_URL}/organizations/{owner}/settings/apps/new?state={}",
            urlencoding::encode(&registration.state)
        )
    );
    assert_manifest(
        &registration.manifest,
        &harness.state().config().app_base_url,
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_refuses_an_invalid_owner() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for owner in ["bad login", "", "-acme", "acme--corp"] {
        assert_refused(
            &harness.register(&tenant, &json!({ "owner": owner })).await,
            StatusCode::BAD_REQUEST,
        );
    }
    harness
        .call(
            Method::POST,
            &apps_path(&tenant),
            Some(&tenant.owner.token),
            Body::from("{"),
        )
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_refuses_an_owner_already_connected_whatever_its_case() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;

    for owner in [
        seeded.account_login.clone(),
        seeded.account_login.to_uppercase(),
    ] {
        assert_refused(
            &harness.register(&tenant, &json!({ "owner": owner })).await,
            StatusCode::CONFLICT,
        );
    }

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_refuses_a_second_personal_app_for_the_same_admin() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    harness.personal_app(&tenant, tenant.owner.user).await;

    assert_refused(
        &harness.register(&tenant, &json!({ "owner": null })).await,
        StatusCode::CONFLICT,
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_allows_a_personal_app_beside_another_admins() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let admin = harness.joined(&tenant, OrgRole::Admin).await;
    harness.personal_app(&tenant, admin.user).await;

    let _: Registration = parsed(
        &harness.register(&tenant, &json!({ "owner": null })).await,
        StatusCode::OK,
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_register_allows_an_owner_another_organization_connected() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let foreign = harness.installation(&stranger).await;

    let _: Registration = parsed(
        &harness
            .register(&tenant, &json!({ "owner": foreign.account_login }))
            .await,
        StatusCode::OK,
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_delete_unknown_app_is_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    harness
        .delete(
            &app_path(&tenant, Uuid::new_v4()),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    harness.discard().await;
}

#[tokio::test]
async fn connection_delete_malformed_app_id_is_a_json_not_found_after_the_guard() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let member = harness.member(&tenant).await;
    let seeded = harness.installation(&tenant).await;
    let malformed = format!("{}/not-a-uuid", apps_path(&tenant));

    assert_refused(
        &harness.delete(&malformed, Some(&member.token)).await,
        StatusCode::FORBIDDEN,
    );
    assert_refused(
        &harness.delete(&malformed, Some(&tenant.owner.token)).await,
        StatusCode::NOT_FOUND,
    );
    assert!(
        harness
            .stored_app(tenant.organization, seeded.app)
            .await
            .is_some(),
        "the App is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn connection_delete_app_cascades_without_asking_github() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let kept = harness.installation(&tenant).await;

    harness
        .delete(&app_path(&tenant, seeded.app), Some(&tenant.owner.token))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    assert!(
        harness
            .stored_app(tenant.organization, seeded.app)
            .await
            .is_none(),
        "the App is gone"
    );
    assert!(
        harness
            .stored_installation(tenant.organization, seeded.installation)
            .await
            .is_none(),
        "its installation went with it"
    );
    assert!(
        harness.active(tenant.organization, kept.installation).await,
        "another App's installation is kept"
    );
    assert_eq!(
        harness.github_requests().await,
        0,
        "the admin deletes the App on GitHub, not Zone"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_installations_without_token_is_unauthorized() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 204, 0).await;

    assert_each(
        vec![
            (
                "#5",
                harness
                    .post(&app_installations_path(&tenant, seeded.app), None)
                    .await,
            ),
            ("#6", harness.post(&sync_path(&tenant), None).await),
            (
                "#7",
                harness
                    .delete(
                        &installation_path(&tenant, seeded.installation, "?uninstall=true"),
                        None,
                    )
                    .await,
            ),
        ],
        StatusCode::UNAUTHORIZED,
        "a caller without a token",
    );
    assert!(
        harness
            .active(tenant.organization, seeded.installation)
            .await,
        "the installation is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_installations_another_organizations_admin_is_forbidden() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let token = Some(stranger.owner.token.as_str());
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 204, 0).await;

    assert_each(
        vec![
            (
                "#5",
                harness
                    .post(&app_installations_path(&tenant, seeded.app), token)
                    .await,
            ),
            ("#6", harness.post(&sync_path(&tenant), token).await),
            (
                "#7",
                harness
                    .delete(
                        &installation_path(&tenant, seeded.installation, "?uninstall=true"),
                        token,
                    )
                    .await,
            ),
        ],
        StatusCode::FORBIDDEN,
        "another organization's admin",
    );
    assert!(
        harness
            .active(tenant.organization, seeded.installation)
            .await,
        "the installation is kept as it was"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_installations_member_is_forbidden() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let member = harness.member(&tenant).await;
    let seeded = harness.installation(&tenant).await;
    let token = Some(member.token.as_str());
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 204, 0).await;

    assert_each(
        vec![
            (
                "#5",
                harness
                    .post(&app_installations_path(&tenant, seeded.app), token)
                    .await,
            ),
            ("#6", harness.post(&sync_path(&tenant), token).await),
            (
                "#7",
                harness
                    .delete(
                        &installation_path(&tenant, seeded.installation, "?uninstall=maybe"),
                        token,
                    )
                    .await,
            ),
        ],
        StatusCode::FORBIDDEN,
        "a member, checked before the query",
    );
    assert!(
        harness
            .active(tenant.organization, seeded.installation)
            .await,
        "the installation is kept as it was"
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_installations_foreign_app_or_installation_is_not_found_and_kept() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let foreign = harness.installation(&stranger).await;
    routes_deleting_installation(&harness.github, APP_ID, foreign.installation_id, 204, 0).await;
    let token = Some(tenant.owner.token.as_str());

    harness
        .post(&app_installations_path(&tenant, foreign.app), token)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    harness
        .delete(
            &installation_path(&tenant, foreign.installation, "?uninstall=true"),
            token,
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    assert!(
        harness
            .active(stranger.organization, foreign.installation)
            .await,
        "the other organization's installation is kept as it was"
    );
    assert!(
        harness
            .stored_app(stranger.organization, foreign.app)
            .await
            .is_some(),
        "the other organization's App is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_create_without_body_returns_the_install_url() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let app = harness
        .stored_app(tenant.organization, seeded.app)
        .await
        .expect("the App exists");

    let created: InstallationUrl = parsed(
        &harness
            .post(
                &app_installations_path(&tenant, seeded.app),
                Some(&tenant.owner.token),
            )
            .await,
        StatusCode::OK,
    );

    assert!(
        created
            .url
            .starts_with(&format!("{}/installations/new?state=", app.html_url)),
        "{}",
        created.url
    );
    assert_eq!(
        harness
            .consumed(&state_of(&created.url), Purpose::Installation)
            .await,
        Some(ConsumedState {
            organization_id: tenant.organization,
            user_id: tenant.owner.user,
            github_app_id: Some(seeded.app),
        }),
        "the state is an installation state for this App"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_create_for_unknown_app_is_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    harness
        .post(
            &app_installations_path(&tenant, Uuid::new_v4()),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    harness.discard().await;
}

#[tokio::test]
async fn installations_sync_without_apps_is_no_content() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    harness
        .post(&sync_path(&tenant), Some(&tenant.owner.token))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(harness.github_requests().await, 0, "nothing was asked");

    harness.discard().await;
}

fn find_installation(stored: &[InstallationRow], installation_id: i64) -> &InstallationRow {
    stored
        .iter()
        .find(|row| row.installation_id == installation_id)
        .unwrap_or_else(|| panic!("installation {installation_id} is stored: {stored:?}"))
}

#[tokio::test]
async fn installations_sync_revokes_missing_revives_and_records_listed() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.organization_app(&tenant).await;
    let missing = harness.installation_of(&app).await;
    let returned = harness.installation_of(&app).await;
    github_installations::mark_revoked(harness.pool(), returned.id)
        .await
        .expect("the installation is marked revoked");
    let fresh = github_app::installation_id();
    let fresh_account = github_app::login("fresh");
    routes_listing_app_installations(
        &harness.github,
        app.app_id,
        ResponseTemplate::new(200).set_body_json(json!([
            routes_listed_installation(returned.installation_id, app.app_id, &app.owner_login),
            routes_listed_installation(fresh, app.app_id, &fresh_account),
        ])),
        1,
    )
    .await;

    harness
        .post(&sync_path(&tenant), Some(&tenant.owner.token))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let stored = harness.stored_installations(tenant.organization).await;
    assert!(
        find_installation(&stored, missing.installation_id)
            .revoked_at
            .is_some(),
        "an installation GitHub no longer lists is revoked"
    );
    assert!(
        find_installation(&stored, returned.installation_id)
            .revoked_at
            .is_none(),
        "a revoked installation GitHub lists again is revived"
    );
    let recorded = find_installation(&stored, fresh);
    assert_eq!(recorded.github_app_id, app.id);
    assert_eq!(recorded.account_login, fresh_account);
    assert_eq!(recorded.account_type, AccountType::Organization);
    assert_eq!(recorded.repository_selection, RepositorySelection::Selected);
    assert!(recorded.revoked_at.is_none());

    harness.discard().await;
}

#[tokio::test]
async fn installations_sync_keeps_a_listed_installation_it_cannot_record() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.organization_app(&tenant).await;
    let unrecordable = harness.installation_of(&app).await;
    routes_listing_app_installations(
        &harness.github,
        app.app_id,
        ResponseTemplate::new(200).set_body_json(json!([routes_listed_installation_at(
            unrecordable.installation_id,
            app.app_id,
            &app.owner_login,
            "settings/installations/relative",
        )])),
        1,
    )
    .await;

    harness
        .post(&sync_path(&tenant), Some(&tenant.owner.token))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let kept = harness
        .stored_installation(tenant.organization, unrecordable.id)
        .await
        .expect("the installation is kept");
    assert!(
        kept.revoked_at.is_none(),
        "GitHub still lists it, so it is not revoked"
    );
    assert_eq!(
        kept.html_url, unrecordable.html_url,
        "the refused listing is not written"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_sync_failure_of_one_app_is_bad_gateway_after_syncing_the_others() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let failing = harness.organization_app(&tenant).await;
    let healthy = harness.organization_app(&tenant).await;
    let untouched = harness.installation_of(&failing).await;
    let missing = harness.installation_of(&healthy).await;
    let fresh = github_app::installation_id();
    routes_listing_app_installations(
        &harness.github,
        failing.app_id,
        ResponseTemplate::new(500),
        1..,
    )
    .await;
    routes_listing_app_installations(
        &harness.github,
        healthy.app_id,
        ResponseTemplate::new(200).set_body_json(json!([routes_listed_installation(
            fresh,
            healthy.app_id,
            &healthy.owner_login,
        )])),
        1,
    )
    .await;

    assert_refused(
        &harness
            .post(&sync_path(&tenant), Some(&tenant.owner.token))
            .await,
        StatusCode::BAD_GATEWAY,
    );

    let stored = harness.stored_installations(tenant.organization).await;
    assert!(
        find_installation(&stored, untouched.installation_id)
            .revoked_at
            .is_none(),
        "an App whose listing failed revokes nothing"
    );
    assert!(
        find_installation(&stored, missing.installation_id)
            .revoked_at
            .is_some(),
        "the healthy App's missing installation is revoked"
    );
    assert_eq!(
        find_installation(&stored, fresh).github_app_id,
        healthy.id,
        "the healthy App's new installation is recorded"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_delete_without_uninstall_skips_github() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let explicit = harness.installation(&tenant).await;
    let implicit = harness.installation(&tenant).await;
    routes_deleting_installation(&harness.github, APP_ID, explicit.installation_id, 204, 0).await;
    routes_deleting_installation(&harness.github, APP_ID, implicit.installation_id, 204, 0).await;

    for (seeded, query) in [(&explicit, "?uninstall=false"), (&implicit, "")] {
        harness
            .delete(
                &installation_path(&tenant, seeded.installation, query),
                Some(&tenant.owner.token),
            )
            .await
            .assert_status(StatusCode::NO_CONTENT);
        assert!(
            harness
                .stored_installation(tenant.organization, seeded.installation)
                .await
                .is_none(),
            "the installation is removed from Zone ({query:?})"
        );
    }

    harness.discard().await;
}

#[tokio::test]
async fn installations_delete_with_uninstall_removes_it_on_github() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 204, 1).await;

    harness
        .delete(
            &installation_path(&tenant, seeded.installation, "?uninstall=true"),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);

    assert!(
        harness
            .stored_installation(tenant.organization, seeded.installation)
            .await
            .is_none()
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_delete_already_gone_on_github_removes_the_row() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 404, 1).await;

    harness
        .delete(
            &installation_path(&tenant, seeded.installation, "?uninstall=true"),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);

    assert!(
        harness
            .stored_installation(tenant.organization, seeded.installation)
            .await
            .is_none()
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_delete_github_failure_is_bad_gateway_and_keeps_the_row() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 500, 1..).await;

    assert_refused(
        &harness
            .delete(
                &installation_path(&tenant, seeded.installation, "?uninstall=true"),
                Some(&tenant.owner.token),
            )
            .await,
        StatusCode::BAD_GATEWAY,
    );

    assert!(
        harness
            .active(tenant.organization, seeded.installation)
            .await,
        "the installation is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_delete_with_invalid_uninstall_is_bad_request() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_deleting_installation(&harness.github, APP_ID, seeded.installation_id, 204, 0).await;

    assert_refused(
        &harness
            .delete(
                &installation_path(&tenant, seeded.installation, "?uninstall=maybe"),
                Some(&tenant.owner.token),
            )
            .await,
        StatusCode::BAD_REQUEST,
    );

    assert!(
        harness
            .active(tenant.organization, seeded.installation)
            .await,
        "the installation is kept"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_delete_unknown_is_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    harness
        .delete(
            &installation_path(&tenant, Uuid::new_v4(), "?uninstall=true"),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    harness.discard().await;
}

#[tokio::test]
async fn installations_sync_keeps_an_installation_recorded_while_github_lists() {
    let listing = std::time::Duration::from_millis(750);
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let app = harness.organization_app(&tenant).await;
    let stale = harness.installation_of(&app).await;
    routes_listing_app_installations(
        &harness.github,
        app.app_id,
        ResponseTemplate::new(200)
            .set_body_json(json!([]))
            .set_delay(listing),
        1,
    )
    .await;

    let sync = sync_path(&tenant);
    let (synced, recorded) = tokio::join!(harness.post(&sync, Some(&tenant.owner.token)), async {
        tokio::time::sleep(listing / 3).await;
        harness.installation_of(&app).await
    },);

    synced.assert_status(StatusCode::NO_CONTENT);
    assert!(
        !harness.active(tenant.organization, stale.id).await,
        "an installation GitHub no longer lists is revoked"
    );
    assert!(
        harness.active(tenant.organization, recorded.id).await,
        "an installation recorded after the listing began is not revoked by it"
    );

    harness.discard().await;
}

#[tokio::test]
async fn installations_path_id_that_is_not_a_uuid_is_a_json_not_found_after_the_guard() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let member = harness.member(&tenant).await;
    let paths = [
        (
            Method::POST,
            format!("{}/not-a-uuid/installations", apps_path(&tenant)),
        ),
        (
            Method::DELETE,
            format!(
                "{}/installations/not-a-uuid?uninstall=true",
                connection_path(&tenant)
            ),
        ),
    ];

    for (method, uri) in paths {
        assert_refused(
            &harness
                .call(method.clone(), &uri, Some(&member.token), Body::empty())
                .await,
            StatusCode::FORBIDDEN,
        );
        assert_refused(
            &harness
                .call(method, &uri, Some(&tenant.owner.token), Body::empty())
                .await,
            StatusCode::NOT_FOUND,
        );
    }
    assert_eq!(harness.github_requests().await, 0, "GitHub is never asked");

    harness.discard().await;
}

#[tokio::test]
async fn authorization_repositories_without_token_is_unauthorized() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 0).await;

    assert_each(
        vec![
            (
                "#9",
                harness.get(&repositories_path(&tenant, ""), None).await,
            ),
            (
                "#10",
                harness
                    .get(
                        &branches_path(&tenant, seeded.installation, "?page=1"),
                        None,
                    )
                    .await,
            ),
        ],
        StatusCode::UNAUTHORIZED,
        "a caller without a token",
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_repositories_another_organizations_admin_is_forbidden() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 0).await;
    let token = Some(stranger.owner.token.as_str());

    assert_each(
        vec![
            (
                "#9",
                harness.get(&repositories_path(&tenant, ""), token).await,
            ),
            (
                "#10",
                harness
                    .get(
                        &branches_path(&tenant, seeded.installation, "?page=1"),
                        token,
                    )
                    .await,
            ),
        ],
        StatusCode::FORBIDDEN,
        "another organization's admin",
    );

    harness.discard().await;
}

#[tokio::test]
async fn authorization_repositories_member_lists_repositories_and_branches() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let member = harness.member(&tenant).await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[routes_listed_repository(1, "acme", "widgets")]),
        1..,
    )
    .await;
    routes_listing_branches(
        &harness.github,
        seeded.installation_id,
        1,
        ResponseTemplate::new(200).set_body_json(json!([routes_listed_branch("main", true)])),
    )
    .await;

    let catalog: Catalog = parsed(
        &harness
            .get(&repositories_path(&tenant, ""), Some(&member.token))
            .await,
        StatusCode::OK,
    );
    assert_eq!(catalog.full_names(), ["acme/widgets"]);

    let branches: Branches = parsed(
        &harness
            .get(
                &branches_path(&tenant, seeded.installation, "?page=1"),
                Some(&member.token),
            )
            .await,
        StatusCode::OK,
    );
    assert_eq!(branches.branches.len(), 1);

    harness.discard().await;
}

#[tokio::test]
async fn authorization_repositories_foreign_installation_is_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let foreign = harness.installation(&stranger).await;
    routes_minting(&harness.github, APP_ID, foreign.installation_id, 0).await;

    harness
        .get(
            &repositories_path(
                &tenant,
                &format!("?installation_id={}", foreign.installation),
            ),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);
    harness
        .branches(&tenant, foreign.installation, "?page=1")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    assert!(
        harness
            .active(stranger.organization, foreign.installation)
            .await,
        "the other organization's installation is kept as it was"
    );

    harness.discard().await;
}

#[tokio::test]
async fn repositories_carry_the_contract_shape() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    let mut row = routes_listed_repository(77, "acme", "widgets");
    row["private"] = json!(true);
    row["archived"] = json!(true);
    row["default_branch"] = json!("trunk");
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[row]),
        1,
    )
    .await;

    let catalog = harness.catalog(&tenant, "").await;

    assert_eq!(catalog.total, 1);
    assert_eq!(catalog.next_page, None);
    let repository = &catalog.repositories[0];
    assert_eq!(repository.installation_id, seeded.installation);
    assert_eq!(repository.id, 77);
    assert_eq!(repository.owner, "acme");
    assert_eq!(repository.name, "widgets");
    assert_eq!(repository.full_name, "acme/widgets");
    assert!(repository.private);
    assert!(repository.archived);
    assert_eq!(repository.default_branch, "trunk");
    assert_eq!(repository.html_url, "https://github.com/acme/widgets");
    let installation = catalog.installation(seeded.installation);
    assert_eq!(installation.account_login, seeded.account_login);
    assert_eq!(
        installation.html_url,
        format!(
            "https://github.com/organizations/{}/settings/installations/{}",
            seeded.account_login, seeded.installation_id
        )
    );
    assert_eq!(installation.error, None);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_without_installations_are_empty() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    let catalog = harness.catalog(&tenant, "").await;

    assert!(catalog.repositories.is_empty());
    assert!(catalog.installations.is_empty());
    assert_eq!(catalog.total, 0);
    assert_eq!(catalog.next_page, None);

    harness.discard().await;
}

async fn widgets_catalog(harness: &Harness, tenant: &Tenant) -> Seeded {
    let seeded = harness.installation(tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[
            routes_listed_repository(1, "acme", "widgets"),
            routes_listed_repository(2, "acme", "gadgets"),
            routes_listed_repository(3, "other", "tools"),
        ]),
        1..,
    )
    .await;
    seeded
}

#[tokio::test]
async fn repositories_query_is_form_decoded_and_trimmed() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    widgets_catalog(&harness, &tenant).await;

    let catalog = harness.catalog(&tenant, "?query=+Wid+").await;

    assert_eq!(catalog.full_names(), ["acme/widgets"]);
    assert_eq!(catalog.total, 1);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_query_matches_the_full_name_case_insensitively() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    widgets_catalog(&harness, &tenant).await;

    let owner = harness.catalog(&tenant, "?query=ACME").await;
    assert_eq!(owner.full_names(), ["acme/gadgets", "acme/widgets"]);

    let full_name = harness.catalog(&tenant, "?query=AcMe%2FWidGets").await;
    assert_eq!(full_name.full_names(), ["acme/widgets"]);

    let nothing = harness.catalog(&tenant, "?query=nothing-matches").await;
    assert!(nothing.repositories.is_empty());
    assert_eq!(nothing.total, 0);
    assert_eq!(
        nothing.installations.len(),
        1,
        "the installations are listed whatever the query"
    );

    harness.discard().await;
}

#[tokio::test]
async fn repositories_sort_by_account_then_name_across_accounts() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let mut seeded = [
        harness.installation(&tenant).await,
        harness.installation(&tenant).await,
    ];
    seeded.sort_by_key(|installation| installation.account_login.to_lowercase());
    let [first, second] = &seeded;
    for (installation, rows) in [
        (second, [(1, "Beta"), (2, "aardvark")]),
        (first, [(3, "Zeta"), (4, "alpha")]),
    ] {
        routes_minting(&harness.github, APP_ID, installation.installation_id, 1..).await;
        let rows: Vec<Value> = rows
            .iter()
            .map(|(id, name)| routes_listed_repository(*id, &installation.account_login, name))
            .collect();
        routes_listing_repositories(
            &harness.github,
            installation.installation_id,
            routes_repositories_page(&rows),
            1..,
        )
        .await;
    }

    let catalog = harness.catalog(&tenant, "").await;

    assert_eq!(
        catalog.full_names(),
        [
            format!("{}/alpha", first.account_login),
            format!("{}/Zeta", first.account_login),
            format!("{}/aardvark", second.account_login),
            format!("{}/Beta", second.account_login),
        ]
    );
    assert_eq!(catalog.total, 4);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_page_with_total_and_next_page() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    let rows: Vec<Value> = ["a-widget", "b-widget", "c-gadget", "d-widget", "e-gadget"]
        .iter()
        .zip(1..)
        .map(|(name, id)| routes_listed_repository(id, "acme", name))
        .collect();
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&rows),
        1..,
    )
    .await;

    let first = harness.catalog(&tenant, "?page=1&per_page=2").await;
    assert_eq!(first.full_names(), ["acme/a-widget", "acme/b-widget"]);
    assert_eq!(first.total, 5);
    assert_eq!(first.next_page, Some(2));

    let last = harness.catalog(&tenant, "?page=3&per_page=2").await;
    assert_eq!(last.full_names(), ["acme/e-gadget"]);
    assert_eq!(last.total, 5);
    assert_eq!(last.next_page, None);

    let filtered = harness
        .catalog(&tenant, "?query=widget&page=1&per_page=2")
        .await;
    assert_eq!(filtered.full_names(), ["acme/a-widget", "acme/b-widget"]);
    assert_eq!(filtered.total, 3, "the total counts what the query kept");
    assert_eq!(filtered.next_page, Some(2));

    let beyond = harness.catalog(&tenant, "?page=9&per_page=2").await;
    assert!(beyond.repositories.is_empty());
    assert_eq!(beyond.total, 5);
    assert_eq!(beyond.next_page, None);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_refuse_invalid_paging_with_a_json_error() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 0).await;
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[]),
        0,
    )
    .await;

    for query in [
        "?page=0",
        "?per_page=0",
        "?per_page=101",
        "?per_page=abc",
        "?page=abc",
        "?page=-1",
        "?refresh=maybe",
        "?installation_id=not-a-uuid",
    ] {
        let response = harness
            .get(
                &repositories_path(&tenant, query),
                Some(&tenant.owner.token),
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{query}: {}",
            response.text()
        );
        assert!(
            serde_json::from_slice::<ErrorBody>(response.bytes())
                .is_ok_and(|body| !body.error.is_empty()),
            "{query} is refused with a JSON error: {}",
            response.text()
        );
    }

    harness.discard().await;
}

#[tokio::test]
async fn repositories_report_a_revoked_installation_without_asking_github() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let revoked = harness.revoked_installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, revoked.installation_id, 0).await;
    routes_listing_repositories(
        &harness.github,
        revoked.installation_id,
        routes_repositories_page(&[]),
        0,
    )
    .await;

    let catalog = harness.catalog(&tenant, "").await;

    assert!(catalog.repositories.is_empty());
    let installation = catalog.installation(revoked.installation);
    assert_eq!(installation.error, Some(Unlisted::Revoked));
    assert_eq!(installation.account_login, revoked.account_login);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_report_a_suspended_installation_without_asking_github() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let suspended = harness.suspended_installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, suspended.installation_id, 0).await;
    routes_listing_repositories(
        &harness.github,
        suspended.installation_id,
        routes_repositories_page(&[]),
        0,
    )
    .await;

    let catalog = harness.catalog(&tenant, "").await;

    assert!(catalog.repositories.is_empty());
    assert_eq!(
        catalog.installation(suspended.installation).error,
        Some(Unlisted::Suspended)
    );

    harness.discard().await;
}

#[tokio::test]
async fn repositories_github_failure_marks_only_that_installation_unavailable() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let failing = harness.installation(&tenant).await;
    let healthy = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, failing.installation_id, 1..).await;
    routes_minting(&harness.github, APP_ID, healthy.installation_id, 1..).await;
    routes_listing_repositories(
        &harness.github,
        failing.installation_id,
        ResponseTemplate::new(500),
        1..,
    )
    .await;
    routes_listing_repositories(
        &harness.github,
        healthy.installation_id,
        routes_repositories_page(&[routes_listed_repository(
            9,
            &healthy.account_login,
            "widgets",
        )]),
        1..,
    )
    .await;

    let catalog = harness.catalog(&tenant, "").await;

    assert_eq!(
        catalog.full_names(),
        [format!("{}/widgets", healthy.account_login)]
    );
    assert_eq!(
        catalog.installation(failing.installation).error,
        Some(Unlisted::Unavailable)
    );
    assert_eq!(catalog.installation(healthy.installation).error, None);
    assert!(
        harness
            .active(tenant.organization, failing.installation)
            .await,
        "a GitHub outage revokes nothing"
    );

    harness.discard().await;
}

#[tokio::test]
async fn repositories_report_an_installation_github_uninstalled_as_revoked() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_refusing_mint(&harness.github, APP_ID, seeded.installation_id, 404).await;
    github_app::uninstalled(&harness.github, seeded.installation_id).await;

    let catalog = harness.catalog(&tenant, "").await;

    assert!(catalog.repositories.is_empty());
    assert_eq!(
        catalog.installation(seeded.installation).error,
        Some(Unlisted::Revoked)
    );
    assert!(
        harness
            .stored_installation(tenant.organization, seeded.installation)
            .await
            .is_some_and(|installation| installation.revoked_at.is_some()),
        "the installation is marked revoked"
    );

    harness.discard().await;
}

#[tokio::test]
async fn repositories_are_cached_until_refreshed() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[routes_listed_repository(1, "acme", "widgets")]),
        1,
    )
    .await;

    let fetched = harness.catalog(&tenant, "").await;
    let cached = harness.catalog(&tenant, "?query=wid").await;
    assert_eq!(fetched.full_names(), ["acme/widgets"]);
    assert_eq!(cached.full_names(), ["acme/widgets"]);
    harness.github.verify().await;
    harness.github.reset().await;

    routes_minting(&harness.github, APP_ID, seeded.installation_id, 0..).await;
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[
            routes_listed_repository(1, "acme", "widgets"),
            routes_listed_repository(2, "acme", "gadgets"),
        ]),
        1,
    )
    .await;

    let refreshed = harness.catalog(&tenant, "?refresh=true").await;
    assert_eq!(refreshed.full_names(), ["acme/gadgets", "acme/widgets"]);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_of_an_unknown_installation_are_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    harness.installation(&tenant).await;

    harness
        .get(
            &repositories_path(&tenant, &format!("?installation_id={}", Uuid::new_v4())),
            Some(&tenant.owner.token),
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    harness.discard().await;
}

#[tokio::test]
async fn repositories_filter_by_installation() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let chosen = harness.installation(&tenant).await;
    let other = harness.installation(&tenant).await;
    for (installation, id, name) in [(&chosen, 1, "chosen"), (&other, 2, "other")] {
        routes_minting(&harness.github, APP_ID, installation.installation_id, 0..).await;
        routes_listing_repositories(
            &harness.github,
            installation.installation_id,
            routes_repositories_page(&[routes_listed_repository(
                id,
                &installation.account_login,
                name,
            )]),
            0..,
        )
        .await;
    }

    let catalog = harness
        .catalog(
            &tenant,
            &format!("?installation_id={}", chosen.installation),
        )
        .await;

    assert_eq!(
        catalog.full_names(),
        [format!("{}/chosen", chosen.account_login)]
    );
    assert!(
        catalog
            .repositories
            .iter()
            .all(|repository| repository.installation_id == chosen.installation)
    );

    harness.discard().await;
}

#[tokio::test]
async fn repositories_drop_a_repository_without_an_absolute_web_address() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    routes_listing_repositories(
        &harness.github,
        seeded.installation_id,
        routes_repositories_page(&[
            routes_listed_repository(1, "acme", "widgets"),
            routes_listed_repository_at(2, "acme", "relative", "acme/relative"),
            routes_listed_repository_at(3, "acme", "rooted", "/acme/rooted"),
            routes_listed_repository_at(4, "acme", "ftp", "ftp://github.com/acme/ftp"),
        ]),
        1..,
    )
    .await;

    let catalog = harness.catalog(&tenant, "").await;

    assert_eq!(catalog.full_names(), ["acme/widgets"]);
    assert_eq!(catalog.total, 1);
    assert_eq!(catalog.installation(seeded.installation).error, None);

    harness.discard().await;
}

#[tokio::test]
async fn branches_list_names_as_github_spells_them_with_next_page() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    let mut rows = vec![
        routes_listed_branch("main", true),
        routes_listed_branch("+release", false),
    ];
    rows.extend(
        (rows.len()..BRANCH_PAGE_SIZE)
            .map(|index| routes_listed_branch(&format!("feature/{index}"), false)),
    );
    routes_listing_branches(
        &harness.github,
        seeded.installation_id,
        1,
        ResponseTemplate::new(200).set_body_json(rows),
    )
    .await;

    let branches: Branches = parsed(
        &harness
            .branches(&tenant, seeded.installation, "?page=1")
            .await,
        StatusCode::OK,
    );

    assert_eq!(branches.branches.len(), BRANCH_PAGE_SIZE);
    assert_eq!(branches.next_page, Some(2));
    assert_eq!(branches.branches[0].name, "main");
    assert!(branches.branches[0].protected);
    assert_eq!(branches.branches[1].name, "+release");
    assert!(!branches.branches[1].protected);

    harness.discard().await;
}

#[tokio::test]
async fn branches_last_page_has_no_next_page() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    routes_listing_branches(
        &harness.github,
        seeded.installation_id,
        2,
        ResponseTemplate::new(200).set_body_json(json!([
            routes_listed_branch("release/1", false),
            routes_listed_branch("release/2", false),
        ])),
    )
    .await;

    let branches: Branches = parsed(
        &harness
            .branches(&tenant, seeded.installation, "?page=2")
            .await,
        StatusCode::OK,
    );

    assert_eq!(branches.branches.len(), 2);
    assert_eq!(branches.next_page, None);

    harness.discard().await;
}

#[tokio::test]
async fn branches_of_an_unknown_installation_are_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    harness
        .branches(&tenant, Uuid::new_v4(), "?page=1")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    harness.discard().await;
}

#[tokio::test]
async fn branches_of_a_revoked_or_suspended_installation_are_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let revoked = harness.revoked_installation(&tenant).await;
    let suspended = harness.suspended_installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, revoked.installation_id, 0).await;
    routes_minting(&harness.github, APP_ID, suspended.installation_id, 0).await;

    for seeded in [&revoked, &suspended] {
        harness
            .branches(&tenant, seeded.installation, "?page=1")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    harness.discard().await;
}

#[tokio::test]
async fn branches_of_a_repository_github_does_not_find_are_not_found() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    routes_listing_branches(
        &harness.github,
        seeded.installation_id,
        1,
        ResponseTemplate::new(404).set_body_json(json!({ "message": "Not Found" })),
    )
    .await;

    harness
        .branches(&tenant, seeded.installation, "?page=1")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    harness.discard().await;
}

#[tokio::test]
async fn branches_github_failure_is_bad_gateway() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 1..).await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/branches"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1..)
        .mount(&harness.github)
        .await;

    assert_refused(
        &harness
            .branches(&tenant, seeded.installation, "?page=1")
            .await,
        StatusCode::BAD_GATEWAY,
    );

    harness.discard().await;
}

#[tokio::test]
async fn branches_page_zero_is_bad_request() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    routes_minting(&harness.github, APP_ID, seeded.installation_id, 0).await;

    assert_refused(
        &harness
            .branches(&tenant, seeded.installation, "?page=0")
            .await,
        StatusCode::BAD_REQUEST,
    );
    harness
        .branches(&tenant, seeded.installation, "?page=abc")
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    harness.discard().await;
}
