//! An OpenAI-compatible LiteLLM stand-in for tests that run the real server
//! binary: streamed completions come from a script, chunk by chunk with set
//! pauses, and plain completions, such as a chat's title, return set text.

mod reply;
mod script;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub use reply::Reply;
use script::Script;

const MODEL: &str = "stub-model";
const TITLE: &str = "Stub title";

pub struct StubLitellm {
    address: SocketAddr,
    script: Arc<Script>,
    server: JoinHandle<()>,
}

impl StubLitellm {
    /// Streams "Hello" and titles every chat "Stub title" until told otherwise.
    pub async fn start() -> Self {
        let script = Arc::new(Script::new(
            Reply::new(["Hello"], Duration::ZERO),
            TITLE.to_string(),
        ));
        let router = Router::new()
            .route("/chat/completions", post(complete))
            .route("/v1/chat/completions", post(complete))
            .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({"error": "not stubbed"}))) })
            .with_state(Arc::clone(&script));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port for the stub");
        let address = listener.local_addr().expect("the stub's address");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self {
            address,
            script,
            server,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The reply the next streamed completion gets; queued replies go in order.
    pub fn queue(&self, reply: Reply) {
        self.script.queue(reply);
    }

    /// The reply every streamed completion gets once the queue is empty.
    pub fn fall_back_to(&self, reply: Reply) {
        self.script.fall_back_to(reply);
    }

    /// The text every plain, unstreamed completion returns.
    pub fn complete_with(&self, completion: impl Into<String>) {
        self.script.complete_with(completion.into());
    }

    /// Every request body received, in order.
    pub fn requests(&self) -> Vec<Value> {
        self.script.requests()
    }

    /// How many streamed completions were asked for.
    pub fn streamed(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| request["stream"] == true)
            .count()
    }
}

impl Drop for StubLitellm {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn complete(State(script): State<Arc<Script>>, Json(request): Json<Value>) -> Response {
    script.record(request.clone());
    let model = request["model"].as_str().unwrap_or(MODEL).to_string();
    if request["stream"] != true {
        return Json(json!({
            "id": "stub",
            "object": "chat.completion",
            "created": 0,
            "model": model,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": script.completion()},
                "finish_reason": "stop",
            }],
        }))
        .into_response();
    }
    let reply = script.next();
    let chunks = reply
        .chunks
        .into_iter()
        .map(move |content| {
            (
                reply.pause,
                chunk(&model, json!({"content": content}), None),
            )
        })
        .chain([
            (Duration::ZERO, chunk(MODEL, json!({}), Some("stop"))),
            (Duration::ZERO, "data: [DONE]\n\n".to_string()),
        ]);
    let body = futures_util::stream::iter(chunks).then(|(pause, event)| async move {
        tokio::time::sleep(pause).await;
        Ok::<Bytes, Infallible>(Bytes::from(event))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .expect("a streamed response")
}

fn chunk(model: &str, delta: Value, finish: Option<&str>) -> String {
    let event = json!({
        "id": "stub",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
    });
    format!("data: {event}\n\n")
}
