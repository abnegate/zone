//! Sources and projects backed by a GitHub App installation instead of a
//! stored token: attaching one, refusing one Zone may not use, detaching, and
//! what is left once the installation or its App is deleted.

use crate::common::github_app::{self, Seeded};
use crate::common::{self, TestClient, TestResponse, test_email, test_password};
use abnegate_vcs::pull_request::ApplicationId;
use axum::http::StatusCode;
use chrono::{Duration, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_server::db::github_apps;
use zone_server::db::github_installations::{self, InstallationStatus};
use zone_server::db::projects::{self, NewProject, ProjectRow};
use zone_server::db::sources::{self, GITHUB, NewSource, SourceRow};
use zone_server::state::AppState;

const GITLAB: &str = "gitlab";
const INSTALLATION_ID: i64 = 51_000_001;
const STORED_TOKEN: &str = "ghp_throwaway_stored";
const SENT_TOKEN: &str = "ghp_throwaway_sent";
const REPOSITORY_URL: &str = "https://github.com/acme/widgets";
const DISCONNECTED: &str = "This GitHub account isn't connected to this organization, or it was disconnected. Reconnect GitHub.";
const UNREACHABLE: &str = "Zone can't reach this repository through that GitHub connection.";
const GITHUB_UNAVAILABLE: &str = "GitHub could not be reached.";
const MINTED_TOKEN: &str = "ghs_throwaway_minted";
const REPOSITORY_PATH: &str = "/repos/acme/widgets";

/// Source names are unique per source type across every workspace.
fn source_name(prefix: &str) -> String {
    format!("{prefix} {}", Uuid::new_v4())
}

fn repository() -> Value {
    json!({ "owner": "acme", "repo": "widgets", "branch": "main" })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Active,
    Revoked,
    Suspended,
}

/// Unlike a bare `Option` field, one read through this must be present, so a
/// response that leaves the field out fails rather than reading as null.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Installation {
    #[serde(deserialize_with = "present")]
    github_installation_id: Option<Uuid>,
    #[serde(deserialize_with = "present")]
    github_installation_status: Option<Status>,
    #[serde(deserialize_with = "present")]
    github_account_login: Option<String>,
}

impl Installation {
    fn of(seeded: &Seeded, status: Status) -> Self {
        Self {
            github_installation_id: Some(seeded.installation),
            github_installation_status: Some(status),
            github_account_login: Some(seeded.account_login.clone()),
        }
    }

    fn active(seeded: &Seeded) -> Self {
        Self::of(seeded, Status::Active)
    }

    fn detached() -> Self {
        Self {
            github_installation_id: None,
            github_installation_status: None,
            github_account_login: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct SourceConnection {
    has_credentials: bool,
    #[serde(flatten)]
    installation: Installation,
}

impl SourceConnection {
    fn installed(seeded: &Seeded) -> Self {
        Self {
            has_credentials: false,
            installation: Installation::active(seeded),
        }
    }

    fn detached(has_credentials: bool) -> Self {
        Self {
            has_credentials,
            installation: Installation::detached(),
        }
    }
}

fn source_connection(response: &TestResponse) -> SourceConnection {
    let body = response.json_value();
    serde_json::from_value(body["source"].clone())
        .unwrap_or_else(|error| panic!("the source carries its connection ({error}): {body}"))
}

fn project_installation(response: &TestResponse) -> Installation {
    let body = response.json_value();
    serde_json::from_value(body["project"].clone())
        .unwrap_or_else(|error| panic!("the project carries its installation ({error}): {body}"))
}

fn assert_disconnected(response: &TestResponse) {
    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json_value()["error"].as_str(),
        Some(DISCONNECTED),
        "an installation Zone may not use is refused with the reconnect message"
    );
}

fn assert_unreachable(response: &TestResponse) {
    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json_value()["error"].as_str(),
        Some(UNREACHABLE),
        "a repository the installation cannot read is refused with the reachability message"
    );
}

/// GitHub mints `MINTED_TOKEN` for the installation `calls` times.
async fn issuing(server: &MockServer, installation_id: i64, calls: u64) {
    let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
    Mock::given(method("POST"))
        .and(path(format!(
            "/app/installations/{installation_id}/access_tokens"
        )))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token": MINTED_TOKEN,
            "expires_at": expires_at,
        })))
        .expect(calls)
        .mount(server)
        .await;
}

