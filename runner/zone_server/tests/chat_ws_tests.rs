//! End-to-end tests for the chat WebSocket at /ws/chats/:chat_id.
//!
//! POST /api/chats/:id/messages only stores the user's message; the socket is
//! the only path that produces an assistant reply, so it is the path the
//! console depends on and the one worth covering here.
//!
//! These run against the LiteLLM given by LITELLM_HOST/LITELLM_KEY. Without a
//! reachable LiteLLM the generation half cannot be exercised, so the streaming
//! test skips rather than failing on an unrelated environment problem.

mod common;

use common::{
    TestClient, create_test_pool, create_test_router, create_test_state, init_tracing, test_config,
    test_email, test_password,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use zone_server::config::Config;

/// Serve the real router on an ephemeral port and return its address.
async fn spawn_server() -> String {
    init_tracing();
    let state = create_test_state(test_config(), create_test_pool().await);
    let router = create_test_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("{}:{}", addr.ip(), addr.port())
}

async fn spawn_server_with_config(config: Config) -> String {
    init_tracing();
    let state = create_test_state(config, create_test_pool().await);
    let router = create_test_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("{}:{}", addr.ip(), addr.port())
}

/// Register a user and create a workspace and chat for it, over the same
/// database the spawned server uses.
async fn seed_chat(client: &TestClient) -> (String, String) {
    seed_chat_with_model(
        client,
        &std::env::var("TEST_MODEL").unwrap_or_else(|_| "llama3.2:3b".into()),
    )
    .await
}

/// Register a user and create a workspace and chat on a specific model.
async fn seed_chat_with_model(client: &TestClient, model: &str) -> (String, String) {
    let suffix = uuid::Uuid::new_v4().simple().to_string();

    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    let token = body["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("register must return an access token, got {body}"))
        .to_string();

    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "WS Org", "slug": format!("ws-org-{}", suffix) }),
            &token,
        )
        .await;
    let org_id = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization")
        .to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "WS Workspace", "slug": format!("ws-ws-{}", suffix) }),
            &token,
        )
        .await;
    let workspace_id = response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace")
        .to_string();

    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({
                "workspace_id": workspace_id,
                "title": "WS Chat",
                "model_name": model,
            }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["chat"]["id"]
        .as_str()
        .expect("chat")
        .to_string();

    (token, chat_id)
}

async fn next_frame(
    stream: &mut (
             impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin
         ),
    within: Duration,
) -> Option<Value> {
    loop {
        let frame = tokio::time::timeout(within, stream.next()).await.ok()??;
        match frame.ok()? {
            WsMessage::Text(text) => return serde_json::from_str(&text).ok(),
            WsMessage::Close(_) => return None,
            _ => continue,
        }
    }
}

#[tokio::test]
async fn test_chat_ws_rejects_unauthenticated_send() {
    let client = TestClient::with_db().await;
    let (_token, chat_id) = seed_chat(&client).await;
    let addr = spawn_server().await;

    let (mut socket, _) = connect_async(format!("ws://{}/ws/chats/{}", addr, chat_id))
        .await
        .expect("websocket connect");

    socket
        .send(WsMessage::Text(
            json!({ "type": "send", "content": "no auth" })
                .to_string()
                .into(),
        ))
        .await
        .expect("send");

    let frame = next_frame(&mut socket, Duration::from_secs(10))
        .await
        .expect("server must answer an unauthenticated send");
    assert_eq!(
        frame["type"], "error",
        "sending before auth must be refused, got {frame}"
    );
}

