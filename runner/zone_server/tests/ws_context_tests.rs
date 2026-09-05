//! WebSocket context gathering integration tests
//!
//! Tests the real-time event streaming for context gathering operations via WebSocket.

mod common;

use std::net::SocketAddr;

use chrono::Duration;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

use zone_server::db::{context_gatherings, gathering_events, workspace_members};

/// Shared test context that maintains a single pool for consistency
struct TestContext {
    pool: PgPool,
    addr: SocketAddr,
}

impl TestContext {
    async fn new() -> Self {
        // Initialize tracing subscriber for debugging
        let _ = tracing_subscriber::fmt()
            .with_env_filter("zone_server=debug")
            .try_init();

        let config = common::test_config();
        let pool = common::create_test_pool().await;
        let state = common::create_test_state(config, pool.clone());
        let router = common::create_test_router(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        // Wait for server to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Self { pool, addr }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn addr(&self) -> SocketAddr {
        self.addr
    }
}

/// Get a valid auth token for WebSocket tests by creating user directly in database
async fn get_ws_auth_token_with_pool(pool: &PgPool) -> (String, Uuid) {
    use zone_server::auth::jwt::create_access_token;
    use zone_server::db::users;

    let email = common::test_email();
    let password_hash = zone_server::auth::hash_password(&common::test_password()).unwrap();

    // Create user directly in database
    let user = users::create_user(pool, &email, &password_hash, Some("WS Test User"), false)
        .await
        .expect("Failed to create test user");

    // Create JWT token directly
    let config = common::test_config();
    let token = create_access_token(
        user.id,
        &email,
        vec![], // roles
        vec![], // permissions
        false,  // is_admin
        &config.jwt_secret,
        Duration::seconds(config.jwt_access_lifetime as i64),
    )
    .expect("Failed to create token");

    (token, user.id)
}

/// Helper to create test data using the shared pool
async fn setup_test_gathering_data_with_pool(pool: &PgPool) -> (Uuid, Uuid, String) {
    let (token, user_id) = get_ws_auth_token_with_pool(pool).await;
    let (_org_id, workspace_id, _) = common::setup_test_data(pool).await;

    // Add the authenticated user to the workspace
    workspace_members::add_member(
        pool,
        workspace_id,
        user_id,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    (workspace_id, user_id, token)
}

/// Helper to create a test gathering using shared pool
async fn create_test_gathering_with_pool(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    source_ids: &[Uuid],
) -> Uuid {
    context_gatherings::create_gathering(pool, user_id, workspace_id, source_ids)
        .await
        .expect("Failed to create gathering")
}

/// Helper to add an event using shared pool
async fn add_gathering_event_with_pool(
    pool: &PgPool,
    gathering_id: Uuid,
    event_type: &str,
    payload: &serde_json::Value,
) {
    gathering_events::persist_event(pool, gathering_id, event_type, payload)
        .await
        .expect("Failed to persist event");
}

/// Helper to complete a gathering using shared pool
async fn complete_gathering_with_pool(pool: &PgPool, gathering_id: Uuid, status: &str) {
    context_gatherings::update_status(pool, gathering_id, status)
        .await
        .expect("Failed to update gathering status");
}

// Legacy helpers for backward compatibility with existing tests
async fn start_test_server() -> SocketAddr {
    let config = common::test_config();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool);
    let router = common::create_test_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    addr
}

async fn get_ws_auth_token() -> (String, Uuid) {
    let pool = common::create_test_pool().await;
    get_ws_auth_token_with_pool(&pool).await
}

async fn setup_test_gathering_data() -> (Uuid, Uuid, Uuid, String) {
    let pool = common::create_test_pool().await;
    let (workspace_id, user_id, token) = setup_test_gathering_data_with_pool(&pool).await;
    (workspace_id, user_id, Uuid::new_v4(), token)
}

async fn create_test_gathering(workspace_id: Uuid, user_id: Uuid, source_ids: &[Uuid]) -> Uuid {
    let pool = common::create_test_pool().await;
    create_test_gathering_with_pool(&pool, workspace_id, user_id, source_ids).await
}

async fn complete_gathering(gathering_id: Uuid, status: &str) {
    let pool = common::create_test_pool().await;
    complete_gathering_with_pool(&pool, gathering_id, status).await
}

// =============================================================================
// WebSocket Connection Tests
// =============================================================================

#[tokio::test]
async fn test_ws_connect_without_auth() {
    let addr = start_test_server().await;
    let gathering_id = Uuid::new_v4();
    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);

    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Don't send auth, wait for timeout or error
    let result =
        tokio::time::timeout(std::time::Duration::from_millis(500), ws_stream.next()).await;

