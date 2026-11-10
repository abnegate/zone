//! An organization's MCP server reaches every provider a chat can run on.
//!
//! An HTTP backend is handed the server's tools in the catalog zone's own loop
//! calls from. A coding agent runs its own loop and reaches them only through
//! zone's `/mcp` endpoint, so each agent kind is served the turn's tools there
//! and asked for its catalog the way the spawned agent asks for it.
use crate::common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use abnegate_agent_cli::AgentKind;
use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uuid::Uuid;
use zone_core::llm::{BuiltinTools, LlmBackend, cli};
use zone_core::variables::Variables;
use zone_server::agent::{ApprovalGate, ApprovalPolicy, ChatTools, LoopBudget, WorkspaceScope};
use zone_server::config::Config;
use zone_server::db::mcp_servers::{self, NewMcpServer};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::tasks::Origin;
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::mcp::{AgentTools, Scope, endpoint, offered};
use zone_server::services::mcp_servers::transport::Transport;
use zone_server::state::AppState;

const SERVER: &str = "stub";
const TOOL: &str = "stub__delegate";
const PYTHON: &str = "python3";
const TOOLS_LIST: &str = "tools/list";

const ENABLED: &str = "ZONE_MCP_ENABLED";
const SERVERS: &str = "ZONE_MCP_SERVERS";
const CONFIGURATION: &str = "ZONE_MCP_CONFIG";
const AUTO_CONNECT: &str = "ZONE_MCP_AUTO_CONNECT";
const AUTO_MAGENTS: &str = "ZONE_MCP_AUTO_MAGENTS";

const AGENTS: [AgentKind; 2] = [AgentKind::Claude, AgentKind::Codex];

/// No instance server reaches the test from the host: not `magents` on the PATH, nor a
/// `~/.zone/mcp.json`, nor the environment the test binary was started with.
fn isolated() -> Variables {
    let mut variables =
        Variables::isolated(&[ENABLED, SERVERS, CONFIGURATION, AUTO_CONNECT, AUTO_MAGENTS]);
    variables.set(AUTO_CONNECT, "false");
    variables
}

fn stub() -> NewMcpServer {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_mcp_server.py");
    NewMcpServer {
        name: SERVER.to_string(),
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
    for agent in AGENTS {
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
    let pool = common::create_test_pool().await;
    let (organization, workspace, user) = common::setup_test_data(&pool).await;
    organization_members::add_member(&pool, organization, user, OrgRole::Admin, None)
        .await
        .expect("an organization admin");
    workspace_members::add_member(&pool, workspace, user, WorkspaceRole::Member, None)
        .await
        .expect("a workspace member");
    let state = AppState::new(
        Config {
            mcp_commands: true,
            ..common::test_config()
        },
        pool.clone(),
        None,
    );
    state.disable_mcp();
    mcp_servers::create_authorized(&pool, state.master_key(), organization, user, stub())
        .await
        .expect("the organization's stub server is stored");

    assert_parity(&state, workspace, user, TOOL).await;

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(&pool)
        .await
        .expect("the organization is removed");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .expect("the user is removed");
}