#[tokio::test]
async fn test_chat_ws_streams_an_assistant_reply() {
    let Ok(litellm_host) = std::env::var("LITELLM_HOST") else {
        eprintln!("skipping: LITELLM_HOST not set");
        return;
    };

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat(&client).await;
    let addr = spawn_server().await;

    let (mut socket, _) = connect_async(format!("ws://{}/ws/chats/{}", addr, chat_id))
        .await
        .expect("websocket connect");

    socket
        .send(WsMessage::Text(
            json!({ "type": "auth", "token": token }).to_string().into(),
        ))
        .await
        .expect("auth");

    socket
        .send(WsMessage::Text(
            json!({ "type": "send", "content": "Reply with exactly: PONG" })
                .to_string()
                .into(),
        ))
        .await
        .expect("send");

    let mut saw_saved = false;
    let mut saw_start = false;
    let mut chunks = String::new();
    let mut ended: Option<String> = None;

    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(120)).await {
        match frame["type"].as_str() {
            Some("message_saved") => saw_saved = true,
            Some("message_start") => saw_start = true,
            Some("chunk") => chunks.push_str(frame["content"].as_str().unwrap_or_default()),
            Some("message_end") => {
                ended = Some(frame["content"].as_str().unwrap_or_default().to_string());
                break;
            }
            Some("error") => panic!(
                "server reported an error against {}: {}",
                litellm_host, frame["message"]
            ),
            _ => {}
        }
    }

    assert!(saw_saved, "the user message must be saved and acknowledged");
    assert!(saw_start, "the assistant message must be announced");
    let ended = ended.expect("the assistant message must complete");
    assert!(
        !ended.trim().is_empty(),
        "the assistant reply must not be empty"
    );

    // The console renders the accumulated chunks, so they have to add up to the
    // final content rather than merely arriving.
    assert_eq!(
        chunks.trim(),
        ended.trim(),
        "streamed chunks must reconstruct the final message"
    );

    // And the reply has to survive the request: the console reloads history
    // from GET /api/chats/:id.
    let response = client
        .get_auth(&format!("/api/chats/{}", chat_id), &token)
        .await;
    let messages = response.json_value()["chat"]["messages"]
        .as_array()
        .expect("chat.messages")
        .clone();
    assert!(
        messages.iter().any(|m| m["role"] == "assistant"),
        "the assistant reply must be persisted, got {messages:?}"
    );
}

