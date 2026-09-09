//! Shared real-router conversation fixture. Every inference is a deterministic mock;
//! metadata discovery and database persistence exercise the production paths.
use super::{
    TestClient, create_test_router, create_test_state, test_config, test_email, test_password,
};
use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as SocketMessage,
};
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path},
};
use zone_core::context::{ContextSource, ContextUsage, Policy};
use zone_server::config::Config;
use zone_server::db::{chats, context::Store};
use zone_server::services::chat::session::{LEASE_LIFETIME, Settings};

pub const MODEL: &str = "context-test";
pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// `read_file` pages at this many characters, so one large read costs exactly
/// one page in the durable entry and in every request that carries it.
pub const PAGE_CHARS: usize = 8_000;

/// `zone_core` estimates one token per four UTF-8 bytes.
const BYTES_PER_TOKEN: u64 = 4;

/// Wide enough that measuring a chat's fixed prefix is never itself budget-bound.
const MEASUREMENT_WINDOW: u64 = 200_000;

/// Tool calls one seeded agent round issues, so history arrives as rounds a
/// real turn could have produced rather than as one transaction no lease can
/// outlive.
const ROUND: usize = 64;

/// The advertised context length, shared with the metadata mocks so a fixture
/// can resize the model between preparations; capacity is resolved per
/// preparation, never cached across turns.
#[derive(Clone)]
struct Window(Arc<AtomicU64>);

impl Window {
    fn new(limit: Option<u64>) -> Self {
        Self(Arc::new(AtomicU64::new(limit.unwrap_or_default())))
    }

    fn get(&self) -> Option<u64> {
        Some(self.0.load(Ordering::SeqCst)).filter(|limit| *limit > 0)
    }

    fn set(&self, limit: u64) {
        self.0.store(limit, Ordering::SeqCst);
    }
}

fn page_tokens() -> u64 {
    u64::try_from(PAGE_CHARS)
        .unwrap_or(u64::MAX)
        .div_ceil(BYTES_PER_TOKEN)
}

/// Invert the production budget: the smallest advertised window whose
/// compaction threshold reaches `budget` tokens.
fn window_for(budget: u64) -> u64 {
    let settings = Settings::from_env().unwrap_or_default();
    let threshold = |limit: u64| {
        Policy {
            limit: Some(limit),
            reserved: settings.reserved(Some(limit)),
            source: ContextSource::Configured,
        }
        .threshold()
        .unwrap_or_default()
    };
    let ceiling = budget
        .saturating_mul(2)
        .saturating_add(u64::from(settings.output));
    let (mut low, mut high) = (1, ceiling.max(2));
    while low < high {
        let middle = low + (high - low) / 2;
        if threshold(middle) < budget {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

#[derive(Clone)]
pub struct Script {
    replies: Arc<Mutex<VecDeque<ResponseTemplate>>>,
    summary: Arc<Mutex<ResponseTemplate>>,
    finalization: Arc<Mutex<Option<ResponseTemplate>>>,
}

impl Script {
    pub fn new(replies: Vec<ResponseTemplate>) -> Self {
        Self {
            replies: Arc::new(Mutex::new(replies.into())),
            summary: Arc::new(Mutex::new(summary_response(state()))),
            finalization: Arc::new(Mutex::new(None)),
        }
    }
    pub fn push(&self, response: ResponseTemplate) {
        self.replies.lock().unwrap().push_back(response);
    }
    pub fn summarize(&self, response: ResponseTemplate) {
        *self.summary.lock().unwrap() = response;
    }
    pub fn finalize(&self, response: ResponseTemplate) {
        *self.finalization.lock().unwrap() = Some(response);
    }
    pub fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        if body["stream"] == true {
            if body.get("tools").is_none_or(Value::is_null)
                && body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["role"] == "tool")
                && let Some(response) = self.finalization.lock().unwrap().clone()
            {
                return response;
            }
            self.replies.lock().unwrap().pop_front().unwrap_or_else(|| {
                ResponseTemplate::new(500).set_body_json(
                    json!({"error":{"message":"Unexpected extra inference in deterministic test"}}),
                )
            })
        } else {
            self.summary.lock().unwrap().clone()
        }
    }
}

pub fn state() -> String {
    json!({"objective":"Continue the actual current request", "constraints":["Preserve corrections"], "corrections":["Use the updated path"], "decisions":[], "completed":["Historical exchanges retained"], "evidence":["Original evidence remains available by canonical reference"], "failed":["Retain failed actions as failures"], "pending":["Answer current user"], "questions":[]}).to_string()
}

pub fn summary_response(content: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"id":"summary", "object":"chat.completion", "created":0,"model":MODEL,"choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":"stop"}]}))
}