/// GitHub answers one read of the repository, made with the minted token,
/// with `answer`.
async fn reading_repository(server: &MockServer, answer: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(REPOSITORY_PATH))
        .and(header("authorization", format!("Bearer {MINTED_TOKEN}")))
        .respond_with(answer)
        .expect(1)
        .mount(server)
        .await;
}

async fn reachable(server: &MockServer) {
    issuing(server, INSTALLATION_ID, 1).await;
    reading_repository(
        server,
        ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "main" })),
    )
    .await;
}

fn installation_source_body(installation: Uuid) -> Value {
    json!({
        "name": source_name("Widgets"),
        "source_type": GITHUB,
        "config": repository(),
        "github_installation_id": installation,
    })
}

struct Tenant {
    token: String,
    user: Uuid,
    organization: Uuid,
    workspace: Uuid,
}

/// One client whose GitHub, installation tokens included, is a mock server
/// that answers only what a test mounts, so a test that reached GitHub
/// unexpectedly would fail loudly instead of reaching the real one.
struct Harness {
    client: TestClient,
    github: MockServer,
}

impl Harness {
    async fn new() -> Self {
        let github = MockServer::start().await;
        let client = TestClient::with_config(github_app::config_for(&github)).await;
        client
            .state()
            .install_github(github_app::issuer_provider(&github, client.state()));
        Self::without_indexing(client.state());
        Self { client, github }
    }

    /// Creating or re-crediting a github source spawns indexing, which reads
    /// the repository through the adapter's own, unconfigurable GitHub URL.
    fn without_indexing(state: &AppState) {
        state.index_semaphore().close();
    }

    fn state(&self) -> &AppState {
        self.client.state()
    }

    fn pool(&self) -> &PgPool {
        self.state().db()
    }

