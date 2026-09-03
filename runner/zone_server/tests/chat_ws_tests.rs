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
