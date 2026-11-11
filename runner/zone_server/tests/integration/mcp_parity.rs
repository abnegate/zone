//! An organization's MCP server reaches every provider a chat can run on,
//! whether zone starts it as a command or reaches it over streamable HTTP.
//!
//! An HTTP backend is handed the server's tools in the catalog zone's own loop
//! calls from. A coding agent runs its own loop and reaches them only through
//! zone's `/mcp` endpoint, so each agent kind is served the turn's tools there
//! and asked for its catalog the way the spawned agent asks for it.
use std::collections::BTreeMap;
use std::path::PathBuf;

use abnegate_agent::mcp::McpConfig;
use abnegate_agent::mcp::McpHub;
use abnegate_agent::mcp::McpServer;
use abnegate_agent_cli::AgentKind;
use axum::http::StatusCode;
use serde_json::Value;
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;
use zone_core::llm::BuiltinTools;
use zone_core::llm::LlmBackend;
use zone_core::llm::cli;
use zone_server::agent::ApprovalGate;
use zone_server::agent::ApprovalPolicy;
use zone_server::agent::ChatTools;
use zone_server::agent::LoopBudget;
use zone_server::agent::WorkspaceScope;
use zone_server::config::Config;
use zone_server::db::mcp_servers;
use zone_server::db::mcp_servers::McpServerChanges;
use zone_server::db::mcp_servers::NewMcpServer;
use zone_server::db::organization_members;
use zone_server::db::organization_members::OrgRole;
use zone_server::db::tasks::Origin;
use zone_server::db::workspace_members;
use zone_server::db::workspace_members::WorkspaceRole;
use zone_server::mcp::AgentTools;
use zone_server::mcp::Scope;
use zone_server::mcp::endpoint;
use zone_server::mcp::offered;
use zone_server::services::mcp_servers::stub::Stub;
use zone_server::services::mcp_servers::transport::Transport;
use zone_server::state::AppState;

use crate::common;
use crate::common::mcp::isolated;

const STDIO_SERVER: &str = "stub";
const STDIO_TOOL: &str = "stub__delegate";
const HTTP_SERVER: &str = "stub_http";
const HTTP_TOOL: &str = "stub_http__delegate";
const COLLIDED_TOOL: &str = "stub_http__delegate_2";
const OFFERED: &str = "delegate";
const UNOFFERED: &str = "other";
const PYTHON: &str = "python3";
const TOOLS_LIST: &str = "tools/list";

fn stdio() -> NewMcpServer {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_mcp_server.py");
    NewMcpServer {
        name: STDIO_SERVER.to_string(),
        transport: Transport::Stdio.as_str().to_string(),
        command: Some(PYTHON.to_string()),
        arguments: vec![script.to_string_lossy().into_owned()],
        url: None,
        tools: None,
        enabled: true,
        environment: BTreeMap::new(),
        headers: BTreeMap::new(),
    }
}

fn http(stub: &Stub) -> NewMcpServer {
    NewMcpServer {
        name: HTTP_SERVER.to_string(),
        transport: Transport::Http.as_str().to_string(),
        command: None,
        arguments: Vec::new(),
        url: Some(stub.url()),
        tools: None,
        enabled: true,
        environment: BTreeMap::new(),
        headers: BTreeMap::new(),
    }
}

/// An organization whose one user administers it and holds `role` in its one workspace.
struct Tenant {
    state: AppState,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
}

impl Tenant {
    /// A tenant of a server with no instance MCP server of its own.
    async fn new(commands: bool, role: WorkspaceRole) -> Self {
        Self::beside(McpHub::new(), commands, role).await
    }