#[tokio::test]
async fn test_reply_survives_the_reader_navigating_away() {
    let Ok(_) = std::env::var("LITELLM_HOST") else {
        eprintln!("skipping: LITELLM_HOST not set");
        return;
    };

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat(&client).await;
    let addr = spawn_server().await;

    let (mut socket, _) = connect_async(format!("ws://{}/ws/chats/{}", addr, chat_id))
        .await
        .expect("websocket connect");

    socket
        .send(WsMessage::Text(
            json!({ "type": "auth", "token": token }).to_string().into(),
        ))
        .await
        .expect("auth");
    socket
        .send(WsMessage::Text(
            json!({ "type": "send", "content": "Say hello in one short sentence." })
                .to_string()
                .into(),
        ))
        .await
        .expect("send");

    // Read until generation is genuinely under way, then leave the page.
    let mut started = false;
    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(120)).await {
        if frame["type"] == "message_start" {
            started = true;
        }
        if frame["type"] == "chunk" {
            break;
        }
    }
    assert!(started, "generation must start before the reader leaves");

    drop(socket);

    // The console reloads history from the database when it comes back, so the
    // reply has to be there even though nothing was listening for the rest of it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let response = client
            .get_auth(&format!("/api/chats/{}", chat_id), &token)
            .await;
        let body = response.json_value();
        let assistant = body["chat"]["messages"]
            .as_array()
            .expect("chat.messages")
            .iter()
            .find(|m| m["role"] == "assistant")
            .cloned();

        if let Some(message) = assistant {
            assert!(
                !message["content"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .is_empty(),
                "a persisted reply must not be empty"
            );
            return;
        }

        assert!(
            tokio::time::Instant::now() < deadline,
            "the assistant reply was discarded when the reader navigated away"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// A 64x64 solid red PNG, inlined so the test needs no image dependencies.
fn red_png_data_url() -> String {
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAb0lEQVR4nO3PAQkAAAyEwO9feoshgnABdLep8QUNyPEFDcjxBQ3I8QUNyPEFDcjxBQ3I8QUNyPEFDcjxBQ3I8QUNyPEFDcjxBQ3I8QUNyPEFDcjxBQ3I8QUNyPEFDcjxBQ3I8QUNyPEFDcjxBQ3IPanc8OLDQitxAAAAAElFTkSuQmCC"
        .to_string()
}

#[tokio::test]
async fn test_chat_ws_sends_images_to_a_vision_model() {
    let Ok(_) = std::env::var("LITELLM_HOST") else {
        eprintln!("skipping: LITELLM_HOST not set");
        return;
    };
    let vision_model = std::env::var("TEST_VISION_MODEL").unwrap_or_else(|_| "llava:7b".into());

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat_with_model(&client, &vision_model).await;
    let addr = spawn_server().await;

    let (mut socket, _) = connect_async(format!("ws://{}/ws/chats/{}", addr, chat_id))
        .await
        .expect("websocket connect");

    socket
        .send(WsMessage::Text(
            json!({ "type": "auth", "token": token }).to_string().into(),
        ))
        .await
        .expect("auth");

    // Images travel in metadata; the server widens the LLM body into content
    // parts so a vision model actually receives them.
    socket
        .send(WsMessage::Text(
            json!({
                "type": "send",
                "content": "What colour is this image? Answer in one word.",
                "metadata": {
                    "attachments": [
                        { "name": "red.png", "mime": "image/png", "url": red_png_data_url() }
                    ]
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send");

    let mut reply = String::new();
    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(240)).await {
        match frame["type"].as_str() {
            Some("message_end") => {
                reply = frame["content"].as_str().unwrap_or_default().to_string();
                break;
            }
            Some("error") => panic!("server error: {}", frame["message"]),
            _ => {}
        }
    }

    assert!(
        reply.to_lowercase().contains("red"),
        "the vision model must describe the image it was sent, got: {reply}"
    );
}

#[tokio::test]
async fn test_image_request_routes_directly_and_serves_protected_artifact() {
    let comfy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .and(wiremock::matchers::body_string_contains(
            "custom-image.safetensors",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "flux-1"})))
        .expect(1)
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/flux-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "flux-1": {
                "status": {"status_str": "success"},
                "outputs": {"7": {"images": [{
                    "filename": "zone_flux.png", "subfolder": "", "type": "temp"
                }]}}
            }
        })))
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(vec![137, 80, 78, 71]),
        )
        .expect(1)
        .mount(&comfy)
        .await;

    let client = TestClient::with_db().await;
    // This model deliberately fails normal-chat validation. Image routing must
    // still succeed because the selected chat model is not changed or invoked.
    let (token, chat_id) = seed_chat_with_model(&client, "not-a-chat-model").await;
    let artifact_root =
        std::env::temp_dir().join(format!("zone-ws-artifacts-{}", uuid::Uuid::new_v4()));
    let mut config = test_config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = comfy.uri();
    config.comfyui.poll_interval_ms = 50;
    config.comfyui.artifact_root = artifact_root.clone();
    config.comfyui.checkpoint = "custom-image.safetensors".to_string();
    let addr = spawn_server_with_config(config).await;

    let (mut socket, _) = connect_async(format!("ws://{}/ws/chats/{}", addr, chat_id))
        .await
        .expect("websocket connect");
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    socket
        .send(WsMessage::Text(
            json!({
                "type": "send",
                "content": "Please generate an image of a blue fox",
                "metadata": {"image_generation": true}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let mut image_url = None;
    let mut assistant_message_id = None;
    let mut saw_progress = false;
    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(10)).await {
        match frame["type"].as_str() {
            Some("status") => saw_progress = true,
            Some("image") => {
                image_url = frame["attachment"]["url"].as_str().map(str::to_string);
            }
            Some("message_end") => {
                assistant_message_id = frame["message_id"].as_str().map(str::to_string);
                break;
            }
            Some("error") => panic!("unexpected image generation error: {frame}"),
            _ => {}
        }
    }
    assert!(saw_progress);
    let image_url = image_url.expect("image event must contain an artifact URL");
    let assistant_message_id =
        assistant_message_id.expect("message_end must contain the persisted message ID");
    assert!(image_url.starts_with("/api/artifacts/"));
    assert!(image_url.contains(&format!("/{assistant_message_id}/")));

    let artifact = reqwest::Client::new()
        .get(format!("http://{addr}{image_url}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(artifact.status(), reqwest::StatusCode::OK);
    assert_eq!(artifact.bytes().await.unwrap().as_ref(), &[137, 80, 78, 71]);
    let reloaded = client
        .get_auth(&format!("/api/chats/{chat_id}"), &token)
        .await
        .json_value();
    let persisted = reloaded["chat"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["metadata"]["attachments"][0]["url"] == image_url)
        .expect("generated image metadata must survive reload");
    assert_eq!(persisted["id"], assistant_message_id);

    // Deleting a metadata-crafted message must not be able to remove another
    // message's artifacts.
    let malicious = client
        .post_json_auth(
            &format!("/api/chats/{chat_id}/messages"),
            &json!({
                "role": "user",
                "content": "crafted metadata",
                "metadata": {"attachments": [{
                    "name": "stolen.png",
                    "mime": "image/png",
                    "url": image_url
                }]}
            }),
            &token,
        )
        .await
        .json_value()["message"]["id"]
        .as_str()
        .expect("crafted message id")
        .to_string();
    let crafted_deleted = reqwest::Client::new()
        .delete(format!(
            "http://{addr}/api/chats/{chat_id}/messages/{malicious}"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(crafted_deleted.status(), reqwest::StatusCode::NO_CONTENT);
    let retained_artifact = reqwest::Client::new()
        .get(format!("http://{addr}{image_url}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(retained_artifact.status(), reqwest::StatusCode::OK);

    let deleted = reqwest::Client::new()
        .delete(format!(
            "http://{addr}/api/chats/{chat_id}/messages/{assistant_message_id}"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), reqwest::StatusCode::NO_CONTENT);
    let removed_artifact = reqwest::Client::new()
        .get(format!("http://{addr}{image_url}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(removed_artifact.status(), reqwest::StatusCode::NOT_FOUND);
    let _ = tokio::fs::remove_dir_all(artifact_root).await;
}

#[tokio::test]
async fn test_image_failure_never_announces_empty_assistant_message() {
    let comfy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&comfy)
        .await;

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat(&client).await;
    let mut config = test_config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = comfy.uri();
    let addr = spawn_server_with_config(config).await;
    let (mut socket, _) = connect_async(format!("ws://{addr}/ws/chats/{chat_id}"))
        .await
        .expect("websocket connect");
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    socket
        .send(WsMessage::Text(
            json!({
                "type": "send",
                "content": "Generate an image of a failed request",
                "metadata": {"image_generation": true}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let mut saw_start = false;
    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(10)).await {
        match frame["type"].as_str() {
            Some("message_start") => saw_start = true,
            Some("error") => break,
            _ => {}
        }
    }
    assert!(
        !saw_start,
        "a failed generation must not create an empty bubble"
    );

    let reloaded = client
        .get_auth(&format!("/api/chats/{chat_id}"), &token)
        .await
        .json_value();
    assert!(
        reloaded["chat"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["role"] != "assistant"),
        "failed generation must not persist an empty assistant message"
    );
}

#[tokio::test]
async fn test_image_persist_failure_sends_error_without_empty_message() {
    let comfy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "flux-io"})))
        .expect(1)
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/flux-io"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "flux-io": {
                "status": {"status_str": "success"},
                "outputs": {"7": {"images": [{
                    "filename": "zone.png", "subfolder": "", "type": "temp"
                }]}}
            }
        })))
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(vec![137, 80, 78, 71]),
        )
        .expect(1)
        .mount(&comfy)
        .await;

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat(&client).await;
    let artifact_root =
        std::env::temp_dir().join(format!("zone-ws-not-a-dir-{}", uuid::Uuid::new_v4()));
    tokio::fs::write(&artifact_root, b"not a directory")
        .await
        .unwrap();
    let mut config = test_config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = comfy.uri();
    config.comfyui.poll_interval_ms = 50;
    config.comfyui.artifact_root = artifact_root.clone();
    let addr = spawn_server_with_config(config).await;
    let (mut socket, _) = connect_async(format!("ws://{addr}/ws/chats/{chat_id}"))
        .await
        .expect("websocket connect");
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    socket
        .send(WsMessage::Text(
            json!({
                "type": "send",
                "content": "Generate an image of an unwritable store",
                "metadata": {"image_generation": true}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let mut saw_start = false;
    let mut error = None;
    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(10)).await {
        match frame["type"].as_str() {
            Some("message_start") => saw_start = true,
            Some("error") => {
                error = frame["message"].as_str().map(str::to_string);
                break;
            }
            _ => {}
        }
    }
    assert!(
        !saw_start,
        "a persist failure must not create an empty bubble"
    );
    assert_eq!(
        error.as_deref(),
        Some("Image generation failed: could not store the image")
    );
    let _ = tokio::fs::remove_file(artifact_root).await;
}

struct DelayedPrompt {
    prompt_id: String,
    starts: std::sync::Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
}

impl wiremock::Respond for DelayedPrompt {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        self.starts.lock().unwrap().push(std::time::Instant::now());
        std::thread::sleep(Duration::from_millis(80));
        ResponseTemplate::new(200).set_body_json(json!({"prompt_id": self.prompt_id}))
    }
}

async fn collect_generated_image(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> (String, String) {
    let mut image_url = None;
    let mut message_id = None;
    while let Some(frame) = next_frame(socket, Duration::from_secs(10)).await {
        match frame["type"].as_str() {
            Some("image") => image_url = frame["attachment"]["url"].as_str().map(str::to_string),
            Some("message_end") => {
                message_id = frame["message_id"].as_str().map(str::to_string);
                break;
            }
            Some("error") => panic!("unexpected concurrent generation error: {frame}"),
            _ => {}
        }
    }
    (message_id.expect("message_end"), image_url.expect("image"))
}

#[tokio::test]
async fn test_concurrent_image_sends_are_serialized_per_chat() {
    let comfy = MockServer::start().await;
    let prompt_starts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(DelayedPrompt {
            prompt_id: "flux-a".to_string(),
            starts: prompt_starts.clone(),
        })
        .expect(2)
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/flux-a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "flux-a": {
                "status": {"status_str": "success"},
                "outputs": {"7": {"images": [{
                    "filename": "zone.png", "subfolder": "", "type": "temp"
                }]}}
            }
        })))
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(vec![137, 80, 78, 71]),
        )
        .mount(&comfy)
        .await;

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat_with_model(&client, "not-a-chat-model").await;
    let artifact_root =
        std::env::temp_dir().join(format!("zone-ws-concurrent-{}", uuid::Uuid::new_v4()));
    let mut config = test_config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = comfy.uri();
    config.comfyui.poll_interval_ms = 50;
    config.comfyui.artifact_root = artifact_root.clone();
    let addr = spawn_server_with_config(config).await;

    let connect = async |token: &str| {
        let (mut socket, _) = connect_async(format!("ws://{addr}/ws/chats/{chat_id}"))
            .await
            .expect("websocket connect");
        socket
            .send(WsMessage::Text(
                json!({"type": "auth", "token": token}).to_string().into(),
            ))
            .await
            .unwrap();
        socket
    };
    let mut first = connect(&token).await;
    let mut second = connect(&token).await;
    let send = json!({
        "type": "send",
        "content": "Generate an image of overlapping foxes",
        "metadata": {"image_generation": true}
    })
    .to_string();
    first
        .send(WsMessage::Text(send.clone().into()))
        .await
        .unwrap();
    second.send(WsMessage::Text(send.into())).await.unwrap();

    let first_result = collect_generated_image(&mut first);
    let second_result = collect_generated_image(&mut second);
    let ((first_id, first_url), (second_id, second_url)) =
        tokio::join!(first_result, second_result);
    assert_ne!(first_id, second_id);
    assert!(first_url.contains(&format!("/{first_id}/")));
    assert!(second_url.contains(&format!("/{second_id}/")));

    let starts = prompt_starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 2);
    let gap = starts[1].saturating_duration_since(starts[0]);
    assert!(
        gap >= Duration::from_millis(70),
        "per-chat generation must serialize overlapping sends, gap was {gap:?}"
    );
    let _ = tokio::fs::remove_dir_all(artifact_root).await;
}