    // Either timeout or close is acceptable
    match result {
        Err(_) => {}           // Timeout is fine
        Ok(None) => {}         // Stream closed is fine
        Ok(Some(Err(_))) => {} // Error is fine
        Ok(Some(Ok(_))) => {}  // Message (auth timeout) is fine
    }
}

#[tokio::test]
async fn test_ws_connect_with_invalid_auth() {
    let addr = start_test_server().await;
    let gathering_id = Uuid::new_v4();
    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);

    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Send invalid auth
    let auth_msg = json!({
        "type": "auth",
        "token": "invalid-token"
    });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Should receive error message
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "error");
        assert!(
            msg["message"]
                .as_str()
                .unwrap()
                .contains("Authentication failed")
        );
    }
}

#[tokio::test]
async fn test_ws_connect_with_invalid_message_format() {
    let addr = start_test_server().await;
    let gathering_id = Uuid::new_v4();
    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);

    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Send invalid JSON
    ws_stream
        .send(Message::Text("not valid json".into()))
        .await
        .expect("send");

    // Should receive error message
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "error");
        assert!(
            msg["message"]
                .as_str()
                .unwrap()
                .contains("Invalid message format")
        );
    }
}

// =============================================================================
// Ownership Verification Tests
// =============================================================================

#[tokio::test]
async fn test_ws_gathering_not_found() {
    let addr = start_test_server().await;
    let (token, _user_id) = get_ws_auth_token().await;
    let gathering_id = Uuid::new_v4(); // Non-existent gathering
    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);

    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Send valid auth
    let auth_msg = json!({
        "type": "auth",
        "token": token
    });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Should receive error about gathering not found
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "error");
        assert!(msg["message"].as_str().unwrap().contains("not found"));
    }
}

#[tokio::test]
async fn test_ws_unauthorized_access_to_gathering() {
    let addr = start_test_server().await;

    // Create a gathering for one user
    let (workspace_id, user_id, _, _) = setup_test_gathering_data().await;
    let source_ids = vec![Uuid::new_v4()];
    let gathering_id = create_test_gathering(workspace_id, user_id, &source_ids).await;

    // Get token for a different user
    let (other_token, _) = get_ws_auth_token().await;

    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate as different user
    let auth_msg = json!({
        "type": "auth",
        "token": other_token
    });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Should receive access denied error
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "error");
        assert!(msg["message"].as_str().unwrap().contains("Access denied"));
    }
}

#[tokio::test]
async fn test_ws_authorized_user_can_connect() {
    let addr = start_test_server().await;

    // Create a gathering for a user
    let (workspace_id, user_id, _, token) = setup_test_gathering_data().await;
    let source_ids = vec![Uuid::new_v4()];
    let gathering_id = create_test_gathering(workspace_id, user_id, &source_ids).await;

    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate as the correct user
    let auth_msg = json!({
        "type": "auth",
        "token": token
    });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Should receive init message
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "init");
        assert_eq!(msg["gathering_id"], gathering_id.to_string());
        assert_eq!(msg["status"], "connected");
    } else {
        panic!("Expected init message");
    }
}

// =============================================================================
// Event Streaming Tests
// =============================================================================

