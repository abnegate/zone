//! Organization-managed MCP servers over the API: who may read and write them,
//! that their secrets are sealed and never handed back, what a definition may
//! hold, how an edit reaches the running connection, and how a workspace
//! narrows what its organization offers.

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_core::variables::Variables;
use zone_server::config::Config;

use crate::common::{self, TestClient, TestResponse, test_email, test_password};

const ENABLED_VARIABLE: &str = "ZONE_MCP_ENABLED";
const SERVERS_VARIABLE: &str = "ZONE_MCP_SERVERS";
const CONFIG_VARIABLE: &str = "ZONE_MCP_CONFIG";
const AUTO_CONNECT_VARIABLE: &str = "ZONE_MCP_AUTO_CONNECT";
const AUTO_MAGENTS_VARIABLE: &str = "ZONE_MCP_AUTO_MAGENTS";

const CONNECTED: &str = "connected";
const CONNECTING: &str = "connecting";
const DISABLED: &str = "disabled";

const ORGANIZATION: &str = "organization";
const INSTANCE: &str = "instance";

const ORGANIZATION_ADMIN: &str = "admin";
const ORGANIZATION_MEMBER: &str = "member";
const WORKSPACE_ADMIN: &str = "admin";
const WORKSPACE_MEMBER: &str = "member";

const STUB_TOOL: &str = "delegate";
const REMOTE_URL: &str = "http://127.0.0.1:1/mcp";
const HEADER: &str = "Authorization";
const HEADER_SECRET: &str = "s3cr3t";
const ENVIRONMENT: &str = "TOKEN";
const ENVIRONMENT_SECRET: &str = "v4lue";
const SEALED_PREFIX: &str = "ENC[";
const INSTANCE_SERVER: &str = "shared";

const MCP_SERVER_RESOURCE: &str = "mcp_server";
const WORKSPACE_MCP_SERVERS_RESOURCE: &str = "workspace_mcp_servers";
const CREATED: &str = "mcp_server.created";
const UPDATED: &str = "mcp_server.updated";
const DELETED: &str = "mcp_server.deleted";
const RECONNECTED: &str = "mcp_server.reconnected";
const WORKSPACE_UPDATED: &str = "workspace_mcp_servers.updated";

const SETTLE: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(50);

const SERVER_FIELDS: [&str; 16] = [
    "arguments",
    "available_tools",
    "command",
    "created_at",
    "enabled",
    "environment",
    "error",
    "headers",
    "id",
    "name",
    "source",
    "status",
    "tools",
    "transport",
    "updated_at",
    "url",
];
const WORKSPACE_FIELDS: [&str; 2] = ["effective", "workspace_enabled"];

/// The process environment with no instance MCP server, held for the test: `magents` on PATH
/// would otherwise appear as an instance row on one machine and not another.
fn isolated() -> Variables {
    let mut environment = Variables::isolated(&[
        ENABLED_VARIABLE,
        SERVERS_VARIABLE,
        CONFIG_VARIABLE,
        AUTO_CONNECT_VARIABLE,
        AUTO_MAGENTS_VARIABLE,
    ]);
    environment.set(AUTO_CONNECT_VARIABLE, "false");
    environment
}

async fn client(commands: bool) -> TestClient {
    TestClient::with_config(Config {
        mcp_commands: commands,
        ..common::test_config()
    })
    .await
}

struct Account {
    token: String,
    user: Uuid,
}