    async fn tenant(&self) -> Tenant {
        let suffix = Uuid::new_v4().simple().to_string();
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

        let organization = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "GitHub Org", "slug": format!("github-org-{suffix}") }),
                &token,
            )
            .await
            .json_value()["organization"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("an organization");
        let workspace = self
            .client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({ "name": "GitHub Workspace", "slug": format!("github-ws-{suffix}") }),
                &token,
            )
            .await
            .json_value()["workspace"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("a workspace");

        Tenant {
            token,
            user,
            organization,
            workspace,
        }
    }

    async fn installation(&self, tenant: &Tenant) -> Seeded {
        github_app::seed_installation(self.state(), tenant.organization, INSTALLATION_ID).await
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

    async fn seed_source(
        &self,
        tenant: &Tenant,
        source_type: &str,
        credentials: Option<&str>,
        installation: Option<Uuid>,
    ) -> Uuid {
        let sealed = credentials.map(|token| {
            zone_server::crypto::encrypt(self.state().encryption_key(), token)
                .expect("the token seals")
        });
        sources::create_source(
            self.pool(),
            NewSource {
                workspace_id: tenant.workspace,
                name: &source_name("Widgets"),
                source_type,
                config: repository(),
                description: None,
                url: None,
                credentials_encrypted: sealed.as_deref(),
                github_installation_id: installation,
            },
        )
        .await
        .expect("the source is stored")
        .id
    }

    async fn token_source(&self, tenant: &Tenant) -> Uuid {
        self.seed_source(tenant, GITHUB, Some(STORED_TOKEN), None)
            .await
    }

    async fn installation_source(&self, tenant: &Tenant, installation: Uuid) -> Uuid {
        self.seed_source(tenant, GITHUB, None, Some(installation))
            .await
    }

    async fn stored_source(&self, tenant: &Tenant, id: Uuid) -> SourceRow {
        sources::get_source(self.pool(), id, tenant.workspace)
            .await
            .expect("the source is readable")
            .expect("the source exists")
    }

    async fn stored_sources(&self, tenant: &Tenant) -> Vec<SourceRow> {
        sources::list_sources(self.pool(), tenant.workspace, None, None, 100, 0)
            .await
            .expect("the workspace's sources are readable")
    }

    async fn project(&self, tenant: &Tenant) -> Uuid {
        projects::create(
            self.pool(),
            NewProject {
                name: "Widgets",
                description: None,
                workspace_id: Some(tenant.workspace),
                source_id: None,
                status: None,
            },
        )
        .await
        .expect("the project is stored")
        .id
    }

    async fn installation_project(&self, tenant: &Tenant, installation: Uuid) -> Uuid {
        let project = self.project(tenant).await;
        projects::link_github(
            self.pool(),
            project,
            REPOSITORY_URL,
            None,
            Some(installation),
        )
        .await
        .expect("the project links")
        .expect("the project exists");
        project
    }

    async fn stored_project(&self, id: Uuid) -> ProjectRow {
        projects::get_project(self.pool(), id)
            .await
            .expect("the project is readable")
            .expect("the project exists")
    }

    async fn create_source(&self, tenant: &Tenant, body: Value) -> TestResponse {
        self.client
            .post_json_auth(
                &format!("/api/workspaces/{}/sources", tenant.workspace),
                &body,
                &tenant.token,
            )
            .await
    }

    async fn update_source(&self, tenant: &Tenant, id: Uuid, body: Value) -> TestResponse {
        self.client
            .put_json_auth(
                &format!("/api/workspaces/{}/sources/{id}", tenant.workspace),
                &body,
                &tenant.token,
            )
            .await
    }

    async fn get_source(&self, tenant: &Tenant, id: Uuid) -> TestResponse {
        let response = self
            .client
            .get_auth(
                &format!("/api/workspaces/{}/sources/{id}", tenant.workspace),
                &tenant.token,
            )
            .await;
        response.assert_status(StatusCode::OK);
        response
    }

    async fn link_project(&self, tenant: &Tenant, id: Uuid, body: Value) -> TestResponse {
        self.client
            .post_json_auth(&format!("/api/projects/{id}/github"), &body, &tenant.token)
            .await
    }

    async fn get_project(&self, tenant: &Tenant, id: Uuid) -> TestResponse {
        let response = self
            .client
            .get_auth(&format!("/api/projects/{id}"), &tenant.token)
            .await;
        response.assert_status(StatusCode::OK);
        response
    }

    async fn assert_detached(&self, tenant: &Tenant, source: Uuid, project: Uuid) {
        assert_eq!(
            self.stored_source(tenant, source)
                .await
                .github_installation_id,
            None,
            "the source's installation is nulled in the database"
        );
        assert_eq!(
            self.stored_project(project).await.github_installation_id,
            None,
            "the project's installation is nulled in the database"
        );
        assert_eq!(
            source_connection(&self.get_source(tenant, source).await),
            SourceConnection::detached(false)
        );
        assert_eq!(
            project_installation(&self.get_project(tenant, project).await),
            Installation::detached()
        );
    }

    async fn discard(&self, tenants: &[&Tenant]) {
        for tenant in tenants {
            common::discard(self.pool(), tenant.workspace, &[tenant.user]).await;
        }
    }
}

