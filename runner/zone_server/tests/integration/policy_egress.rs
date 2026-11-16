//! What an organization's agent policy lets a chat's or run's tools reach. A policy that
//! restricts the network takes the tools offline as a chat's own setting does, and readmits only
//! `fetch_url` and the MCP servers reached over HTTP at an admitted host; a server list withholds
//! every MCP server it does not name, network or not. A chat offline on its own is readmitted
//! nothing.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::Value;
use serde_json::json;
use tokio::time::Instant;
use uuid::Uuid;
use zone_core::vpn::CHAT_OFFLINE;
use zone_enterprise::policy::Effective;
use zone_enterprise::policy::Rules;
use zone_server::agent::ChatTools;
use zone_server::agent::WorkspaceScope;
use zone_server::agent::wait::KIND_CHECK;
use zone_server::agent::wait::WAIT_FOR;
use zone_server::agent::web::FETCH_URL;
use zone_server::agent::web::WEB_SEARCH;
use zone_server::config::Config;
use zone_server::db::tasks::Origin;
use zone_server::services::mcp_servers::stub::Stub;

use crate::common;
use crate::common::TestClient;
use crate::common::mcp::isolated;
use crate::common::test_email;
use crate::common::test_password;

const ADMITTED: &str = "admitted";
const FOREIGN: &str = "foreign";
const LOCAL: &str = "local";
const SERVERS: [&str; 3] = [ADMITTED, FOREIGN, LOCAL];
const STUB_TOOL: &str = "delegate";
const LOOPBACK: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";
const PATH: &str = "/mcp";
const GITHUB_TOOL: &str = "create_pull_request";
const LIST_MODELS: &str = "list_models";
const OFFLINE_SOURCES: [&str; 1] = ["comfy"];

const CONNECTED: &str = "connected";
const FAILED: &str = "failed";
/// How long a hub waits on one server's handshake, and a margin.
const SETTLE: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(50);

fn tool(server: &str) -> String {
    format!("{server}__{STUB_TOOL}")
}

fn policy(mcp_servers: Option<&[&str]>, domains: Option<&[&str]>) -> Option<Arc<Effective>> {
    let owned = |entries: &[&str]| entries.iter().map(|entry| entry.to_string()).collect();
    Some(Arc::new(Effective::new(
        Rules {
            mcp_servers: mcp_servers.map(owned),
            domains: domains.map(owned),
            ..Rules::default()
        },
        Rules::default(),
    )))
}

/// A policy that admits only the loopback address the stub listens on.
fn loopback() -> Option<Arc<Effective>> {
    policy(None, Some(&[LOOPBACK]))
}

async fn client() -> TestClient {
    TestClient::with_config(Config {
        mcp_commands: true,
        web_search: zone_server::services::search::defaults().with_enabled(true),
        ..common::test_config()
    })
    .await
}

/// An organization with one workspace, made by its owner.
struct Tenant {
    token: String,
    user: Uuid,
    organization: Uuid,
    workspace: Uuid,
}

impl Tenant {
    async fn new(client: &TestClient) -> Self {
        let registered = client
            .post_json(
                "/api/auth/register",
                &json!({ "email": test_email(), "password": test_password() }),
            )
            .await
            .json_value();
        let token = registered["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("registration returns an access token: {registered}"))
            .to_string();
        let user = identifier(&registered["user"]["id"]);
        let organization = client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "Policy", "slug": Uuid::new_v4().to_string() }),
                &token,
            )
            .await
            .json_value();
        let organization = identifier(&organization["organization"]["id"]);
        let workspace = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({ "name": "Policy workspace", "slug": Uuid::new_v4().to_string() }),
                &token,
            )
            .await
            .json_value();
        let workspace = identifier(&workspace["workspace"]["id"]);
        Self {
            token,
            user,
            organization,
            workspace,
        }
    }

    fn scope(&self, client: &TestClient, offline: bool) -> WorkspaceScope {
        WorkspaceScope {
            state: client.state().clone(),
            workspace_id: self.workspace,
            chat_id: Some(Uuid::new_v4()),
            user_id: self.user,
            offline,
            origin: Origin::Member,
        }
    }

    async fn chat(&self, client: &TestClient, policy: Option<Arc<Effective>>) -> ChatTools {
        ChatTools::build_with(self.scope(client, false), policy).await
    }

    /// Store `server` and wait until the organization's hub has connected it.
    async fn serve(&self, client: &TestClient, server: Value) {
        let servers = format!("/api/organizations/{}/mcp-servers", self.organization);
        let response = client.post_json_auth(&servers, &server, &self.token).await;
        response.assert_status(StatusCode::CREATED);
        let id = response.json_value()["id"]
            .as_str()
            .unwrap_or_else(|| panic!("a stored server has an id: {server}"))
            .to_string();
        let uri = format!("{servers}/{id}");
        let deadline = Instant::now() + SETTLE;
        loop {
            let stored = client.get_auth(&uri, &self.token).await.json_value();
            if stored["status"] == CONNECTED {
                return;
            }
            assert_ne!(stored["status"], FAILED, "{stored}");
            assert!(Instant::now() < deadline, "never connected: {stored}");
            tokio::time::sleep(POLL).await;
        }
    }

    async fn discard(&self, client: &TestClient) {
        common::discard(client.state().db(), self.workspace, &[self.user]).await;
    }
}