async fn account(client: &TestClient) -> Account {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    Account {
        token: body["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("registration returns an access token: {body}"))
            .to_string(),
        user: body["user"]["id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .unwrap_or_else(|| panic!("registration returns the user: {body}")),
    }
}

async fn organization(client: &TestClient, token: &str) -> Uuid {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    response.json_value()["organization"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the organization is created: {}", response.text()))
}

async fn workspace(client: &TestClient, token: &str, organization: Uuid) -> Uuid {
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Tenant workspace", "slug": Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the workspace is created: {}", response.text()))
}

async fn join_organization(
    client: &TestClient,
    token: &str,
    organization: Uuid,
    user: Uuid,
    role: &str,
) {
    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": user, "role": role }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

async fn join_workspace(client: &TestClient, token: &str, workspace: Uuid, user: Uuid, role: &str) {
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": user, "role": role }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

/// An organization with one workspace, made by its owner.
struct Tenant {
    owner: Account,
    organization: Uuid,
    workspace: Uuid,
}

impl Tenant {
    async fn new(client: &TestClient) -> Self {
        let owner = account(client).await;
        let organization = organization(client, &owner.token).await;
        let workspace = workspace(client, &owner.token, organization).await;
        Self {
            owner,
            organization,
            workspace,
        }
    }

    async fn member(&self, client: &TestClient, role: &str) -> Account {
        let member = account(client).await;
        join_organization(
            client,
            &self.owner.token,
            self.organization,
            member.user,
            role,
        )
        .await;
        member
    }

    async fn workspace_member(&self, client: &TestClient, role: &str) -> Account {
        let member = self.member(client, ORGANIZATION_MEMBER).await;
        join_workspace(client, &self.owner.token, self.workspace, member.user, role).await;
        member
    }

    fn servers(&self) -> String {
        format!("/api/organizations/{}/mcp-servers", self.organization)
    }

    fn server(&self, id: &str) -> String {
        format!("{}/{id}", self.servers())
    }

    fn connection(&self, id: &str) -> String {
        format!("{}/{id}/connection", self.servers())
    }

    fn workspace_servers(&self) -> String {
        format!(
            "/api/organizations/{}/workspaces/{}/mcp-servers",
            self.organization, self.workspace
        )
    }

    async fn create(&self, client: &TestClient, body: &Value) -> Value {
        let response = client
            .post_json_auth(&self.servers(), body, &self.owner.token)
            .await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()
    }

    async fn discard(&self, client: &TestClient, others: &[&Account]) {
        let users: Vec<Uuid> = std::iter::once(self.owner.user)
            .chain(others.iter().map(|account| account.user))
            .collect();
        common::discard(client.state().db(), self.workspace, &users).await;
    }
}

fn stub() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_mcp_server.py")
        .to_string_lossy()
        .into_owned()
}

fn stdio(name: &str) -> Value {
    json!({
        "name": name,
        "transport": "stdio",
        "command": "python3",
        "arguments": [stub()],
    })
}

fn remote(name: &str) -> Value {
    json!({ "name": name, "transport": "http", "url": REMOTE_URL })
}

fn id(server: &Value) -> String {
    server["id"]
        .as_str()
        .unwrap_or_else(|| panic!("an organization server has an id: {server}"))
        .to_string()
}

fn names(list: &Value) -> Vec<&str> {
    list["servers"]
        .as_array()
        .unwrap_or_else(|| panic!("a list holds servers: {list}"))
        .iter()
        .map(|server| server["name"].as_str().expect("every server is named"))
        .collect()
}

fn fields(server: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = server
        .as_object()
        .unwrap_or_else(|| panic!("a server is an object: {server}"))
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

fn assert_sealed(response: &TestResponse) {
    let text = response.text();
    assert!(
        !text.contains(HEADER_SECRET) && !text.contains(ENVIRONMENT_SECRET),
        "a response handed a secret value back: {text}"
    );
}

/// GET `uri` until the server it returns reaches `status`, and hand that body back.
async fn settle(client: &TestClient, uri: &str, token: &str, status: &str) -> Value {
    let deadline = tokio::time::Instant::now() + SETTLE;
    loop {
        let response = client.get_auth(uri, token).await;
        response.assert_status(StatusCode::OK);
        let server = response.json_value();
        if server["status"] == status {
            return server;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the server never reached {status} within {SETTLE:?}: {server}"
        );
        tokio::time::sleep(POLL).await;
    }
}

#[tokio::test]
async fn an_organization_member_reads_but_only_an_administrator_writes() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let member = tenant.member(&client, ORGANIZATION_MEMBER).await;
    let admin = tenant.member(&client, ORGANIZATION_ADMIN).await;
    let probe = id(&tenant.create(&client, &remote("probe")).await);

    let listed = client.get_auth(&tenant.servers(), &member.token).await;
    listed.assert_status(StatusCode::OK);
    assert_eq!(names(&listed.json_value()), ["probe"]);
    client
        .get_auth(&tenant.server(&probe), &member.token)
        .await
        .assert_status(StatusCode::OK);

    let refused = [
        client
            .post_json_auth(&tenant.servers(), &remote("member-made"), &member.token)
            .await,
        client
            .put_json_auth(
                &tenant.server(&probe),
                &json!({ "tools": ["one"] }),
                &member.token,
            )
            .await,
        client
            .delete_auth(&tenant.server(&probe), &member.token)
            .await,
        client
            .post_json_auth(&tenant.connection(&probe), &json!({}), &member.token)
            .await,
    ];
    for response in &refused {
        response.assert_status(StatusCode::FORBIDDEN);
    }

    let created = client
        .post_json_auth(&tenant.servers(), &remote("admin-made"), &admin.token)
        .await;
    created.assert_status(StatusCode::CREATED);
    let updated = client
        .put_json_auth(
            &tenant.server(&probe),
            &json!({ "tools": ["one"] }),
            &admin.token,
        )
        .await;
    updated.assert_status(StatusCode::OK);
    assert_eq!(updated.json_value()["tools"], json!(["one"]));
    let reconnecting = client
        .post_json_auth(&tenant.connection(&probe), &json!({}), &admin.token)
        .await;
    reconnecting.assert_status(StatusCode::ACCEPTED);
    assert_eq!(reconnecting.json_value()["status"], CONNECTING);
    client
        .delete_auth(&tenant.server(&id(&created.json_value())), &admin.token)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let listed = client.get_auth(&tenant.servers(), &member.token).await;
    assert_eq!(names(&listed.json_value()), ["probe"]);

    tenant.discard(&client, &[&member, &admin]).await;
}

#[tokio::test]
async fn a_stranger_finds_nothing_on_any_route() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let stranger = account(&client).await;
    organization(&client, &stranger.token).await;
    let probe = id(&tenant.create(&client, &remote("probe")).await);

    let answers = [
        client.get_auth(&tenant.servers(), &stranger.token).await,
        client
            .post_json_auth(&tenant.servers(), &remote("intruder"), &stranger.token)
            .await,
        client
            .get_auth(&tenant.server(&probe), &stranger.token)
            .await,
        client
            .put_json_auth(
                &tenant.server(&probe),
                &json!({ "enabled": false }),
                &stranger.token,
            )
            .await,
        client
            .delete_auth(&tenant.server(&probe), &stranger.token)
            .await,
        client
            .post_json_auth(&tenant.connection(&probe), &json!({}), &stranger.token)
            .await,
        client
            .get_auth(&tenant.workspace_servers(), &stranger.token)
            .await,
        client
            .put_json_auth(
                &tenant.workspace_servers(),
                &json!({ "servers": { probe.as_str(): false } }),
                &stranger.token,
            )
            .await,
    ];
    for response in &answers {
        response.assert_status(StatusCode::NOT_FOUND);
    }

    let kept = client
        .get_auth(&tenant.server(&probe), &tenant.owner.token)
        .await;
    kept.assert_status(StatusCode::OK);
    assert_eq!(kept.json_value()["enabled"], true);

    tenant.discard(&client, &[&stranger]).await;
}

#[tokio::test]
async fn a_member_outside_the_workspace_and_a_foreign_workspace_find_nothing() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let outsider = tenant.member(&client, ORGANIZATION_ADMIN).await;
    let probe = id(&tenant.create(&client, &remote("probe")).await);

    client
        .get_auth(&tenant.workspace_servers(), &outsider.token)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    client
        .put_json_auth(
            &tenant.workspace_servers(),
            &json!({ "servers": { probe.as_str(): false } }),
            &outsider.token,
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    let elsewhere = organization(&client, &tenant.owner.token).await;
    let foreign = workspace(&client, &tenant.owner.token, elsewhere).await;
    let crossed = format!(
        "/api/organizations/{}/workspaces/{foreign}/mcp-servers",
        tenant.organization
    );
    client
        .get_auth(&crossed, &tenant.owner.token)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    client
        .put_json_auth(
            &crossed,
            &json!({ "servers": { probe.as_str(): false } }),
            &tenant.owner.token,
        )
        .await
        .assert_status(StatusCode::NOT_FOUND);

    tenant.discard(&client, &[&outsider]).await;
}

#[tokio::test]
async fn a_command_server_needs_the_operator_flag() {
    let _environment = isolated();
    let off = client(false).await;
    let tenant = Tenant::new(&off).await;

    off.post_json_auth(&tenant.servers(), &stdio("stub"), &tenant.owner.token)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    let listed = off.get_auth(&tenant.servers(), &tenant.owner.token).await;
    listed.assert_status(StatusCode::OK);
    assert_eq!(
        listed.json_value(),
        json!({ "commands": false, "servers": [] })
    );

    let on = client(true).await;
    let created = on
        .post_json_auth(&tenant.servers(), &stdio("stub"), &tenant.owner.token)
        .await;
    created.assert_status(StatusCode::CREATED);
    let created = created.json_value();
    assert_eq!(created["transport"], "stdio");
    assert_eq!(created["command"], "python3");
    assert_eq!(created["arguments"], json!([stub()]));
    assert_eq!(created["url"], Value::Null);
    let listed = on
        .get_auth(&tenant.servers(), &tenant.owner.token)
        .await
        .json_value();
    assert_eq!(listed["commands"], true);
    assert_eq!(names(&listed), ["stub"]);

    tenant.discard(&on, &[]).await;
}

#[tokio::test]
async fn secrets_are_sealed_and_never_handed_back() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;

    let response = client
        .post_json_auth(
            &tenant.servers(),
            &json!({
                "name": "remote",
                "transport": "http",
                "url": REMOTE_URL,
                "headers": { HEADER: format!("Bearer {HEADER_SECRET}") },
                "environment": { ENVIRONMENT: ENVIRONMENT_SECRET },
            }),
            &tenant.owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    assert_sealed(&response);
    let created = response.json_value();
    assert_eq!(fields(&created), SERVER_FIELDS);
    assert_eq!(created["name"], "remote");
    assert_eq!(created["source"], ORGANIZATION);
    assert_eq!(created["transport"], "http");
    assert_eq!(created["command"], Value::Null);
    assert_eq!(created["arguments"], json!([]));
    assert_eq!(created["url"], REMOTE_URL);
    assert_eq!(created["tools"], Value::Null);
    assert_eq!(created["enabled"], true);
    assert_eq!(
        created["headers"],
        json!([{ "name": HEADER, "has_value": true }])
    );
    assert_eq!(
        created["environment"],
        json!([{ "name": ENVIRONMENT, "has_value": true }])
    );
    assert!(created["created_at"].is_string(), "{created}");
    assert!(created["updated_at"].is_string(), "{created}");
    let server = id(&created);

    let listed = client
        .get_auth(&tenant.servers(), &tenant.owner.token)
        .await;
    listed.assert_status(StatusCode::OK);
    assert_sealed(&listed);
    assert_eq!(listed.json_value()["servers"][0]["id"], server.as_str());
    let read = client
        .get_auth(&tenant.server(&server), &tenant.owner.token)
        .await;
    read.assert_status(StatusCode::OK);
    assert_sealed(&read);
    assert_eq!(read.json_value()["headers"], created["headers"]);

    let audited: Vec<String> = sqlx::query_scalar(
        "SELECT new_values::text FROM audit_logs \
         WHERE organization_id = $1 AND resource_type = $2",
    )
    .bind(tenant.organization)
    .bind(MCP_SERVER_RESOURCE)
    .fetch_all(client.state().db())
    .await
    .expect("the audit log is readable");
    assert_eq!(audited.len(), 1, "one create is one audit row: {audited:?}");
    for row in &audited {
        assert!(
            !row.contains(HEADER_SECRET) && !row.contains(ENVIRONMENT_SECRET),
            "the audit log holds a secret value: {row}"
        );
    }

    let stored: String = sqlx::query_scalar("SELECT secrets FROM mcp_servers WHERE id = $1")
        .bind(Uuid::parse_str(&server).expect("a server id"))
        .fetch_one(client.state().db())
        .await
        .expect("the server's secrets are stored");
    assert!(stored.starts_with(SEALED_PREFIX), "{stored}");
    assert!(
        !stored.contains(HEADER_SECRET) && !stored.contains(ENVIRONMENT_SECRET),
        "the secrets column holds a plaintext value"
    );

    tenant.discard(&client, &[]).await;
}

#[tokio::test]
async fn a_null_secret_keeps_its_value_and_an_omitted_one_is_removed() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let server = id(&tenant
        .create(
            &client,
            &json!({
                "name": "remote",
                "transport": "http",
                "url": REMOTE_URL,
                "headers": { HEADER: format!("Bearer {HEADER_SECRET}") },
                "environment": { ENVIRONMENT: ENVIRONMENT_SECRET },
            }),
        )
        .await);
    let environment = json!([{ "name": ENVIRONMENT, "has_value": true }]);

    let kept = client
        .put_json_auth(
            &tenant.server(&server),
            &json!({ "headers": { HEADER: null } }),
            &tenant.owner.token,
        )
        .await;
    kept.assert_status(StatusCode::OK);
    assert_sealed(&kept);
    let kept = kept.json_value();
    assert_eq!(
        kept["headers"],
        json!([{ "name": HEADER, "has_value": true }])
    );
    assert_eq!(kept["environment"], environment);

    client
        .put_json_auth(
            &tenant.server(&server),
            &json!({ "environment": { ENVIRONMENT: null, "UNSAVED": null } }),
            &tenant.owner.token,
        )
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    let removed = client
        .put_json_auth(
            &tenant.server(&server),
            &json!({ "headers": {} }),
            &tenant.owner.token,
        )
        .await;
    removed.assert_status(StatusCode::OK);
    let removed = removed.json_value();
    assert_eq!(removed["headers"], json!([]));
    assert_eq!(removed["environment"], environment);

    let read = client
        .get_auth(&tenant.server(&server), &tenant.owner.token)
        .await
        .json_value();
    assert_eq!(read["headers"], json!([]));
    assert_eq!(read["environment"], environment);

    tenant.discard(&client, &[]).await;
}

