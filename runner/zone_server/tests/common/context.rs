//! Shared real-router conversation fixture. Every inference is a deterministic mock;
//! metadata discovery and database persistence exercise the production paths.
use super::{
    TestClient, create_test_router, create_test_state, test_config, test_email, test_password,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
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
use zone_core::context::ContextUsage;
use zone_server::config::Config;
use zone_server::db::{chats, context::Store};

pub const MODEL: &str = "context-test";
pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

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
    directory: PathBuf,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    pub async fn new(limit: Option<u64>, agent: bool, replies: Vec<ResponseTemplate>) -> Self {
        let provider = MockServer::start().await;
        let script = Script::new(replies);
        let responder = script.clone();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(move |request: &Request| responder.respond(request))
            .mount(&provider)
            .await;
        let models = limit.map_or_else(|| json!({"data":[]}), |limit| json!({"data":[{"model_name":MODEL,"litellm_params":{"model":format!("ollama_chat/{MODEL}"),"api_base":provider.uri(),"num_ctx":limit},"model_info":{"id":"verified-local-route"}}]}));
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(models))
            .mount(&provider)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models":[]})))
            .mount(&provider)
            .await;
        Mock::given(method("POST")).and(path("/api/show")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"capabilities":["completion","tools","vision"],"model_info":{"general.architecture":"test","test.context_length":limit.unwrap_or(32768)}}))).mount(&provider).await;
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
            directory,
            server,
        }
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
    let lease = store
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap();
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
    let calls: Vec<_> = (0..count)
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
    let mut references = Vec::new();
    for index in 0..count {
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
    let covered: Vec<_> = entries.into_iter().map(|entry| entry.id).collect();
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
    store.release(&lease).await.unwrap();
    references
}