    /// A tenant of a server whose instance MCP servers are the ones `instance` connected.
    async fn beside(instance: McpHub, commands: bool, role: WorkspaceRole) -> Self {
        let pool = common::create_test_pool().await;
        let (organization, workspace, user) = common::setup_test_data(&pool).await;
        organization_members::add_member(&pool, organization, user, OrgRole::Admin, None)
            .await
            .expect("an organization admin");
        workspace_members::add_member(&pool, workspace, user, role, None)
            .await
            .expect("a workspace member");
        let state = AppState::new(
            Config {
                mcp_commands: commands,
                ..common::test_config()
            },
            pool,
            None,
        );
        state.install_mcp(instance);
        Self {
            state,
            organization,
            workspace,
            user,
        }
    }

    async fn store(&self, server: NewMcpServer) -> Uuid {
        mcp_servers::create_authorized(
            self.state.db(),
            self.state.master_key(),
            self.organization,
            self.user,
            server,
        )
        .await
        .expect("the organization's stub server is stored")
        .id
    }

    async fn allow(&self, server: Uuid, tools: &[&str]) {
        let changes = McpServerChanges {
            tools: Some(Some(tools.iter().map(|tool| tool.to_string()).collect())),
            ..McpServerChanges::default()
        };
        mcp_servers::update_authorized(
            self.state.db(),
            self.state.master_key(),
            self.organization,
            server,
            self.user,
            changes,
        )
        .await
        .expect("the server's allowlist is stored");
    }

    async fn switch(&self, server: Uuid, enabled: Option<bool>) {
        mcp_servers::set_workspace_overrides_authorized(
            self.state.db(),
            self.organization,
            self.workspace,
            self.user,
            BTreeMap::from([(server, enabled)]),
        )
        .await
        .expect("the workspace's override is stored");
    }

    /// Whether a chat in the workspace is offered `tool` on its next turn.
    async fn offers(&self, tool: &str) -> bool {
        ChatTools::build(scope(&self.state, self.workspace, self.user))
            .await
            .has(tool)
    }

    async fn reaches_every_provider(&self, server: NewMcpServer, tool: &str) {
        self.store(server).await;
        assert_parity(&self.state, self.workspace, self.user, tool).await;
    }

    async fn discard(self) {
        let pool = self.state.db();
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(pool)
            .await
            .expect("the organization is removed");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(self.user)
            .execute(pool)
            .await
            .expect("the user is removed");
    }
}

fn scope(state: &AppState, workspace: Uuid, user: Uuid) -> WorkspaceScope {
    WorkspaceScope {
        state: state.clone(),
        workspace_id: workspace,
        chat_id: Some(Uuid::new_v4()),
        user_id: user,
        offline: false,
        origin: Origin::Member,
    }
}

/// Zone's own router, serving `/mcp` at the address returned.
async fn served(state: &AppState) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port for zone");
    let address = listener.local_addr().expect("zone's address").to_string();
    let router = common::create_test_router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("zone serves");
    });
    (format!("http://{address}"), server)
}

