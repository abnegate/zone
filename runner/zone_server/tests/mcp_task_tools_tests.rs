//! A task run is handed the attached MCP tools, the way a chat turn is.
//!
//! The live pass connected magents and attached its 22 tools to every chat
//! turn, and a task told to hand work to another agent searched its catalog
//! six times without finding one: `for_task` assembled with the servers
//! left out. This attaches a stub stdio server and asks the run's catalog.
mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;
use zone_core::mcp::{McpConfig, McpHub, McpServerSpec};
use zone_server::agent::ChatTools;
use zone_server::db::workspace_members;
use zone_server::state::AppState;

fn stub_server() -> McpServerSpec {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_mcp_server.py");
    McpServerSpec {
        name: "stub".to_string(),
        command: "python3".to_string(),
        args: vec![script.to_string_lossy().into_owned()],
        env: HashMap::new(),
        cwd: None,
        disabled: false,
    }
}

#[tokio::test]
async fn a_task_run_is_handed_the_attached_mcp_tools() {
    let pool = common::create_test_pool().await;
    let (organization, workspace, user) = common::setup_test_data(&pool).await;
    workspace_members::add_member(
        &pool,
        workspace,
        user,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .unwrap();

    let hub = McpHub::connect(&McpConfig {
        servers: vec![stub_server()],
    })
    .await;
    assert_eq!(hub.server_count(), 1, "the stub server has to connect");
    let state = AppState::new(common::test_config(), pool.clone(), None);
    state.install_mcp(hub);

    let tools = ChatTools::for_task(&state, std::env::temp_dir(), workspace, Some(user)).await;

    assert!(
        tools.has("stub_delegate"),
        "the run's catalog lacks the server's tool: {:?}",
        tools.names()
    );
    let listed = tools
        .deferred()
        .into_iter()
        .find(|listed| listed.name == "stub_delegate")
        .expect("an MCP tool is listed for the run to load");
    assert!(
        listed.remote,
        "the catalog marks the server's own text as remote"
    );
    assert!(
        listed.purpose.contains("another coding agent"),
        "{}",
        listed.purpose
    );

    // The chat profile is unchanged by the fix: the same hub reaches it.
    let chat = ChatTools::build(zone_server::agent::tools::WorkspaceScope {
        state: state.clone(),
        workspace_id: workspace,
        user_id: user,
        chat_id: Some(Uuid::new_v4()),
    })
    .await;
    assert!(chat.has("stub_delegate"));

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
}

/// Without a writer there is no workspace scope, and MCP servers attach only
/// with one: an unattributed run gets the sandbox and nothing that reaches
/// out of it.
#[tokio::test]
async fn an_unattributed_run_gets_no_mcp_tools() {
    let pool = common::create_test_pool().await;
    let hub = McpHub::connect(&McpConfig {
        servers: vec![stub_server()],
    })
    .await;
    assert_eq!(hub.server_count(), 1);
    let state = AppState::new(common::test_config(), pool, None);
    state.install_mcp(hub);

    let tools = ChatTools::for_task(&state, std::env::temp_dir(), Uuid::new_v4(), None).await;

    assert!(!tools.has("stub_delegate"), "{:?}", tools.names());
}