#[tokio::test]
async fn a_seeded_installation_is_attachable_and_its_key_signs() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;

    let attachable =
        github_installations::attachable(harness.pool(), tenant.workspace, seeded.installation)
            .await
            .expect("the installation is readable")
            .expect("a workspace may attach its own organization's installation");
    assert_eq!(attachable.status(), InstallationStatus::Active);
    assert_eq!(attachable.account_login, seeded.account_login);
    assert_eq!(attachable.installation_id, seeded.installation_id);
    assert_eq!(attachable.github_app_id, seeded.app);

    let credential = github_installations::credential(harness.pool(), seeded.installation)
        .await
        .expect("the credential is readable")
        .expect("the installation has a credential");
    let provider = github_app::issuer_provider(&harness.github, harness.state());
    assert_eq!(
        provider
            .open(&credential.private_key)
            .expect("the key opens under the state's key")
            .expose(),
        github_app::TEST_PRIVATE_KEY,
        "the App's key is sealed the way the provider opens it"
    );
    provider
        .application_jwt(
            ApplicationId::new(u64::try_from(credential.app_id).expect("a positive App id")),
            &credential.private_key,
        )
        .expect("the sealed key signs an App JWT");

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn creating_a_source_on_an_installation_stores_no_token() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;

    let created = harness
        .create_source(&tenant, installation_source_body(seeded.installation))
        .await;

    created.assert_status(StatusCode::CREATED);
    assert_eq!(
        source_connection(&created),
        SourceConnection::installed(&seeded)
    );
    let id = created.json_value()["source"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("the created source's id");
    let stored = harness.stored_source(&tenant, id).await;
    assert_eq!(stored.github_installation_id, Some(seeded.installation));
    assert_eq!(stored.credentials_encrypted, None, "no token is stored");

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn creating_a_source_with_credentials_and_an_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let mut body = installation_source_body(seeded.installation);
    body["credentials"] = json!(SENT_TOKEN);

    let refused = harness.create_source(&tenant, body).await;

    refused.assert_status(StatusCode::BAD_REQUEST);
    assert!(
        harness.stored_sources(&tenant).await.is_empty(),
        "nothing is created"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn creating_a_non_github_source_on_an_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let mut body = installation_source_body(seeded.installation);
    body["source_type"] = json!(GITLAB);

    let refused = harness.create_source(&tenant, body).await;

    refused.assert_status(StatusCode::BAD_REQUEST);
    assert!(
        harness.stored_sources(&tenant).await.is_empty(),
        "nothing is created"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn creating_a_source_on_another_organizations_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let theirs = harness.installation(&stranger).await;

    let refused = harness
        .create_source(&tenant, installation_source_body(theirs.installation))
        .await;

    assert_disconnected(&refused);
    assert!(
        harness.stored_sources(&tenant).await.is_empty(),
        "nothing is created"
    );

    harness.discard(&[&tenant, &stranger]).await;
}

#[tokio::test]
async fn creating_a_source_on_a_revoked_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let revoked = harness.revoked_installation(&tenant).await;

    let refused = harness
        .create_source(&tenant, installation_source_body(revoked.installation))
        .await;

    assert_disconnected(&refused);
    assert!(
        harness.stored_sources(&tenant).await.is_empty(),
        "nothing is created"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn creating_a_source_on_a_suspended_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let suspended = harness.suspended_installation(&tenant).await;

    let refused = harness
        .create_source(&tenant, installation_source_body(suspended.installation))
        .await;

    assert_disconnected(&refused);
    assert!(
        harness.stored_sources(&tenant).await.is_empty(),
        "nothing is created"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn attaching_an_installation_to_a_token_source_clears_the_token() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    reachable(&harness.github).await;

    let attached = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": seeded.installation }),
        )
        .await;

    attached.assert_status(StatusCode::OK);
    assert_eq!(
        source_connection(&attached),
        SourceConnection::installed(&seeded)
    );
    let stored = harness.stored_source(&tenant, source).await;
    assert_eq!(stored.github_installation_id, Some(seeded.installation));
    assert_eq!(
        stored.credentials_encrypted, None,
        "the stored token is cleared"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn attaching_an_installation_alongside_a_rename_returns_the_final_row() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;

    let renamed = source_name("Renamed widgets");
    reachable(&harness.github).await;

    let attached = harness
        .update_source(
            &tenant,
            source,
            json!({ "name": renamed, "github_installation_id": seeded.installation }),
        )
        .await;

    attached.assert_status(StatusCode::OK);
    assert_eq!(
        attached.json_value()["source"]["name"].as_str(),
        Some(renamed.as_str())
    );
    assert_eq!(
        source_connection(&attached),
        SourceConnection::installed(&seeded),
        "the response is the row after the attach, not the row the rename returned"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn attaching_an_installation_reads_the_repository_the_request_names() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    issuing(&harness.github, INSTALLATION_ID, 1).await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/gadgets"))
        .and(header("authorization", format!("Bearer {MINTED_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "main" })))
        .expect(1)
        .mount(&harness.github)
        .await;
    let gadgets = json!({ "owner": "acme", "repo": "gadgets", "branch": "main" });

    let attached = harness
        .update_source(
            &tenant,
            source,
            json!({ "config": gadgets, "github_installation_id": seeded.installation }),
        )
        .await;

    attached.assert_status(StatusCode::OK);
    let stored = harness.stored_source(&tenant, source).await;
    assert_eq!(stored.github_installation_id, Some(seeded.installation));
    assert_eq!(stored.config, gadgets);

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn attaching_an_unreachable_repository_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    let before = harness.stored_source(&tenant, source).await;
    issuing(&harness.github, INSTALLATION_ID, 1).await;
    reading_repository(
        &harness.github,
        ResponseTemplate::new(404).set_body_json(json!({ "message": "Not Found" })),
    )
    .await;

    let refused = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": seeded.installation }),
        )
        .await;

    assert_unreachable(&refused);
    assert!(
        !refused.text().contains(MINTED_TOKEN),
        "the refusal never carries the token"
    );
    let after = harness.stored_source(&tenant, source).await;
    assert_eq!(after.github_installation_id, None, "nothing is attached");
    assert_eq!(
        after.credentials_encrypted, before.credentials_encrypted,
        "the stored token is untouched"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn attaching_while_github_fails_is_a_bad_gateway() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    let before = harness.stored_source(&tenant, source).await;
    issuing(&harness.github, INSTALLATION_ID, 1).await;
    reading_repository(&harness.github, ResponseTemplate::new(503)).await;

    let failed = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": seeded.installation }),
        )
        .await;

    failed.assert_status(StatusCode::BAD_GATEWAY);
    assert_eq!(
        failed.json_value()["error"].as_str(),
        Some(GITHUB_UNAVAILABLE)
    );
    let after = harness.stored_source(&tenant, source).await;
    assert_eq!(after.github_installation_id, None, "nothing is attached");
    assert_eq!(
        after.credentials_encrypted, before.credentials_encrypted,
        "the stored token is untouched"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn attaching_an_installation_github_uninstalled_is_refused_and_marks_it_revoked() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/app/installations/{INSTALLATION_ID}/access_tokens"
        )))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&harness.github)
        .await;

    let refused = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": seeded.installation }),
        )
        .await;

    assert_disconnected(&refused);
    assert_eq!(
        harness
            .stored_source(&tenant, source)
            .await
            .github_installation_id,
        None,
        "nothing is attached"
    );
    let credential = github_installations::credential(harness.pool(), seeded.installation)
        .await
        .expect("the installation is readable")
        .expect("the installation is kept");
    assert_eq!(credential.status(), InstallationStatus::Revoked);

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn detaching_with_null_clears_the_installation() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness
        .installation_source(&tenant, seeded.installation)
        .await;

    let detached = harness
        .update_source(&tenant, source, json!({ "github_installation_id": null }))
        .await;

    detached.assert_status(StatusCode::OK);
    assert_eq!(
        source_connection(&detached),
        SourceConnection::detached(false)
    );
    assert_eq!(
        harness
            .stored_source(&tenant, source)
            .await
            .github_installation_id,
        None
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn leaving_the_installation_out_keeps_it() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness
        .installation_source(&tenant, seeded.installation)
        .await;

    let renamed = harness
        .update_source(
            &tenant,
            source,
            json!({ "name": source_name("Renamed widgets") }),
        )
        .await;

    renamed.assert_status(StatusCode::OK);
    assert_eq!(
        source_connection(&renamed),
        SourceConnection::installed(&seeded)
    );
    assert_eq!(
        harness
            .stored_source(&tenant, source)
            .await
            .github_installation_id,
        Some(seeded.installation)
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn sending_credentials_to_an_installation_source_detaches_it() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness
        .installation_source(&tenant, seeded.installation)
        .await;

    let credited = harness
        .update_source(&tenant, source, json!({ "credentials": SENT_TOKEN }))
        .await;

    credited.assert_status(StatusCode::OK);
    assert_eq!(
        source_connection(&credited),
        SourceConnection::detached(true)
    );
    let stored = harness.stored_source(&tenant, source).await;
    assert_eq!(stored.github_installation_id, None);
    assert!(
        stored.credentials_encrypted.is_some(),
        "the sent token is stored"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn updating_with_credentials_and_an_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    let before = harness.stored_source(&tenant, source).await;

    let refused = harness
        .update_source(
            &tenant,
            source,
            json!({ "credentials": SENT_TOKEN, "github_installation_id": seeded.installation }),
        )
        .await;

    refused.assert_status(StatusCode::BAD_REQUEST);
    let after = harness.stored_source(&tenant, source).await;
    assert_eq!(after.github_installation_id, None, "nothing is attached");
    assert_eq!(
        after.credentials_encrypted, before.credentials_encrypted,
        "the stored token is untouched"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn updating_a_non_github_source_with_an_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness.seed_source(&tenant, GITLAB, None, None).await;

    let refused = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": seeded.installation }),
        )
        .await;

    refused.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        harness
            .stored_source(&tenant, source)
            .await
            .github_installation_id,
        None,
        "nothing is attached"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn updating_with_another_organizations_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let theirs = harness.installation(&stranger).await;
    let source = harness.token_source(&tenant).await;
    let before = harness.stored_source(&tenant, source).await;

    let refused = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": theirs.installation }),
        )
        .await;

    assert_disconnected(&refused);
    let after = harness.stored_source(&tenant, source).await;
    assert_eq!(after.github_installation_id, None, "nothing is attached");
    assert_eq!(
        after.credentials_encrypted, before.credentials_encrypted,
        "the stored token is untouched"
    );

    harness.discard(&[&tenant, &stranger]).await;
}

#[tokio::test]
async fn updating_with_a_revoked_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let revoked = harness.revoked_installation(&tenant).await;
    let source = harness.token_source(&tenant).await;
    let before = harness.stored_source(&tenant, source).await;

    let refused = harness
        .update_source(
            &tenant,
            source,
            json!({ "github_installation_id": revoked.installation }),
        )
        .await;

    assert_disconnected(&refused);
    let after = harness.stored_source(&tenant, source).await;
    assert_eq!(after.github_installation_id, None, "nothing is attached");
    assert_eq!(
        after.credentials_encrypted, before.credentials_encrypted,
        "the stored token is untouched"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn a_source_reports_its_installation_suspended_then_revoked() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness
        .installation_source(&tenant, seeded.installation)
        .await;

    github_installations::mark_suspended(harness.pool(), seeded.installation)
        .await
        .expect("the installation is marked suspended");
    assert_eq!(
        source_connection(&harness.get_source(&tenant, source).await).installation,
        Installation::of(&seeded, Status::Suspended)
    );

    github_installations::mark_revoked(harness.pool(), seeded.installation)
        .await
        .expect("the installation is marked revoked");
    assert_eq!(
        source_connection(&harness.get_source(&tenant, source).await).installation,
        Installation::of(&seeded, Status::Revoked),
        "a revocation outranks a suspension"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn linking_a_project_to_an_installation_stores_no_token() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let project = harness.project(&tenant).await;

    let linked = harness
        .link_project(
            &tenant,
            project,
            json!({ "repo_url": REPOSITORY_URL, "github_installation_id": seeded.installation }),
        )
        .await;

    linked.assert_status(StatusCode::OK);
    assert_eq!(project_installation(&linked), Installation::active(&seeded));
    assert_eq!(
        linked.json_value()["project"]["github_repo_url"].as_str(),
        Some(REPOSITORY_URL)
    );
    let stored = harness.stored_project(project).await;
    assert_eq!(stored.github_installation_id, Some(seeded.installation));
    assert_eq!(stored.github_access_token, None, "no token is stored");

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn linking_with_a_token_and_an_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let project = harness.project(&tenant).await;

    let refused = harness
        .link_project(
            &tenant,
            project,
            json!({
                "repo_url": REPOSITORY_URL,
                "access_token": SENT_TOKEN,
                "github_installation_id": seeded.installation,
            }),
        )
        .await;

    refused.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        harness.stored_project(project).await.github_repo_url,
        None,
        "nothing is linked"
    );

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn linking_to_another_organizations_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let stranger = harness.tenant().await;
    let theirs = harness.installation(&stranger).await;
    let project = harness.project(&tenant).await;

    let refused = harness
        .link_project(
            &tenant,
            project,
            json!({ "repo_url": REPOSITORY_URL, "github_installation_id": theirs.installation }),
        )
        .await;

    assert_disconnected(&refused);
    let stored = harness.stored_project(project).await;
    assert_eq!(stored.github_repo_url, None, "nothing is linked");
    assert_eq!(stored.github_installation_id, None, "nothing is attached");

    harness.discard(&[&tenant, &stranger]).await;
}

#[tokio::test]
async fn linking_to_a_revoked_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let revoked = harness.revoked_installation(&tenant).await;
    let project = harness.project(&tenant).await;

    let refused = harness
        .link_project(
            &tenant,
            project,
            json!({ "repo_url": REPOSITORY_URL, "github_installation_id": revoked.installation }),
        )
        .await;

    assert_disconnected(&refused);
    let stored = harness.stored_project(project).await;
    assert_eq!(stored.github_repo_url, None, "nothing is linked");
    assert_eq!(stored.github_installation_id, None, "nothing is attached");

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn linking_to_a_suspended_installation_is_refused() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let suspended = harness.suspended_installation(&tenant).await;
    let project = harness.project(&tenant).await;

    let refused = harness
        .link_project(
            &tenant,
            project,
            json!({ "repo_url": REPOSITORY_URL, "github_installation_id": suspended.installation }),
        )
        .await;

    assert_disconnected(&refused);
    let stored = harness.stored_project(project).await;
    assert_eq!(stored.github_repo_url, None, "nothing is linked");
    assert_eq!(stored.github_installation_id, None, "nothing is attached");

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn deleting_the_installation_detaches_its_source_and_project() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness
        .installation_source(&tenant, seeded.installation)
        .await;
    let project = harness
        .installation_project(&tenant, seeded.installation)
        .await;

    assert!(
        github_installations::delete(harness.pool(), tenant.organization, seeded.installation)
            .await
            .expect("the installation is deleted"),
        "the installation existed"
    );

    harness.assert_detached(&tenant, source, project).await;

    harness.discard(&[&tenant]).await;
}

#[tokio::test]
async fn deleting_the_app_detaches_its_source_and_project() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let seeded = harness.installation(&tenant).await;
    let source = harness
        .installation_source(&tenant, seeded.installation)
        .await;
    let project = harness
        .installation_project(&tenant, seeded.installation)
        .await;

    assert!(
        github_apps::delete(harness.pool(), tenant.organization, seeded.app)
            .await
            .expect("the App is deleted"),
        "the App existed"
    );

    harness.assert_detached(&tenant, source, project).await;

    harness.discard(&[&tenant]).await;
}