#[tokio::test]
async fn an_invalid_definition_is_refused() {
    let _environment = isolated();
    let client = client(true).await;
    let tenant = Tenant::new(&client).await;
    let refused = [
        json!({
            "name": "expanding",
            "transport": "stdio",
            "command": "python3",
            "arguments": [stub(), "${HOME}"],
        }),
        json!({
            "name": "expanding",
            "transport": "stdio",
            "command": "python3",
            "arguments": [stub()],
            "environment": { "WHERE": "${HOME}" },
        }),
        json!({ "name": "events", "transport": "sse", "url": "http://127.0.0.1:1/sse" }),
        json!({ "name": "credentialed", "transport": "http", "url": "http://u:p@host/mcp" }),
        remote(&"n".repeat(33)),
        json!({ "name": "mismatched", "transport": "http", "command": "python3" }),
    ];
    for body in &refused {
        let response = client
            .post_json_auth(&tenant.servers(), body, &tenant.owner.token)
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{body} was not refused: {}",
            response.text()
        );
    }

    let server = id(&tenant.create(&client, &remote("remote")).await);
    client
        .put_json_auth(
            &tenant.server(&server),
            &json!({ "transport": "stdio" }),
            &tenant.owner.token,
        )
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    let listed = client
        .get_auth(&tenant.servers(), &tenant.owner.token)
        .await
        .json_value();
    assert_eq!(names(&listed), ["remote"]);
    assert_eq!(listed["servers"][0]["transport"], "http");

    tenant.discard(&client, &[]).await;
}