#[tokio::test]
async fn test_protected_artifact_urls_are_not_forwarded_to_litellm() {
    let litellm = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"safe\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
                ),
        )
        .mount(&litellm)
        .await;

    let client = TestClient::with_db().await;
    let (token, chat_id) = seed_chat(&client).await;
    let protected = "/api/artifacts/00000000-0000-0000-0000-000000000001/00000000-0000-0000-0000-000000000002/00000000-0000-0000-0000-000000000003/image.png";
    client
        .post_json_auth(
            &format!("/api/chats/{chat_id}/messages"),
            &json!({
                "role": "assistant",
                "content": "Generated image.",
                "metadata": {"attachments": [{
                    "name": "generated-image-1.png",
                    "mime": "image/png",
                    "url": protected
                }]}
            }),
            &token,
        )
        .await;

    let mut config = test_config();
    config.litellm_host = litellm.uri();
    config.comfyui.enabled = true;
    let addr = spawn_server_with_config(config).await;
    let (mut socket, _) = connect_async(format!("ws://{addr}/ws/chats/{chat_id}"))
        .await
        .expect("websocket connect");
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    socket
        .send(WsMessage::Text(
            json!({"type": "send", "content": "What color was the previous answer?"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    while let Some(frame) = next_frame(&mut socket, Duration::from_secs(10)).await {
        match frame["type"].as_str() {
            Some("message_end") | Some("error") => break,
            _ => {}
        }
    }

    let requests = litellm.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.url.path() == "/chat/completions"),
        "LiteLLM must receive the follow-up"
    );
    for request in requests {
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            !body.contains("/api/artifacts/"),
            "protected artifact URLs must not be forwarded to LiteLLM: {body}"
        );
        assert!(
            !body.contains(protected),
            "the exact protected URL must stay out of later-turn model context"
        );
    }
}