fn identifier(value: &Value) -> Uuid {
    value
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("an id: {value}"))
}

fn http(name: &str, host: &str, stub: &Stub) -> Value {
    json!({
        "name": name,
        "transport": "http",
        "url": format!("http://{host}:{}{PATH}", stub.port()),
    })
}

fn stdio(name: &str) -> Value {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_mcp_server.py");
    json!({
        "name": name,
        "transport": "stdio",
        "command": "python3",
        "arguments": [script.to_string_lossy()],
    })
}

fn offered(tools: &ChatTools) -> Vec<&str> {
    SERVERS
        .into_iter()
        .filter(|server| tools.has(&tool(server)))
        .collect()
}

#[tokio::test]
async fn a_restricted_network_keeps_only_an_http_server_at_an_admitted_host() {
    let _environment = isolated();
    let stub = Stub::start(&[]).await;
    let client = client().await;
    let tenant = Tenant::new(&client).await;
    tenant.serve(&client, http(ADMITTED, LOOPBACK, &stub)).await;
    tenant.serve(&client, http(FOREIGN, LOCALHOST, &stub)).await;
    tenant.serve(&client, stdio(LOCAL)).await;

    let unrestricted = tenant.chat(&client, None).await;
    let restricted = tenant.chat(&client, loopback()).await;
    let run = ChatTools::for_task_with(
        client.state(),
        std::env::temp_dir(),
        tenant.workspace,
        Some(tenant.user),
        loopback(),
    )
    .await;
    let offline = ChatTools::build_with(tenant.scope(&client, true), loopback()).await;
    tenant.discard(&client).await;

    assert_eq!(
        offered(&unrestricted),
        SERVERS,
        "{:?}",
        unrestricted.names()
    );
    assert!(!unrestricted.offline());
    for (holder, tools) in [("chat", &restricted), ("run", &run)] {
        assert!(tools.offline(), "the {holder} is not held offline");
        assert_eq!(offered(tools), [ADMITTED], "{holder}: {:?}", tools.names());
        assert!(tools.mcp_guidance().is_some(), "{holder}");
    }
    assert!(offline.offline());
    assert!(
        offered(&offline).is_empty(),
        "a chat offline on its own is readmitted a server: {:?}",
        offline.names()
    );
    assert!(!offline.has(FETCH_URL));
}

#[tokio::test]
async fn a_server_list_withholds_every_server_it_leaves_out_with_the_network_open() {
    let _environment = isolated();
    let stub = Stub::start(&[]).await;
    let client = client().await;
    let tenant = Tenant::new(&client).await;
    tenant.serve(&client, http(ADMITTED, LOOPBACK, &stub)).await;
    tenant.serve(&client, stdio(LOCAL)).await;

    let named = tenant.chat(&client, policy(Some(&[LOCAL]), None)).await;
    let nothing = tenant.chat(&client, policy(Some(&[]), None)).await;
    tenant.discard(&client).await;

    assert!(!named.offline());
    assert_eq!(offered(&named), [LOCAL], "{:?}", named.names());
    assert!(named.has(WEB_SEARCH) && named.has(GITHUB_TOOL));
    assert!(offered(&nothing).is_empty(), "{:?}", nothing.names());
    assert_eq!(nothing.mcp_guidance(), None);
}