pub fn answer(content: &str) -> ResponseTemplate {
    stream(json!({"content":content}))
}

pub fn delta(value: Value) -> ResponseTemplate {
    stream(value)
}

pub fn tool(id: &str, name: &str, arguments: Value) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}})
}

pub fn calls(prose: &str, calls: Vec<Value>) -> ResponseTemplate {
    let calls: Vec<_> = calls
        .into_iter()
        .enumerate()
        .map(|(index, mut call)| {
            call["index"] = index.into();
            call
        })
        .collect();
    stream(json!({"content":prose,"tool_calls":calls}))
}

fn stream(delta: Value) -> ResponseTemplate {
    let reason = if delta["tool_calls"].is_array() {
        "tool_calls"
    } else {
        "stop"
    };
    let content = json!({"model":MODEL,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let finish = json!({"model":MODEL,"choices":[{"index":0,"delta":{},"finish_reason":reason}]});
    let usage = json!({"model":MODEL,"choices":[],"usage":{"prompt_tokens":37,"completion_tokens":9,"total_tokens":46}});
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(format!(
            "data: {content}\n\ndata: {finish}\n\ndata: {usage}\n\ndata: [DONE]\n\n"
        ))
}

pub struct Harness {
    pub pool: PgPool,
    pub client: TestClient,
    pub provider: MockServer,
    pub script: Script,
    pub token: String,
    pub chat: Uuid,
    pub workspace: Uuid,
    pub address: String,
    pub config: Config,
    window: Window,
    directory: PathBuf,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    pub async fn new(limit: Option<u64>, agent: bool, replies: Vec<ResponseTemplate>) -> Self {
        Self::advertising(Window::new(limit), agent, replies).await
    }

    /// A chat whose compaction budget holds its fixed prefix — the rendered
    /// system prompt, the tool schemas and the framing the server adds, all
    /// measured through the real preview endpoint — plus `pages` tool result
    /// pages and half of one more, so the page after those is the one that has
    /// to compact. Every term scales with the prompt, so growing the prompt
    /// moves the window with it instead of invalidating a written-down number.
    pub async fn holding(pages: u64, agent: bool, replies: Vec<ResponseTemplate>) -> Self {
        let harness =
            Self::advertising(Window::new(Some(MEASUREMENT_WINDOW)), agent, replies).await;
        let page = page_tokens();
        let budget = harness.prefix().await + pages * page + page / 2;
        harness.window.set(window_for(budget));
        harness
    }

    async fn advertising(window: Window, agent: bool, replies: Vec<ResponseTemplate>) -> Self {
        let provider = MockServer::start().await;
        let script = Script::new(replies);
        let responder = script.clone();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(move |request: &Request| responder.respond(request))
            .mount(&provider)
            .await;
        let uri = provider.uri();
        let routes = window.clone();
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(move |_: &Request| {
                let models = routes.get().map_or_else(|| json!({"data":[]}), |limit| json!({"data":[{"model_name":MODEL,"litellm_params":{"model":format!("ollama_chat/{MODEL}"),"api_base":uri,"num_ctx":limit},"model_info":{"id":"verified-local-route"}}]}));
                ResponseTemplate::new(200).set_body_json(models)
            })
            .mount(&provider)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models":[]})))
            .mount(&provider)
            .await;
        let native = window.clone();
        Mock::given(method("POST")).and(path("/api/show")).respond_with(move |_: &Request| ResponseTemplate::new(200).set_body_json(json!({"capabilities":["completion","tools","vision"],"model_info":{"general.architecture":"test","test.context_length":native.get().unwrap_or(32768)}}))).mount(&provider).await;
        let database = super::context_database_url();
        let pool = PgPool::connect(&database).await.unwrap();
        let mut config = test_config();
        config.database_url = database;
        config.litellm_host = provider.uri();
        config.ollama_host = provider.uri();
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        let state = create_test_state(config.clone(), pool.clone());
        let router = create_test_router(state);
        let client = TestClient::new(router.clone());
        let (address, server) = serve(router).await;
        let suffix = Uuid::new_v4();
        let registered = client
            .post_json(
                "/api/auth/register",
                &json!({"email":test_email(),"password":test_password()}),
            )
            .await
            .json_value();
        let token = registered["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("registration failed: {registered}"))
            .to_owned();
        let organization = client
            .post_json_auth(
                "/api/organizations",
                &json!({"name":"Context acceptance","slug":format!("context-{suffix}")}),
                &token,
            )
            .await
            .json_value();
        let organization = organization["organization"]["id"].as_str().unwrap();
        let workspace = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({"name":"Acceptance workspace","slug":format!("context-{suffix}")}),
                &token,
            )
            .await
            .json_value();
        let workspace: Uuid = workspace["workspace"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let created = client.post_json_auth("/api/chats", &json!({"workspace_id":workspace,"title":"Context acceptance","model_name":MODEL,"agent_enabled":agent,"automatic_title":false,"auto_approve":true}), &token).await.json_value();
        let chat = created["chat"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("chat creation failed: {created}"))
            .parse()
            .unwrap();
        let directory = std::env::temp_dir().join(format!("zone-context-acceptance-{suffix}"));
        std::fs::create_dir(&directory).unwrap();
        Self {
            pool,
            client,
            provider,
            script,
            token,
            chat,
            workspace,
            address,
            config,
            window,
            directory,
            server,
        }
    }

    /// Every token a request carries before any tool result: the rendered
    /// instructions, the tool schemas and the framing the server adds itself.
    async fn prefix(&self) -> u64 {
        let response = self.preview("", None).await;
        response.assert_status(StatusCode::OK);
        usage(&response.json_value()["context"]).used
    }

    pub async fn connect(&self) -> Socket {
        self.connect_to(&self.address).await
    }

    pub async fn connect_to(&self, address: &str) -> Socket {
        let (mut socket, _) = connect_async(format!("ws://{address}/ws/chats/{}", self.chat))
            .await
            .unwrap();
        send(&mut socket, json!({"type":"auth","token":self.token})).await;
        let initial = next(&mut socket).await;
        assert_eq!(initial["type"], "init", "initial frame: {initial}");
        socket
    }

    pub async fn restart(&mut self) {
        self.server.abort();
        let router = create_test_router(create_test_state(self.config.clone(), self.pool.clone()));
        let (address, server) = serve(router.clone()).await;
        self.address = address;
        self.server = server;
        self.client = TestClient::new(router);
    }

    pub async fn turn(&self, content: &str) -> Vec<Value> {
        let mut socket = self.connect().await;
        send(&mut socket, json!({"type":"send","content":content})).await;
        let frames = finish(&mut socket).await;
        let _ = socket.close(None).await;
        frames
    }

    pub async fn preview(&self, content: &str, metadata: Option<Value>) -> super::TestResponse {
        self.client
            .post_json_auth(
                &format!("/api/chats/{}/context", self.chat),
                &json!({"content":content,"metadata":metadata}),
                &self.token,
            )
            .await
    }

    pub async fn seed(&self, role: &str, content: &str) -> Uuid {
        chats::create_message(&self.pool, self.chat, role, content, None)
            .await
            .unwrap()
            .id
    }

    pub async fn history(&self) -> zone_chat::history::History {
        self.store().load().await.unwrap()
    }

    pub fn store(&self) -> Store {
        Store::new(self.pool.clone(), self.chat, Some(self.workspace))
    }

    pub async fn requests(&self) -> Vec<Value> {
        self.provider
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/chat/completions")
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }

    pub async fn until_requests(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while self.requests().await.len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("model request must arrive within fixture timeout");
    }

    pub fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.directory.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

async fn serve(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (address, task)
}

pub async fn send(socket: &mut Socket, value: Value) {
    socket
        .send(SocketMessage::Text(value.to_string().into()))
        .await
        .unwrap();
}

pub async fn next(socket: &mut Socket) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(20), socket.next())
            .await
            .expect("WebSocket event timed out")
            .expect("WebSocket closed")
            .unwrap();
        match message {
            SocketMessage::Text(text) => return serde_json::from_str(&text).unwrap(),
            SocketMessage::Close(_) => panic!("WebSocket closed before completion"),
            _ => {}
        }
    }
}

pub async fn finish(socket: &mut Socket) -> Vec<Value> {
    let mut frames = Vec::new();
    loop {
        let frame = next(socket).await;
        let terminal = matches!(
            frame["type"].as_str(),
            Some("message_end" | "error" | "cancelled")
        );
        frames.push(frame);
        if terminal {
            return frames;
        }
    }
}

pub fn successful(frames: &[Value]) {
    assert!(
        frames.iter().any(|frame| frame["type"] == "message_end"),
        "generation must complete: {frames:?}"
    );
    assert!(
        !frames.iter().any(|frame| {
            frame["type"] == "error" || frame["type"] == "message_end" && !frame["error"].is_null()
        }),
        "generation error: {frames:?}"
    );
}

pub fn ordinary(requests: &[Value]) -> Vec<&Value> {
    requests
        .iter()
        .filter(|request| request["stream"] == true)
        .collect()
}
pub fn summaries(requests: &[Value]) -> Vec<&Value> {
    requests
        .iter()
        .filter(|request| request["stream"] == false)
        .collect()
}

pub fn usage(value: &Value) -> ContextUsage {
    let usage: ContextUsage =
        serde_json::from_value(value.clone()).expect("authoritative context DTO");
    assert_eq!(usage.used, usage.breakdown.total());
    assert_eq!(
        usage.remaining,
        usage
            .threshold
            .map(|threshold| threshold.saturating_sub(usage.used))
    );
    if usage.breakdown.attachments.is_none() {
        assert!(usage.incomplete);
    }
    usage
}

pub fn pairs(request: &Value) -> usize {
    let messages = request["messages"].as_array().unwrap();
    let mut count = 0;
    let mut identifiers = std::collections::HashSet::new();
    for message in messages {
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                let id = call["id"].as_str().unwrap();
                assert!(
                    identifiers.insert(id),
                    "call ids must be unique across turns"
                );
                assert_eq!(messages.iter().filter(|message| message["role"] == "tool" && message["tool_call_id"] == id).count(), 1, "exactly one terminal result per call {id}");
                count += 1;
            }
        }
    }
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .count(),
        count,
        "no orphan tool results"
    );
    count
}

