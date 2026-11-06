//! Zone's MCP endpoint against the agent that will actually call it.
//!
//! Everything else about this endpoint is tested against a request this
//! repository wrote. This is the one test that runs the real `claude` binary
//! the way a chat turn does, which is the only thing that settles whether
//! zone's tools reach the agent. It settles it on zone's side: the endpoint is
//! served through a recorder, and the test asserts the agent opened the
//! handshake and was answered a catalog holding every tool of the turn,
//! whatever the model then writes.

use crate::common::context::Harness;
use abnegate_agent_cli::AgentKind;
use abnegate_llm::{LlmConfig, Message};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::post;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use uuid::Uuid;
use zone_core::llm::{BuiltinTools, LlmBackend, LlmClient, cli};
use zone_server::agent::{ApprovalGate, ApprovalPolicy, ChatTools, LoopBudget, WorkspaceScope};
use zone_server::mcp::{BODY_LIMIT, Lease, PATH, Scope, Turn, endpoint, serve};
use zone_server::state::AppState;

/// How long the agent has to attach zone's server and answer one short turn.
const TURN_TIMEOUT: Duration = Duration::from_secs(180);

/// The shortest turn there is: attaching zone's server is all the test needs of it.
const PROMPT: &str = "Reply with only the word OK.";

const INITIALIZE: &str = "initialize";
const TOOLS_LIST: &str = "tools/list";

type Console = tokio::sync::mpsc::UnboundedReceiver<zone_server::agent::AgentEvent>;

/// What zone's endpoint was asked during the turn, and the tools it answered `tools/list` with.
#[derive(Default)]
struct Recorded {
    methods: Vec<String>,
    listed: Vec<String>,
}

type Recorder = Arc<Mutex<Recorded>>;

/// Zone's own endpoint, with the method of every request and the name of every listed tool
/// recorded on the way through.
async fn recording(State(recorder): State<Recorder>, headers: HeaderMap, body: Bytes) -> Response {
    let method = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|request| request.get("method")?.as_str().map(str::to_string));
    let response = serve(headers, body).await;
    let Some(method) = method else {
        return response;
    };
    if method != TOOLS_LIST {
        recorder.lock().expect("the recorder").methods.push(method);
        return response;
    }
    let (parts, body) = response.into_parts();
    let answer = axum::body::to_bytes(body, BODY_LIMIT)
        .await
        .expect("zone's answer to tools/list");
    let listed = serde_json::from_slice::<Value>(&answer)
        .ok()
        .and_then(|answer| {
            answer
                .pointer("/result/tools")?
                .as_array()?
                .iter()
                .map(|tool| tool.get("name")?.as_str().map(str::to_string))
                .collect::<Option<Vec<String>>>()
        })
        .unwrap_or_default();
    let mut recorded = recorder.lock().expect("the recorder");
    recorded.methods.push(method);
    recorded.listed.extend(listed);
    drop(recorded);
    Response::from_parts(parts, Body::from(answer))
}

/// Zone's endpoint behind a recorder, and the address it is served at.
async fn recorded() -> (String, Recorder, tokio::task::JoinHandle<()>) {
    let recorder = Recorder::default();
    let router = Router::new()
        .route(
            PATH,
            post(recording).layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .with_state(Arc::clone(&recorder));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port for the recorder");
    let address = listener
        .local_addr()
        .expect("the recorder's address")
        .to_string();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the recorder serves");
    });
    (address, recorder, server)
}

async fn lease(harness: &Harness, address: &str) -> (Lease, Console) {
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
    (turn.open(endpoint(&format!("http://{address}"))), received)
}

#[tokio::test]
#[ignore = "requires the host's claude CLI and a migrated DATABASE_URL"]
async fn the_host_agent_connects_to_zone_and_is_shown_zones_tools() {
    let harness = Harness::new(None, true, vec![]).await;
    let (address, recorder, server) = recorded().await;
    let (lease, _events) = lease(&harness, &address).await;
    let toolset = lease.toolset();
    let client = LlmClient::new(LlmConfig::default())
        .with_backend(LlmBackend::cli(AgentKind::Claude, cli::settings()))
        .with_toolset(toolset.clone(), BuiltinTools::Withheld);

    tokio::time::timeout(TURN_TIMEOUT, client.chat(&[Message::user(PROMPT)], None))
        .await
        .expect("the agent answers in time")
        .expect("the agent attaches zone's required server and answers");
    server.abort();

    let recorded = recorder.lock().expect("the recorder");
    for method in [INITIALIZE, TOOLS_LIST] {
        assert!(
            recorded.methods.iter().any(|seen| seen == method),
            "the agent never sent zone's endpoint {method}: it sent {:?}",
            recorded.methods
        );
    }
    for tool in &toolset.tools {
        assert!(
            recorded.listed.contains(tool),
            "zone's endpoint did not list {tool} to the agent: it listed {:?}",
            recorded.listed
        );
    }
}