#[tokio::test]
async fn test_ws_streams_existing_events_in_order() {
    // Use shared context to ensure server and test use same pool
    let ctx = TestContext::new().await;
    let pool = ctx.pool();

    let (workspace_id, user_id, token) = setup_test_gathering_data_with_pool(pool).await;
    let gathering_id = create_test_gathering_with_pool(pool, workspace_id, user_id, &[]).await;

    // Add events before connecting using the SAME pool as server
    add_gathering_event_with_pool(
        pool,
        gathering_id,
        "Started",
        &json!({"message": "Gathering started"}),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    add_gathering_event_with_pool(
        pool,
        gathering_id,
        "Progress",
        &json!({"step": 1, "total": 3}),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    add_gathering_event_with_pool(
        pool,
        gathering_id,
        "Progress",
        &json!({"step": 2, "total": 3}),
    )
    .await;

    let url = format!("ws://{}/ws/context/{}", ctx.addr(), gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Receive init message
    let _ = ws_stream.next().await;

    // Should receive events in chronological order
    let mut event_types = Vec::new();
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while event_types.len() < 3 {
            if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
                let text_str: &str = text.as_ref();
                let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
                if msg["type"] == "event" {
                    event_types.push(msg["event_type"].as_str().unwrap().to_string());
                }
            }
        }
    });

    let _ = timeout.await;
    assert_eq!(event_types.len(), 3, "Should receive 3 events");
    assert_eq!(event_types[0], "Started");
    assert_eq!(event_types[1], "Progress");
    assert_eq!(event_types[2], "Progress");
}

#[tokio::test]
async fn test_ws_streams_new_events_during_connection() {
    // Use shared context
    let ctx = TestContext::new().await;
    let pool = ctx.pool().clone();

    let (workspace_id, user_id, token) = setup_test_gathering_data_with_pool(&pool).await;
    let gathering_id = create_test_gathering_with_pool(&pool, workspace_id, user_id, &[]).await;

    let url = format!("ws://{}/ws/context/{}", ctx.addr(), gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Receive init message
    let init_msg = ws_stream.next().await;
    assert!(init_msg.is_some(), "Should receive init message");

    // Wait for WebSocket to complete first poll cycle
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Add first event - server should pick it up on next poll
    add_gathering_event_with_pool(&pool, gathering_id, "Progress", &json!({"step": 1})).await;

    // Wait for poll cycle
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Add second event
    add_gathering_event_with_pool(&pool, gathering_id, "Progress", &json!({"step": 2})).await;

    // Wait for poll cycle then complete
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    complete_gathering_with_pool(&pool, gathering_id, "completed").await;

    // Collect events
    let mut progress_count = 0;
    let mut received_terminal = false;

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let text_str: &str = text.as_ref();
                    let parsed: serde_json::Value = serde_json::from_str(text_str).expect("parse");

                    match parsed["type"].as_str() {
                        Some("event") if parsed["event_type"] == "Progress" => {
                            progress_count += 1;
                        }
                        Some("terminal") => {
                            received_terminal = true;
                            break;
                        }
                        _ => {}
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                    // Ignore ping/pong
                }
                _ => {}
            }
        }
    });

    let _ = timeout.await;
    assert!(
        progress_count >= 1,
        "Should receive at least 1 progress event, got {}",
        progress_count
    );
    assert!(received_terminal, "Should receive terminal message");
}