#[tokio::test]
async fn a_name_is_unique_in_the_organization_and_against_instance_servers() {
    let mut environment = isolated();
    environment.set(
        SERVERS_VARIABLE,
        format!(r#"{{"mcpServers":{{"{INSTANCE_SERVER}":{{"command":"true"}}}}}}"#),
    );
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let server = id(&tenant.create(&client, &remote("remote")).await);

    client
        .post_json_auth(&tenant.servers(), &remote("remote"), &tenant.owner.token)
        .await
        .assert_status(StatusCode::CONFLICT);
    client
        .post_json_auth(
            &tenant.servers(),
            &remote(INSTANCE_SERVER),
            &tenant.owner.token,
        )
        .await
        .assert_status(StatusCode::CONFLICT);
    client
        .put_json_auth(
            &tenant.server(&server),
            &json!({ "name": INSTANCE_SERVER }),
            &tenant.owner.token,
        )
        .await
        .assert_status(StatusCode::CONFLICT);

    let listed = client
        .get_auth(&tenant.servers(), &tenant.owner.token)
        .await
        .json_value();
    assert_eq!(names(&listed), ["remote", INSTANCE_SERVER]);
    let instance = &listed["servers"][1];
    assert_eq!(fields(instance), SERVER_FIELDS);
    assert_eq!(instance["source"], INSTANCE);
    assert_eq!(instance["id"], Value::Null);
    assert_eq!(instance["created_at"], Value::Null);
    assert_eq!(instance["updated_at"], Value::Null);
    assert_eq!(listed["servers"][0]["source"], ORGANIZATION);

    tenant.discard(&client, &[]).await;
}

#[tokio::test]
async fn an_edit_reconnects_the_server_and_an_allowlist_edit_does_not() {
    let _environment = isolated();
    let client = client(true).await;
    let tenant = Tenant::new(&client).await;
    let token = tenant.owner.token.as_str();
    let server = id(&tenant.create(&client, &stdio("stub")).await);
    let uri = tenant.server(&server);

    let connected = settle(&client, &uri, token, CONNECTED).await;
    assert_eq!(connected["available_tools"], json!([STUB_TOOL]));
    assert_eq!(connected["error"], Value::Null);

    let narrowed = client
        .put_json_auth(&uri, &json!({ "tools": ["other"] }), token)
        .await;
    narrowed.assert_status(StatusCode::OK);
    let narrowed = narrowed.json_value();
    assert_eq!(narrowed["status"], CONNECTED);
    assert_eq!(narrowed["tools"], json!(["other"]));
    assert_eq!(narrowed["available_tools"], json!([STUB_TOOL]));

    let edited = client
        .put_json_auth(&uri, &json!({ "arguments": [stub(), "--again"] }), token)
        .await;
    edited.assert_status(StatusCode::OK);
    let edited = edited.json_value();
    assert_eq!(edited["status"], CONNECTING);
    assert_eq!(edited["arguments"], json!([stub(), "--again"]));
    let reconnected = settle(&client, &uri, token, CONNECTED).await;
    assert_eq!(reconnected["available_tools"], json!([STUB_TOOL]));
    assert_eq!(reconnected["tools"], json!(["other"]));

    let requested = client
        .post_json_auth(&tenant.connection(&server), &json!({}), token)
        .await;
    requested.assert_status(StatusCode::ACCEPTED);
    assert_eq!(requested.json_value()["status"], CONNECTING);
    let again = settle(&client, &uri, token, CONNECTED).await;
    assert_eq!(again["available_tools"], json!([STUB_TOOL]));

    let disabled = client
        .put_json_auth(&uri, &json!({ "enabled": false }), token)
        .await;
    disabled.assert_status(StatusCode::OK);
    let disabled = disabled.json_value();
    assert_eq!(disabled["status"], DISABLED);
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["available_tools"], json!([]));
    let read = client.get_auth(&uri, token).await.json_value();
    assert_eq!(read["status"], DISABLED);

    tenant.discard(&client, &[]).await;
}

