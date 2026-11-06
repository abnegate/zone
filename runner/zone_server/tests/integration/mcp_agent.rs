//! Zone's MCP endpoint against the agent that will actually call it.
//!
//! Everything else about this endpoint is tested against a request this
//! repository wrote. This is the one test that runs the real `claude` binary
//! the way a chat turn does, which is the only thing that settles whether
//! zone's tools reach the model.

use crate::common::context::Harness;
use abnegate_agent_cli::AgentKind;
use abnegate_llm::{LlmConfig, Message};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use zone_core::llm::{BuiltinTools, LlmBackend, LlmClient, Toolset, cli};
use zone_server::agent::{ApprovalGate, ApprovalPolicy, ChatTools, LoopBudget, WorkspaceScope};
use zone_server::mcp::{Lease, Scope, Turn, endpoint};
use zone_server::state::AppState;

/// How long the agent has to attach zone's server and answer one short turn.
const TURN_TIMEOUT: Duration = Duration::from_secs(180);

/// What the model is asked, so its answer names every tool it was shown.
const LIST_TOOLS: &str = "List the full name of every tool you have whose name starts with \
     mcp__zone__, one per line, and nothing else.";

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
            offline: false,
            origin: zone_server::db::tasks::Origin::Member,
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
            builtin: BuiltinTools::Withheld,
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
    let client = LlmClient::new(LlmConfig::default())
        .with_backend(LlmBackend::cli(AgentKind::Claude, cli::settings()))
        .with_toolset(toolset.clone(), BuiltinTools::Withheld);

    let response = tokio::time::timeout(
        TURN_TIMEOUT,
        client.chat(&[Message::user(LIST_TOOLS)], None),
    )
    .await
    .expect("the agent answers in time")
    .expect("the agent attaches zone's required server and answers");
    let answer = response
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_deref())
        .expect("the agent's answer");

    for tool in &toolset.tools {
        let qualified = AgentKind::Claude.tool(Toolset::SERVER, tool);
        assert!(
            answer.contains(&qualified),
            "{qualified} is not among the tools the model was shown: {answer}"
        );
    }
}