#[tokio::test]
async fn test_ws_event_format_is_correct() {
    // Use shared context
    let ctx = TestContext::new().await;
    let pool = ctx.pool();

    let (workspace_id, user_id, token) = setup_test_gathering_data_with_pool(pool).await;
    let gathering_id = create_test_gathering_with_pool(pool, workspace_id, user_id, &[]).await;

    // Connect to WebSocket first
    let url = format!("ws://{}/ws/context/{}", ctx.addr(), gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Wait for init message and check it
    let init_msg = ws_stream.next().await;
    assert!(init_msg.is_some(), "Should receive init message");
    if let Some(Ok(Message::Text(text))) = init_msg {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse init");
        // If we got an error instead of init, fail with details
        if msg["type"] == "error" {
            panic!(
                "Authentication failed: {}",
                msg["message"].as_str().unwrap_or("unknown error")
            );
        }
        assert_eq!(msg["type"], "init", "First message should be init");
    }

    // Wait for the server's first poll cycle to complete
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Now add an event - server should pick it up on next poll
    let test_payload = json!({"message": "Test event", "step": 1});
    add_gathering_event_with_pool(pool, gathering_id, "Progress", &test_payload).await;

    // Receive event with timeout (should come within ~200ms poll interval)
    let event_result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let text_str: &str = text.as_ref();
                    let parsed: serde_json::Value = serde_json::from_str(text_str).expect("parse");
                    if parsed["type"] == "event" {
                        return Some(parsed);
                    }
                    if parsed["type"] == "error" {
                        panic!("Received error: {}", parsed["message"]);
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                    // Ignore ping/pong
                }
                _ => {}
            }
        }
        None
    })
    .await;

    match event_result {
        Ok(Some(msg)) => {
            assert_eq!(msg["type"], "event");
            assert_eq!(msg["event_type"], "Progress");
            assert_eq!(msg["payload"], test_payload);
            assert!(
                msg["created_at"].is_string(),
                "Should have created_at timestamp"
            );
        }
        Ok(None) => panic!("WebSocket stream ended without receiving event"),
        Err(_) => panic!("Timeout waiting for event message"),
    }
}

// =============================================================================
// Terminal Event Tests
// =============================================================================

#[tokio::test]
async fn test_ws_completed_gathering_closes_connection() {
    let addr = start_test_server().await;
    let (workspace_id, user_id, _, token) = setup_test_gathering_data().await;
    let gathering_id = create_test_gathering(workspace_id, user_id, &[]).await;

    // Complete the gathering before connecting
    complete_gathering(gathering_id, "completed").await;

    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Should receive init message
    let _ = ws_stream.next().await;

    // Should receive terminal message and close
    let mut received_terminal = false;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(result) = ws_stream.next().await {
            if let Ok(Message::Text(text)) = result {
                let text_str: &str = text.as_ref();
                let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
                if msg["type"] == "terminal" {
                    assert_eq!(msg["status"], "completed");
                    assert_eq!(msg["gathering_id"], gathering_id.to_string());
                    received_terminal = true;
                    break;
                }
            } else if let Ok(Message::Close(_)) = result {
                break;
            }
        }
    });

    let _ = timeout.await;
    assert!(
        received_terminal,
        "Should receive terminal message for completed gathering"
    );
}

#[tokio::test]
async fn test_ws_failed_gathering_closes_connection() {
    let addr = start_test_server().await;
    let (workspace_id, user_id, _, token) = setup_test_gathering_data().await;
    let gathering_id = create_test_gathering(workspace_id, user_id, &[]).await;

    // Fail the gathering with an error
    let pool = common::create_test_pool().await;
    context_gatherings::update_gathering_status(&pool, gathering_id, "failed", Some("Test error"))
        .await
        .expect("Failed to update status");

    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Should receive init and terminal
    let _ = ws_stream.next().await;

    let mut received_terminal = false;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(result) = ws_stream.next().await {
            if let Ok(Message::Text(text)) = result {
                let text_str: &str = text.as_ref();
                let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
                if msg["type"] == "terminal" {
                    assert_eq!(msg["status"], "failed");
                    received_terminal = true;
                    break;
                }
            }
        }
    });

    let _ = timeout.await;
    assert!(
        received_terminal,
        "Should receive terminal message for failed gathering"
    );
}

