//! Turns to serve, for the tests of both halves of this module.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use uuid::Uuid;

use super::endpoint::PATH;
use super::turn::{Lease, Turn};
use crate::agent::{AgentEvent, ApprovalPolicy, ChatTools, WorkspaceScope};
use crate::state::AppState;

pub(crate) fn state() -> AppState {
    let db = sqlx::PgPool::connect_lazy("postgres://localhost/test")
        .expect("a lazy pool needs no server");
    let state = AppState::new(crate::state::test_config(), db, None);
    state.disable_mcp();
    state
}

pub(crate) async fn tools(chat: Uuid) -> Arc<ChatTools> {
    Arc::new(
        ChatTools::build(WorkspaceScope {
            state: state(),
            workspace_id: Uuid::new_v4(),
            chat_id: Some(chat),
            user_id: Uuid::new_v4(),
        })
        .await,
    )
}

pub(crate) struct Opened {
    pub lease: Lease,
    pub token: String,
    pub workspace: Uuid,
    pub chat: Uuid,
    pub user: Uuid,
    pub approval: ApprovalPolicy,
    pub events: UnboundedReceiver<AgentEvent>,
    pub tools: Arc<ChatTools>,
}

pub(crate) async fn open(approval: ApprovalPolicy) -> Opened {
    let workspace = Uuid::new_v4();
    let chat = Uuid::new_v4();
    let user = Uuid::new_v4();
    let tools = tools(chat).await;
    let (sender, events) = unbounded_channel();
    let lease = Turn::new(
        workspace,
        chat,
        user,
        Arc::clone(&tools),
        approval.clone(),
        sender,
    )
    .open(super::endpoint::endpoint("http://127.0.0.1:8080"));
    let token = lease.toolset().token.expose().to_string();
    assert!(lease.toolset().endpoint.ends_with(PATH));
    Opened {
        lease,
        token,
        workspace,
        chat,
        user,
        approval,
        events,
        tools,
    }
}

/// How long a test waits for a card before deciding none is coming. Bounded so
/// a gate that stops raising them fails the test rather than hanging it.
const CARD_TIMEOUT: Duration = Duration::from_secs(10);

/// The card a confirmed call raises: its id, and what it tells the reader.
pub(crate) struct Card {
    pub id: String,
    pub name: String,
    pub reason: Option<String>,
    pub preview: Option<String>,
}

pub(crate) async fn next_card(events: &mut UnboundedReceiver<AgentEvent>) -> Card {
    tokio::time::timeout(CARD_TIMEOUT, async {
        loop {
            match events.recv().await.expect("the console hears the turn") {
                AgentEvent::ToolApprovalRequired {
                    id,
                    name,
                    reason,
                    preview,
                    ..
                } => {
                    return Card {
                        id,
                        name,
                        reason,
                        preview,
                    };
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("a confirmed call must raise a card")
}

pub(crate) async fn wait_for_card(events: &mut UnboundedReceiver<AgentEvent>) -> String {
    next_card(events).await.id
}

pub(crate) fn drain(events: &mut UnboundedReceiver<AgentEvent>) -> Vec<&'static str> {
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        seen.push(match event {
            AgentEvent::ToolCallStarted { .. } => "started",
            AgentEvent::ToolApprovalRequired { .. } => "approval",
            AgentEvent::ToolCallCompleted { .. } => "completed",
            _ => "other",
        });
    }
    seen
}

/// A `write_file` call's arguments, stating a reason as the catalog requires.
pub(crate) fn write(path: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "path": path.to_string_lossy(),
        "content": "written",
        "reason": "The turn asked for this file.",
    })
}