#[tokio::test]
async fn a_restricted_network_withholds_web_search_and_the_github_tools() {
    let client = client().await;
    let tenant = Tenant::new(&client).await;

    let unrestricted = tenant.chat(&client, None).await;
    let restricted = tenant.chat(&client, loopback()).await;
    tenant.discard(&client).await;

    for name in [WEB_SEARCH, FETCH_URL, GITHUB_TOOL] {
        assert!(unrestricted.has(name), "{name}: {:?}", unrestricted.names());
    }
    assert!(!unrestricted.offline());
    assert!(restricted.offline());
    assert!(restricted.has(FETCH_URL), "{:?}", restricted.names());
    for name in [WEB_SEARCH, GITHUB_TOOL] {
        assert!(!restricted.has(name), "{name} reaches the network");
    }
}

#[tokio::test]
async fn wait_for_refuses_a_network_kind_under_a_restricted_network() {
    let client = client().await;
    let tenant = Tenant::new(&client).await;

    let unrestricted = tenant.chat(&client, None).await;
    let restricted = tenant.chat(&client, loopback()).await;
    let waited = restricted
        .execute(
            WAIT_FOR,
            &json!({ "kind": KIND_CHECK, "id": Uuid::new_v4().to_string() }).to_string(),
        )
        .await;
    tenant.discard(&client).await;

    let described = |tools: &ChatTools| {
        tools
            .all_definitions()
            .iter()
            .find(|definition| definition.function.name == WAIT_FOR)
            .map(|definition| definition.function.description.clone())
            .unwrap_or_else(|| panic!("{WAIT_FOR} is offered: {:?}", tools.names()))
    };
    assert!(described(&unrestricted).contains("checks"));
    assert!(!described(&restricted).contains("checks"));
    assert!(!waited.success, "{waited:?}");
    assert_eq!(waited.error.as_deref(), Some(CHAT_OFFLINE));
}

#[tokio::test]
async fn list_models_browses_only_offline_sources_under_a_restricted_network() {
    let client = client().await;
    let tenant = Tenant::new(&client).await;

    let unrestricted = tenant.chat(&client, None).await;
    let restricted = tenant.chat(&client, loopback()).await;
    tenant.discard(&client).await;

    let sources = |tools: &ChatTools| -> Vec<String> {
        let definition = tools
            .all_definitions()
            .iter()
            .find(|definition| definition.function.name == LIST_MODELS)
            .unwrap_or_else(|| panic!("{LIST_MODELS} is offered: {:?}", tools.names()))
            .clone();
        serde_json::from_value(
            definition.function.parameters["properties"]["source"]["enum"].clone(),
        )
        .expect("the sources are a list")
    };
    assert!(sources(&unrestricted).len() > OFFLINE_SOURCES.len());
    assert_eq!(sources(&restricted), OFFLINE_SOURCES);
}

#[tokio::test]
async fn a_run_with_no_writer_is_held_offline_by_a_restricted_network() {
    let client = client().await;
    let workspace = Uuid::new_v4();

    let unrestricted =
        ChatTools::for_task(client.state(), std::env::temp_dir(), workspace, None).await;
    let restricted = ChatTools::for_task_with(
        client.state(),
        std::env::temp_dir(),
        workspace,
        None,
        loopback(),
    )
    .await;
    let unpolicied =
        ChatTools::for_task_with(client.state(), std::env::temp_dir(), workspace, None, None).await;

    assert!(!unrestricted.offline());
    assert!(!unpolicied.offline());
    assert_eq!(unpolicied.names(), unrestricted.names());
    assert!(restricted.offline());
    let curled = restricted
        .execute(
            "run_command",
            &json!({ "command": "curl", "args": ["https://example.com"] }).to_string(),
        )
        .await;
    assert!(!curled.success, "{curled:?}");
    assert!(
        curled
            .error
            .as_deref()
            .is_some_and(|error| error.contains(CHAT_OFFLINE)),
        "{curled:?}"
    );
}

#[tokio::test]
async fn no_policy_changes_nothing() {
    let client = client().await;
    let tenant = Tenant::new(&client).await;

    let built = ChatTools::build(tenant.scope(&client, false)).await;
    let unpolicied = tenant.chat(&client, None).await;
    let open = tenant
        .chat(
            &client,
            Some(Arc::new(Effective::new(Rules::default(), Rules::default()))),
        )
        .await;
    tenant.discard(&client).await;

    for tools in [&unpolicied, &open] {
        assert_eq!(tools.names(), built.names());
        assert_eq!(tools.offline(), built.offline());
    }
}