#[tokio::test]
async fn a_workspace_override_narrows_what_its_organization_offers() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let admin = tenant.workspace_member(&client, WORKSPACE_ADMIN).await;
    let member = tenant.workspace_member(&client, WORKSPACE_MEMBER).await;
    let server = id(&tenant.create(&client, &remote("remote")).await);

    let read = client
        .get_auth(&tenant.workspace_servers(), &member.token)
        .await;
    read.assert_status(StatusCode::OK);
    let read = read.json_value();
    let row = &read["servers"][0];
    let mut expected: Vec<&str> = SERVER_FIELDS
        .iter()
        .chain(&WORKSPACE_FIELDS)
        .copied()
        .collect();
    expected.sort_unstable();
    assert_eq!(fields(row), expected);
    assert_eq!(names(&read), ["remote"]);
    assert_eq!(row["workspace_enabled"], Value::Null);
    assert_eq!(row["effective"], true);

    client
        .put_json_auth(
            &tenant.workspace_servers(),
            &json!({ "servers": { server.as_str(): false } }),
            &member.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let narrowed = client
        .put_json_auth(
            &tenant.workspace_servers(),
            &json!({ "servers": { server.as_str(): false } }),
            &admin.token,
        )
        .await;
    narrowed.assert_status(StatusCode::OK);
    let narrowed = &narrowed.json_value()["servers"][0];
    assert_eq!(narrowed["workspace_enabled"], false);
    assert_eq!(narrowed["effective"], false);
    assert_eq!(narrowed["enabled"], true);
    let seen = client
        .get_auth(&tenant.workspace_servers(), &member.token)
        .await
        .json_value();
    assert_eq!(seen["servers"][0]["effective"], false);

    let cleared = client
        .put_json_auth(
            &tenant.workspace_servers(),
            &json!({ "servers": { server.as_str(): null } }),
            &admin.token,
        )
        .await;
    cleared.assert_status(StatusCode::OK);
    let cleared = &cleared.json_value()["servers"][0];
    assert_eq!(cleared["workspace_enabled"], Value::Null);
    assert_eq!(cleared["effective"], true);

    client
        .put_json_auth(
            &tenant.server(&server),
            &json!({ "enabled": false }),
            &tenant.owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
    let switched_off = client
        .get_auth(&tenant.workspace_servers(), &member.token)
        .await
        .json_value();
    assert_eq!(switched_off["servers"][0]["workspace_enabled"], Value::Null);
    assert_eq!(switched_off["servers"][0]["effective"], false);

    let other = Tenant::new(&client).await;
    let foreign = id(&other.create(&client, &remote("foreign")).await);
    client
        .put_json_auth(
            &tenant.workspace_servers(),
            &json!({ "servers": { foreign.as_str(): false } }),
            &admin.token,
        )
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    other.discard(&client, &[]).await;
    tenant.discard(&client, &[&admin, &member]).await;
}

#[tokio::test]
async fn a_deleted_server_is_gone() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let server = id(&tenant.create(&client, &remote("remote")).await);

    client
        .delete_auth(&tenant.server(&server), &tenant.owner.token)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    client
        .get_auth(&tenant.server(&server), &tenant.owner.token)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    client
        .delete_auth(&tenant.server(&server), &tenant.owner.token)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    let listed = client
        .get_auth(&tenant.servers(), &tenant.owner.token)
        .await
        .json_value();
    assert_eq!(listed["servers"], json!([]));

    tenant.discard(&client, &[]).await;
}