async fn listed(endpoint: &str, token: &str) -> Vec<String> {
    let response = reqwest::Client::new()
        .post(endpoint)
        .bearer_auth(token)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": TOOLS_LIST, "params": {}}))
        .send()
        .await
        .expect("zone's MCP endpoint answers");
    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
    let answer: Value = response.json().await.expect("a JSON-RPC answer");
    answer["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("a tool catalog: {answer}"))
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

/// `tool` is offered to a chat in `workspace` by `user` on every provider: in an HTTP backend's
/// catalog, and over zone's `/mcp` endpoint to each coding agent.
async fn assert_parity(state: &AppState, workspace: Uuid, user: Uuid, tool: &str) {
    let catalog = ChatTools::build(scope(state, workspace, user)).await;
    assert!(
        catalog.has(tool),
        "the chat's catalog lacks {tool}: {:?}",
        catalog.names()
    );
    let http = offered(&LlmBackend::Http, catalog);
    assert!(
        http.has(tool),
        "an HTTP backend's catalog lacks {tool}: {:?}",
        http.names()
    );

    let (base, server) = served(state).await;
    let address = endpoint(&base);
    for agent in AgentKind::ALL.iter().copied() {
        let backend = LlmBackend::cli(agent, cli::settings());
        let mut tools = offered(
            &backend,
            ChatTools::build(scope(state, workspace, user)).await,
        );
        let served = AgentTools::serve(
            &backend,
            &mut tools,
            Scope {
                workspace,
                chat: Uuid::new_v4(),
                user: Some(user),
                approval: ApprovalPolicy::required(ApprovalGate::new()),
                calls: LoopBudget::chat().max_tool_calls,
                builtin: BuiltinTools::Withheld,
            },
            &address,
        )
        .expect("a coding agent's turn is served over MCP");
        let toolset = served.lease.toolset();
        assert!(
            toolset.tools.iter().any(|name| name == tool),
            "{agent:?}'s allowlist lacks {tool}: {:?}",
            toolset.tools
        );
        let names = listed(&toolset.endpoint, toolset.token.expose()).await;
        assert!(
            names.iter().any(|name| name == tool),
            "{agent:?}'s tools/list over zone's /mcp lacks {tool}: {names:?}"
        );
    }
    server.abort();
}

#[tokio::test]
async fn an_organization_stdio_server_reaches_every_provider() {
    let _variables = isolated();
    let tenant = Tenant::new(true, WorkspaceRole::Member).await;

    tenant.reaches_every_provider(stdio(), STDIO_TOOL).await;

    tenant.discard().await;
}

#[tokio::test]
async fn an_organization_http_server_reaches_every_provider() {
    let _variables = isolated();
    let stub = Stub::start(&[]).await;
    let tenant = Tenant::new(false, WorkspaceRole::Member).await;

    tenant.reaches_every_provider(http(&stub), HTTP_TOOL).await;

    tenant.discard().await;
}

#[tokio::test]
async fn an_allowlisted_tool_whose_name_an_instance_server_took_reaches_every_provider() {
    let _variables = isolated();
    let stub = Stub::start(&[]).await;
    let instance = McpHub::connect(
        &McpConfig::default().with_server(HTTP_SERVER, McpServer::remote(stub.url())),
    )
    .await;
    assert_eq!(instance.server_count(), 1, "the stub server has to connect");
    let tenant = Tenant::beside(instance, false, WorkspaceRole::Member).await;
    let server = tenant.store(http(&stub)).await;

    tenant.allow(server, &[OFFERED]).await;
    assert_parity(&tenant.state, tenant.workspace, tenant.user, COLLIDED_TOOL).await;
    let instance = tenant.offers(HTTP_TOOL).await;
    tenant.allow(server, &[UNOFFERED]).await;
    let hidden = tenant.offers(COLLIDED_TOOL).await;

    assert!(instance, "the instance's own tool lost its name");
    assert!(
        !hidden,
        "an allowlist naming another tool still offers the organization's"
    );

    tenant.discard().await;
}

#[tokio::test]
async fn an_http_server_obeys_its_allowlist_and_its_workspace_override() {
    let _variables = isolated();
    let stub = Stub::start(&[]).await;
    let tenant = Tenant::new(false, WorkspaceRole::Admin).await;
    let server = tenant.store(http(&stub)).await;
    let unlisted = tenant.offers(HTTP_TOOL).await;

    tenant.allow(server, &[UNOFFERED]).await;
    let hidden = tenant.offers(HTTP_TOOL).await;
    tenant.allow(server, &[OFFERED]).await;
    let allowed = tenant.offers(HTTP_TOOL).await;
    tenant.switch(server, Some(false)).await;
    let overridden = tenant.offers(HTTP_TOOL).await;
    tenant.switch(server, None).await;
    let restored = tenant.offers(HTTP_TOOL).await;

    assert!(unlisted, "a server with no allowlist withholds its tool");
    assert!(!hidden, "an allowlist naming another tool still offers it");
    assert!(allowed, "an allowlist naming the tool hides it");
    assert!(
        !overridden,
        "a workspace that switched the server off is still offered its tool"
    );
    assert!(restored, "a cleared override keeps the tool hidden");

    tenant.discard().await;
}