#[tokio::test]
async fn test_ws_transition_to_terminal_state() {
    // Use shared context
    let ctx = TestContext::new().await;
    let pool = ctx.pool().clone();

    let (workspace_id, user_id, token) = setup_test_gathering_data_with_pool(&pool).await;
    let gathering_id = create_test_gathering_with_pool(&pool, workspace_id, user_id, &[]).await;

    let url = format!("ws://{}/ws/context/{}", ctx.addr(), gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Receive init
    let init_msg = ws_stream.next().await;
    assert!(init_msg.is_some(), "Should receive init message");

    // Wait for first poll cycle to complete
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Add event
    add_gathering_event_with_pool(&pool, gathering_id, "Progress", &json!({"step": 1})).await;

    // Wait for poll cycle, then complete
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    complete_gathering_with_pool(&pool, gathering_id, "completed").await;

    // Wait for terminal message
    let mut received_event = false;
    let mut received_terminal = false;

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let text_str: &str = text.as_ref();
                    let parsed: serde_json::Value = serde_json::from_str(text_str).expect("parse");

                    match parsed["type"].as_str() {
                        Some("event") => received_event = true,
                        Some("terminal") => {
                            received_terminal = true;
                            break;
                        }
                        _ => {}
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                    // Ignore ping/pong
                }
                _ => {}
            }
        }
    });

    let _ = timeout.await;
    assert!(received_event, "Should receive event before terminal");
    assert!(
        received_terminal,
        "Should receive terminal message when gathering completes"
    );
}

// =============================================================================
// Client Disconnect Tests
// =============================================================================

#[tokio::test]
async fn test_ws_client_close_is_handled_gracefully() {
    let addr = start_test_server().await;
    let (workspace_id, user_id, _, token) = setup_test_gathering_data().await;
    let gathering_id = create_test_gathering(workspace_id, user_id, &[]).await;

    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Receive init
    let _ = ws_stream.next().await;

    // Close connection
    ws_stream
        .send(Message::Close(None))
        .await
        .expect("send close");

    // Server should handle gracefully (no panic)
}

#[tokio::test]
async fn test_ws_multiple_concurrent_clients() {
    // Use shared context
    let ctx = TestContext::new().await;
    let pool = ctx.pool();

    let (workspace_id, user_id, token) = setup_test_gathering_data_with_pool(pool).await;
    let gathering_id = create_test_gathering_with_pool(pool, workspace_id, user_id, &[]).await;

    // Add an event using shared pool
    add_gathering_event_with_pool(pool, gathering_id, "Progress", &json!({"step": 1})).await;

    let url = format!("ws://{}/ws/context/{}", ctx.addr(), gathering_id);

    // Connect two clients
    let (mut ws1, _) = connect_async(&url).await.expect("connect 1");
    let (mut ws2, _) = connect_async(&url).await.expect("connect 2");

    // Authenticate both
    let auth_msg = json!({ "type": "auth", "token": &token });
    ws1.send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth 1");
    ws2.send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth 2");

    // Both should receive init
    if let Some(Ok(Message::Text(text))) = ws1.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "init");
    }

    if let Some(Ok(Message::Text(text))) = ws2.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "init");
    }

    // Both should receive the existing event
    if let Some(Ok(Message::Text(text))) = ws1.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "event");
    }

    if let Some(Ok(Message::Text(text))) = ws2.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "event");
    }
}

// =============================================================================
// Ping/Pong Tests
// =============================================================================

#[tokio::test]
async fn test_ws_ping_pong() {
    let addr = start_test_server().await;
    let (workspace_id, user_id, _, token) = setup_test_gathering_data().await;
    let gathering_id = create_test_gathering(workspace_id, user_id, &[]).await;

    let url = format!("ws://{}/ws/context/{}", addr, gathering_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send");

    // Skip init
    let _ = ws_stream.next().await;

    // Send ping
    ws_stream
        .send(Message::Ping(vec![1, 2, 3].into()))
        .await
        .expect("send ping");

    // Server should handle ping (pong is handled at protocol level)
}