pub async fn seed_evidence(harness: &Harness, count: usize) -> Vec<String> {
    use zone_chat::history::{NewEntry, ReplayMessage, Summary, fingerprint};
    use zone_core::llm::{FunctionCall, Message, ToolCall};

    let store = harness.store();
    let lease = store.acquire(Uuid::new_v4(), LEASE_LIFETIME).await.unwrap();
    // Renewed on its own schedule as a production turn is: a bare lifetime
    // lapses mid-seed once the database is shared with the rest of the binary.
    let mut guard = store.keep_alive(lease.clone(), LEASE_LIFETIME).unwrap();
    let turn = Uuid::new_v4();
    store
        .begin(
            &lease,
            turn,
            Uuid::new_v4(),
            "Earlier historical request",
            None,
            ReplayMessage::from(&Message::user("Earlier historical request")),
        )
        .await
        .unwrap();
    // An append holds the lease row locked until it commits, so renewal cannot
    // run while one is in flight; rounds keep every transaction far shorter
    // than the lease however loaded the database is.
    let mut references = Vec::new();
    let mut covered = Vec::new();
    for round in (0..count).step_by(ROUND) {
        let last = count.min(round + ROUND);
        let calls: Vec<_> = (round..last)
            .map(|index| ToolCall {
                id: format!("historical-{index}"),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: json!({"path":format!("evidence-{index}")}).to_string(),
                },
            })
            .collect();
        let mut entries = vec![NewEntry {
            id: Uuid::new_v4().to_string(),
            message: ReplayMessage::from(&Message::assistant_with_tools(calls)),
            mutations: Vec::new(),
        }];
        for index in round..last {
            let id = format!("evidence-{}-🙂", Uuid::new_v4());
            references.push(id.clone());
            entries.push(NewEntry {
                id,
                message: ReplayMessage::from(&Message::tool_result(
                    format!("historical-{index}"),
                    format!("PRIVATE_RAW_BODY_{index} résumé🙂"),
                )),
                mutations: Vec::new(),
            });
        }
        store.append(&lease, turn, &entries).await.unwrap();
        covered.extend(entries.into_iter().map(|entry| entry.id));
    }
    store.consumed(&lease, &covered).await.unwrap();
    let history = store.load().await.unwrap();
    let mut state: Value = serde_json::from_str(&state()).unwrap();
    state["evidence"] = json!([format!("Relevant source: {}", references[0])]);
    let summary = Summary {
        content: state.to_string(),
        fingerprint: fingerprint(&history.entries, &covered).unwrap(),
        entries: covered,
        revision: 1,
    };
    store.checkpoint(&lease, None, &summary).await.unwrap();
    store
        .complete(&lease, turn, "Earlier tools completed", None)
        .await
        .unwrap();
    guard.stop().await;
    store.release(&lease).await.unwrap();
    references
}
