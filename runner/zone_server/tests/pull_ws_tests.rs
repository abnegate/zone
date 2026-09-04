//! Model downloads through the public WebSocket route and a real mock provider.
mod common;

use axum::{Json, Router, body::Body, http::StatusCode, response::IntoResponse, routing::post};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use zone_server::auth::{create_access_token, create_refresh_token};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const MODEL: &str = "qwen3.8:27b";

struct Fixture {
    model: &'static str,
    url: String,
    token: String,
    refresh: String,
    calls: Arc<AtomicUsize>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Fixture {
    async fn new(model: &'static str, status: StatusCode, chunks: Vec<&'static str>) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let provider = Router::new().route(
            "/api/pull",
            post(move |Json(payload): Json<Value>| {
                let counter = counter.clone();
                let chunks = chunks.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(payload, json!({"model": model, "stream": true}));
                    let stream = async_stream::stream! {
                        for chunk in chunks {
                            yield Ok::<_, std::io::Error>(chunk);
                            tokio::task::yield_now().await;
                        }
                    };
                    (status, Body::from_stream(stream)).into_response()
                }
            }),
        );
        Self::start(model, provider, calls).await
    }

    async fn start(model: &'static str, provider: Router, calls: Arc<AtomicUsize>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let provider = tokio::spawn(async move {
            axum::serve(listener, provider).await.unwrap();
        });
        let config = common::test_config_with_ollama_host(&format!("http://{address}"));
        let token = create_access_token(
            uuid::Uuid::new_v4(),
            "user@example.com",
            vec![],
            vec![],
            false,
            config.jwt_secret(),
            chrono::Duration::hours(1),
        )
        .unwrap();
        let refresh = create_refresh_token(
            uuid::Uuid::new_v4(),
            config.jwt_secret(),
            chrono::Duration::hours(1),
        )
        .unwrap();
        let pool = PgPoolOptions::new()
            .connect_lazy(&config.database_url)
            .unwrap();
        let router = common::create_test_router(common::create_test_state(config, pool));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            model,
            url: format!("ws://{address}/ws/pull"),
            token,
            refresh,
            calls,
            tasks: vec![provider, server],
        }
    }

    async fn connect(&self) -> Socket {
        connect_async(&self.url)
            .await
            .expect("model pull route must upgrade")
            .0
    }

    async fn pull(&self) -> Socket {
        let mut socket = self.connect().await;
        send(&mut socket, json!({"type": "auth", "token": self.token})).await;
        assert_eq!(receive(&mut socket).await, json!({"type": "authenticated"}));
        assert_eq!(self.calls.load(Ordering::SeqCst), 0);
        send(&mut socket, json!({"model": self.model})).await;
        socket
    }
}

async fn send(socket: &mut Socket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn receive(socket: &mut Socket) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("frame timeout")
        .expect("connection closed")
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn rejects_missing_invalid_and_refresh_authentication_without_contacting_provider() {
    let fixture = Fixture::new(MODEL, StatusCode::OK, vec![]).await;
    for message in [
        json!({"model": MODEL}),
        json!({"type": "auth", "token": "invalid"}),
        json!({"type": "auth", "token": fixture.refresh}),
    ] {
        let mut socket = fixture.connect().await;
        send(&mut socket, message).await;
        assert_eq!(
            receive(&mut socket).await,
            json!({"type": "error", "message": "Authentication failed"})
        );
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn authenticates_and_streams_chunked_progress_until_success() {
    let fixture = Fixture::new(
        MODEL,
        StatusCode::OK,
        vec![
            "{\"sta",
            "tus\":\"pulling manifest\"}\n{\"status\":\"pulling layer\",\"total\":100,",
            "\"completed\":25}\n{\"status\":\"success\"}",
        ],
    )
    .await;
    let mut socket = fixture.pull().await;
    assert_eq!(receive(&mut socket).await["status"], "pulling manifest");
    assert_eq!(receive(&mut socket).await["status"], "pulling layer");
    let progress = receive(&mut socket).await;
    assert_eq!(progress["type"], "progress");
    assert_eq!(progress["percent"], 25.0);
    let complete = receive(&mut socket).await;
    assert_eq!(complete["type"], "complete");
    assert_eq!(complete["success"], true);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_manifest_errors_identify_the_unchanged_reference_and_supported_formats() {
    for model in [
        "qwen/qwen3.8-27b",
        "custom/model:revision",
        "hf.co/owner/GGUF-repository:Q4_K_M",
    ] {
        for (status, chunks) in [
            (
                StatusCode::NOT_FOUND,
                vec![
                    "{\"error\":\"pull model manifest: ",
                    "file does not exist\"}",
                ],
            ),
            (
                StatusCode::OK,
                vec![
                    "{\"error\":\"pull model manifest: ",
                    "file does not exist\"}\n",
                ],
            ),
            (
                StatusCode::OK,
                vec![
                    "{\"error\":\"pull model manifest: ",
                    "file does not exist\"}",
                ],
            ),
        ] {
            let fixture = Fixture::new(model, status, chunks).await;
            let mut socket = fixture.pull().await;
            assert_eq!(
                receive(&mut socket).await,
                json!({
                    "type": "error",
                    "message": format!(
                        "pull model manifest: file does not exist. Ollama could not find \"{model}\". Use an Ollama model:tag or hf.co/owner/GGUF-repository reference."
                    )
                })
            );
            assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn preserves_unrelated_http_and_stream_provider_errors() {
    for (body, message) in [
        ("{\"error\":\"file does not exist\"}", "file does not exist"),
        (
            "{\"error\":\"pull model manifest: unauthorized\"}",
            "pull model manifest: unauthorized",
        ),
        (
            "{\"error\":\"pull model manifest: file does not exist: permission denied\"}",
            "pull model manifest: file does not exist: permission denied",
        ),
    ] {
        for status in [StatusCode::NOT_FOUND, StatusCode::OK] {
            let fixture = Fixture::new(MODEL, status, vec![body]).await;
            let mut socket = fixture.pull().await;
            assert_eq!(
                receive(&mut socket).await,
                json!({"type": "error", "message": message})
            );
        }
    }
}

#[tokio::test]
async fn incomplete_or_malformed_stream_never_reports_success() {
    for chunks in [
        vec!["{\"status\":\"pulling manifest\"}\n"],
        vec!["not json\n"],
    ] {
        let fixture = Fixture::new(MODEL, StatusCode::OK, chunks).await;
        let mut socket = fixture.pull().await;
        loop {
            let event = receive(&mut socket).await;
            assert_ne!(event["type"], "complete");
            if event["type"] == "error" {
                break;
            }
        }
    }
}

struct Disconnected(Arc<tokio::sync::Notify>);

impl Drop for Disconnected {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test]
async fn client_disconnect_drops_the_upstream_download() {
    let stopped = Arc::new(tokio::sync::Notify::new());
    let notification = stopped.clone();
    let provider = Router::new().route(
        "/api/pull",
        post(move || {
            let notification = notification.clone();
            async move {
                let stream = async_stream::stream! {
                    let _guard = Disconnected(notification);
                    yield Ok::<_, std::io::Error>("{\"status\":\"pulling manifest\"}\n");
                    std::future::pending::<()>().await;
                };
                Body::from_stream(stream)
            }
        }),
    );
    let fixture = Fixture::start(MODEL, provider, Arc::new(AtomicUsize::new(0))).await;
    let mut socket = fixture.pull().await;
    assert_eq!(receive(&mut socket).await["type"], "step");
    socket.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), stopped.notified())
        .await
        .expect("upstream body must be dropped when client disconnects");
}