#[tokio::test]
async fn every_write_is_audited() {
    let _environment = isolated();
    let client = client(false).await;
    let tenant = Tenant::new(&client).await;
    let token = tenant.owner.token.as_str();
    let server = id(&tenant.create(&client, &remote("remote")).await);

    client
        .put_json_auth(&tenant.server(&server), &json!({ "tools": ["one"] }), token)
        .await
        .assert_status(StatusCode::OK);
    client
        .post_json_auth(&tenant.connection(&server), &json!({}), token)
        .await
        .assert_status(StatusCode::ACCEPTED);
    client
        .put_json_auth(
            &tenant.workspace_servers(),
            &json!({ "servers": { server.as_str(): false } }),
            token,
        )
        .await
        .assert_status(StatusCode::OK);
    client
        .delete_auth(&tenant.server(&server), token)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs \
         WHERE organization_id = $1 AND resource_type = ANY($2) \
         ORDER BY created_at, id",
    )
    .bind(tenant.organization)
    .bind(vec![MCP_SERVER_RESOURCE, WORKSPACE_MCP_SERVERS_RESOURCE])
    .fetch_all(client.state().db())
    .await
    .expect("the audit log is readable");
    assert_eq!(
        actions,
        [CREATED, UPDATED, RECONNECTED, WORKSPACE_UPDATED, DELETED]
    );

    tenant.discard(&client, &[]).await;
}
