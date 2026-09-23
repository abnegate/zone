//! Zone's MCP endpoint against the agent that will actually call it.
//!
//! Everything else about this endpoint is tested against a request this
//! repository wrote. This is the one test that asks the real `claude` binary
//! whether the handshake it gets is one it can use, which is the only thing
//! that settles whether zone's tools reach the model.
mod common;

use common::context::Harness;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;
use zone_core::llm::{AgentKind, BuiltinTools, CodexSandbox, Toolset};
use zone_server::agent::{ApprovalGate, ApprovalPolicy, ChatTools, LoopBudget, WorkspaceScope};
use zone_server::mcp::{Lease, Scope, Turn, endpoint};
use zone_server::state::AppState;

/// The agent prints its `system`/`init` line as soon as its servers are up,
/// well before the model answers, so the run is abandoned at that line rather
/// than spending a turn to learn something the handshake already said.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(90);

type Console = tokio::sync::mpsc::UnboundedReceiver<zone_server::agent::AgentEvent>;

async fn lease(harness: &Harness) -> (Lease, Console) {
    let state = AppState::new(harness.config.clone(), harness.pool.clone(), None);
    state.disable_mcp();
    let tools = Arc::new(
        ChatTools::build(WorkspaceScope {
            state,
            workspace_id: harness.workspace,
            chat_id: Some(harness.chat),
            user_id: Uuid::new_v4(),
        })
        .await,
    );
    let (events, received) = tokio::sync::mpsc::unbounded_channel();
    let turn = Turn::new(
        Scope {
            workspace: harness.workspace,
            chat: harness.chat,
            user: Some(Uuid::new_v4()),
            approval: ApprovalPolicy::required(ApprovalGate::new()),
            calls: LoopBudget::chat().max_tool_calls,
        },
        tools,
        events,
    );
    (
        turn.open(endpoint(&format!("http://{}", harness.address))),
        received,
    )
}

#[tokio::test]
#[ignore = "requires the host's claude CLI and a migrated DATABASE_URL"]
async fn the_host_agent_connects_to_zone_and_is_shown_zones_tools() {
    let harness = Harness::new(None, true, vec![]).await;
    let (lease, _events) = lease(&harness).await;
    let toolset = lease.toolset();

    let mut child = Command::new("claude")
        .args(AgentKind::Claude.arguments_with(
            None,
            Some(&toolset),
            BuiltinTools::Withheld,
            CodexSandbox::default(),
        ))
        .env(Toolset::TOKEN_VARIABLE, toolset.token.expose())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the host's claude CLI");

    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(b"Say the word ready and stop.\n")
        .await
        .expect("the agent reads its prompt from stdin");
    drop(stdin);

    let mut errors = BufReader::new(child.stderr.take().expect("stderr")).lines();
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
    let init = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if event["type"] == "system" && event["subtype"] == "init" {
                return Some(event);
            }
        }
        None
    })
    .await
    .expect("the agent reports its servers before it answers");
    let init = match init {
        Some(init) => init,
        None => {
            let mut said = String::new();
            while let Ok(Some(line)) = errors.next_line().await {
                said.push_str(&line);
                said.push('\n');
            }
            panic!("the agent printed no init line: {said}");
        }
    };
    let _ = child.kill().await;

    let servers = init["mcp_servers"]
        .as_array()
        .expect("the init line lists the servers the agent connected to");
    let zone = servers
        .iter()
        .find(|server| server["name"] == Toolset::SERVER)
        .unwrap_or_else(|| panic!("zone is not among {servers:?}"));
    assert_eq!(zone["status"], "connected", "{init}");

    let offered: Vec<&str> = init["tools"]
        .as_array()
        .expect("the init line lists the tools the model was shown")
        .iter()
        .filter_map(|tool| tool.as_str())
        .filter(|tool| tool.starts_with("mcp__"))
        .collect();
    for tool in &toolset.tools {
        let qualified = format!("mcp__{}__{tool}", Toolset::SERVER);
        assert!(
            offered.contains(&qualified.as_str()),
            "{qualified} is not among {offered:?}"
        );
    }
}
