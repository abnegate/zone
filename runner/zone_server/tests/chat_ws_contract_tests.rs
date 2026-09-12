//! WebSocket boundary and lifecycle contracts that do not require a live model.

mod common;

use chrono::{Duration as ChronoDuration, Utc};
use common::{
    TestClient, create_test_pool, create_test_router, create_test_state, test_config, test_email,
    test_password,
};
use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WsMessage,
};
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use zone_core::tools::job::{JobExited, JobStarted};
use zone_server::{
    agent::wait::{WaitSettled, Waiting},
    auth::validate_access_token,
    config::Config,
    db::{actions, chats},
    ws::chat::ServerMessage,
};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn spawn(config: Config, pool: PgPool) -> String {
    let router = create_test_router(create_test_state(config, pool));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("{}:{}", address.ip(), address.port())
}

async fn register(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    response["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("registration did not return an access token: {response}"))
        .to_string()
}

async fn seed(client: &TestClient) -> (String, Uuid, Uuid) {
    let token = register(client).await;
    let suffix = Uuid::new_v4().simple().to_string();
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Socket Contracts", "slug": format!("socket-contracts-{suffix}")}),
            &token,
        )
        .await
        .json_value();
    let organization = organization["organization"]["id"].as_str().unwrap();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Socket Contracts", "slug": format!("socket-contracts-{suffix}")}),
            &token,
        )
        .await
        .json_value();
    let workspace = Uuid::parse_str(workspace["workspace"]["id"].as_str().unwrap()).unwrap();
    let chat = client
        .post_json_auth(
            "/api/chats",
            &json!({
                "workspace_id": workspace,
                "title": "Socket Contracts",
                "model_name": "llama3.2:3b"
            }),
            &token,
        )
        .await
        .json_value();
    let chat = Uuid::parse_str(chat["chat"]["id"].as_str().unwrap()).unwrap();
    (token, workspace, chat)
}

async fn connect(address: &str, chat: Uuid) -> Socket {
    connect_async(format!("ws://{address}/ws/chats/{chat}"))
        .await
        .unwrap()
        .0
}

async fn authenticate(address: &str, chat: Uuid, token: &str) -> Socket {
    let mut socket = connect(address, chat).await;
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    let init = next_json(&mut socket).await.expect("initial status");
    assert_eq!(init["type"], "init", "{init}");
    assert_eq!(init["status"], "connected", "{init}");
    socket
}

/// Mirrors AUTH_RECHECK_INTERVAL in the server, which is private to it.
const AUTH_RECHECK: Duration = Duration::from_secs(10);

async fn next_json(socket: &mut Socket) -> Option<Value> {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .ok()??
            .ok()?;
        match frame {
            WsMessage::Text(text) => return serde_json::from_str(&text).ok(),
            WsMessage::Close(_) => return None,
            _ => {}
        }
    }
}

async fn assert_error(socket: &mut Socket, expected: &str) {
    let frame = next_json(socket).await.expect("error frame");
    assert_eq!(frame, json!({"type": "error", "message": expected}));
}

async fn periodic_error(socket: &mut Socket, count: usize) -> Option<String> {
    let mut pong = true;
    for _ in 0..count {
        tokio::time::advance(Duration::from_secs(30)).await;
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        let frame = socket.next().await?.ok()?;
        match frame {
            WsMessage::Ping(data) if pong => {
                // The server closes the moment it reports, so this pong can
                // find a broken pipe. Keep reading rather than giving up: the
                // error frame it sent before closing is still on its way.
                if socket.send(WsMessage::Pong(data)).await.is_err() {
                    pong = false;
                }
            }
            WsMessage::Text(text) => {
                let frame: Value = serde_json::from_str(&text).ok()?;
                if frame["type"] == "error" {
                    return frame["message"].as_str().map(str::to_string);
                }
            }
            WsMessage::Close(_) => return None,
            _ => {}
        }
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
    }
    None
}

#[tokio::test]
async fn unauthenticated_connections_reject_bad_tokens_and_excess_fanout() {
    let pool = create_test_pool().await;
    let address = spawn(test_config(), pool).await;

    let mut invalid = connect(&address, Uuid::new_v4()).await;
    invalid
        .send(WsMessage::Text(
            json!({"type": "auth", "token": "not-a-jwt"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_error(&mut invalid, "Authentication failed").await;
    assert!(next_json(&mut invalid).await.is_none());

    let limited_chat = Uuid::new_v4();
    let mut held = Vec::new();
    for _ in 0..5 {
        held.push(connect(&address, limited_chat).await);
    }
    let mut rejected = connect(&address, limited_chat).await;
    assert_error(&mut rejected, "Too many connections").await;
    assert!(next_json(&mut rejected).await.is_none());

    for mut socket in held {
        socket.close(None).await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn silent_connection_receives_an_authentication_timeout() {
    let pool = PgPool::connect_lazy("postgres://unused:unused@127.0.0.1:9/unused").unwrap();
    let address = spawn(test_config(), pool).await;
    let mut socket = connect(&address, Uuid::new_v4()).await;

    tokio::time::advance(Duration::from_secs(31)).await;
    // The frame still has to cross a real socket. While the clock is paused
    // the runtime answers next_json's timeout by jumping to its deadline, so
    // hand the read real time to arrive in.
    tokio::time::resume();
    assert_error(&mut socket, "Authentication timeout or error").await;
    assert!(next_json(&mut socket).await.is_none());
}

#[tokio::test]
async fn chat_lookup_database_failure_is_reported_without_internal_detail() {
    let healthy = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        healthy,
    )));
    let token = register(&client).await;
    let unavailable = PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(200))
        .connect_lazy("postgres://unused:unused@127.0.0.1:9/unavailable")
        .unwrap();
    let address = spawn(config, unavailable).await;
    let mut socket = connect(&address, Uuid::new_v4()).await;
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();

    assert_error(&mut socket, "Internal server error").await;
    assert!(next_json(&mut socket).await.is_none());
}

#[tokio::test]
async fn membership_database_failure_is_reported_without_internal_detail() {
    const PASSWORD: &str = "zone-contract-password";

    let owner = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        owner.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let role = format!("zone_contract_{}", Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE {role} LOGIN PASSWORD '{PASSWORD}'"
    )))
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "GRANT USAGE ON SCHEMA public TO {role}"
    )))
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "GRANT SELECT ON chats TO {role}"
    )))
    .execute(&owner)
    .await
    .unwrap();

    let options = std::env::var("DATABASE_URL")
        .unwrap()
        .parse::<PgConnectOptions>()
        .unwrap()
        .username(&role)
        .password(PASSWORD);
    let restricted = PgPoolOptions::new().connect_with(options).await.unwrap();
    let address = spawn(config, restricted.clone()).await;
    let mut socket = connect(&address, chat).await;
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    let frame = next_json(&mut socket).await;
    let closed = next_json(&mut socket).await;

    restricted.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP OWNED BY {role}")))
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE {role}")))
        .execute(&owner)
        .await
        .unwrap();

    assert_eq!(
        frame,
        Some(json!({"type": "error", "message": "Internal server error"}))
    );
    assert!(closed.is_none());
}

#[tokio::test]
async fn authentication_fences_invalid_missing_unscoped_and_foreign_chats() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let owner = register(&client).await;
    let address = spawn(config.clone(), pool.clone()).await;

    let now = Utc::now();
    // An access token decodes as the claims flattened alongside a token_type,
    // and a token missing that type is rejected as unauthenticated before the
    // subject is ever read. Carry it so the malformed subject is what fails.
    let invalid_subject = encode(
        &Header::default(),
        &json!({
            "sub": "not-a-uuid",
            "email": test_email(),
            "roles": [],
            "permissions": [],
            "exp": (now + ChronoDuration::minutes(5)).timestamp(),
            "iat": now.timestamp(),
            "jti": Uuid::new_v4().to_string(),
            "is_admin": false,
            "token_type": "access",
        }),
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .unwrap();
    let mut socket = connect(&address, Uuid::new_v4()).await;
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": invalid_subject})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_error(&mut socket, "Invalid user ID").await;

    let mut missing = connect(&address, Uuid::new_v4()).await;
    missing
        .send(WsMessage::Text(
            json!({"type": "auth", "token": owner}).to_string().into(),
        ))
        .await
        .unwrap();
    assert_error(&mut missing, "Chat not found").await;

    let unscoped = chats::create_chat(&pool, None, "Unscoped", "llama3.2:3b", false, true)
        .await
        .unwrap();
    let mut socket = connect(&address, unscoped.id).await;
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": owner}).to_string().into(),
        ))
        .await
        .unwrap();
    assert_error(&mut socket, "Invalid chat configuration").await;

    let (_, _, chat) = seed(&client).await;
    let stranger = register(&client).await;
    let mut socket = connect(&address, chat).await;
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": stranger})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_error(&mut socket, "Access denied").await;
}

#[tokio::test]
async fn authenticated_socket_enforces_control_and_input_contracts() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool).await;
    let mut socket = authenticate(&address, chat, &token).await;

    socket
        .send(WsMessage::Ping(vec![1, 2, 3].into()))
        .await
        .unwrap();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let WsMessage::Pong(data) = frame {
            assert_eq!(data.as_ref(), &[1, 2, 3]);
            break;
        }
    }

    socket
        .send(WsMessage::Text(
            json!({"type":"auth","token":token}).to_string().into(),
        ))
        .await
        .unwrap();
    socket
        .send(WsMessage::Pong(vec![4, 5, 6].into()))
        .await
        .unwrap();

    socket
        .send(WsMessage::Text(
            json!({"type": "approve_tool", "tool_call_id": "unknown", "approved": true})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let refusal = next_json(&mut socket).await.expect("a refusal frame");
    assert_eq!(
        refusal,
        json!({"type": "tool_approval_closed", "tool_call_id": "unknown"})
    );

    socket
        .send(WsMessage::Text("not-json".to_string().into()))
        .await
        .unwrap();
    assert_error(&mut socket, "Invalid message format").await;

    let oversized = "x".repeat(100_001);
    for _ in 0..20 {
        socket
            .send(WsMessage::Text(
                json!({"type": "send", "content": oversized})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        assert_error(&mut socket, "Message too long").await;
    }
    socket
        .send(WsMessage::Text(
            json!({"type": "send", "content": oversized})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_error(&mut socket, "Rate limit exceeded").await;
    socket.close(None).await.unwrap();
}

/// A decision for a call nothing is waiting on is refused to the connection
/// that sent it, and to that connection alone. It is its own frame rather than
/// an `Error` because nothing about the turn has gone wrong: a second window
/// that loses the race to decide a card must not be told the reply died.
#[tokio::test]
async fn a_decision_nothing_is_waiting_on_is_refused_to_that_connection_alone() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool).await;
    let mut deciding = authenticate(&address, chat, &token).await;
    let mut watching = authenticate(&address, chat, &token).await;

    deciding
        .send(WsMessage::Text(
            json!({"type": "approve_tool", "tool_call_id": "call_write", "approved": true})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let refusal = next_json(&mut deciding).await.expect("a refusal frame");
    assert_eq!(
        refusal,
        json!({"type": "tool_approval_closed", "tool_call_id": "call_write"}),
        "a refused decision names the call it refused and is not an error"
    );
    assert!(
        next_json(&mut watching).await.is_none(),
        "the refusal reaches the connection that decided and no other"
    );

    // The refusal closed a card, not the socket, so the connection still
    // answers for itself afterwards.
    deciding
        .send(WsMessage::Text("not-json".to_string().into()))
        .await
        .unwrap();
    assert_error(&mut deciding, "Invalid message format").await;

    deciding.close(None).await.unwrap();
    watching.close(None).await.unwrap();
}

#[tokio::test]
async fn a_send_after_membership_revocation_is_rejected_before_persistence() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, workspace, chat) = seed(&client).await;
    let user = validate_access_token(&token, &config.jwt_secret)
        .unwrap()
        .claims
        .user_id()
        .unwrap();
    let address = spawn(config, pool.clone()).await;
    let mut socket = authenticate(&address, chat, &token).await;

    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    socket
        .send(WsMessage::Text(
            json!({"type":"send","content":"This must not be stored"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    // The send path and the authorization tick both query membership, so either
    // can catch the revocation, and whichever does closes the socket -- which
    // can arrive with the error frame lost behind it. So a close counts as a
    // refusal, a refusal names one of the two, and anything the server would
    // send on the happy path fails. What must hold is the count below.
    if let Some(refusal) = next_json(&mut socket).await {
        assert_eq!(refusal["type"], "error", "{refusal}");
        assert!(
            matches!(
                refusal["message"].as_str().unwrap_or_default(),
                "Workspace access denied" | "Access revoked"
            ),
            "a revoked member's send has to be refused: {refusal}"
        );
    }
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE chat_id = $1")
        .bind(chat)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
    let _ = socket.close(None).await;
}

#[tokio::test]
async fn a_chat_moved_after_authentication_fails_closed_before_generation() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let (_, foreign_workspace, _) = seed(&client).await;
    let address = spawn(config, pool.clone()).await;
    let mut socket = authenticate(&address, chat, &token).await;

    sqlx::query("UPDATE chats SET workspace_id = $1 WHERE id = $2")
        .bind(foreign_workspace)
        .bind(chat)
        .execute(&pool)
        .await
        .unwrap();
    socket
        .send(WsMessage::Text(
            json!({"type":"send","content":"Do not cross the workspace boundary"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_error(
        &mut socket,
        "Conversation evidence was not found in this chat",
    )
    .await;
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn closing_before_authentication_releases_the_connection_slot() {
    let pool = create_test_pool().await;
    let address = spawn(test_config(), pool).await;
    let chat = Uuid::new_v4();
    let mut socket = connect(&address, chat).await;
    socket.close(None).await.unwrap();

    for _ in 0..5 {
        let mut replacement = connect(&address, chat).await;
        replacement.close(None).await.unwrap();
    }
}

#[tokio::test]
async fn idle_authenticated_connections_are_closed() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool).await;
    let mut socket = authenticate(&address, chat, &token).await;

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(301)).await;
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    tokio::time::resume();
    assert!(next_json(&mut socket).await.is_none());
}

#[tokio::test]
async fn periodic_authorization_recheck_disconnects_a_revoked_member() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, workspace, chat) = seed(&client).await;
    let user = validate_access_token(&token, &config.jwt_secret)
        .unwrap()
        .claims
        .user_id()
        .unwrap();
    let address = spawn(config, pool.clone()).await;
    let mut socket = authenticate(&address, chat, &token).await;
    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();

    // The authorization interval's first tick fires as the socket opens, before
    // the revocation above can land, so the second tick is the one that sees it.
    // Stop a millisecond short and resume: SQLx keeps its own Tokio deadlines,
    // so the query has to run on the real clock.
    tokio::time::pause();
    tokio::time::advance(AUTH_RECHECK - Duration::from_millis(1)).await;
    tokio::time::resume();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_error(&mut socket, "Access revoked").await;
    assert!(next_json(&mut socket).await.is_none());
}

#[tokio::test]
async fn periodic_authorization_recheck_keeps_an_active_member_connected() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool).await;
    let mut socket = authenticate(&address, chat, &token).await;

    tokio::time::pause();
    tokio::time::advance(AUTH_RECHECK - Duration::from_millis(1)).await;
    tokio::time::resume();
    tokio::time::sleep(Duration::from_millis(5)).await;

    socket
        .send(WsMessage::Text("not-json".to_string().into()))
        .await
        .unwrap();
    assert_error(&mut socket, "Invalid message format").await;
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn repeated_authorization_database_errors_close_an_unstable_connection() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool.clone()).await;
    let mut socket = authenticate(&address, chat, &token).await;
    pool.close().await;

    tokio::time::pause();
    // Every recheck against the closed pool is an error, so the report is
    // MAX_CONSECUTIVE_ERRORS authorization ticks away. The bound is patience,
    // not arithmetic -- the loop returns as soon as the error lands.
    let reported = periodic_error(&mut socket, 32).await;
    tokio::time::resume();
    assert_eq!(
        reported.as_deref(),
        Some("Connection unstable, please reconnect")
    );
    assert!(next_json(&mut socket).await.is_none());
}

#[tokio::test]
async fn action_delivery_rechecks_membership_before_forwarding() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, workspace, chat) = seed(&client).await;
    let user = validate_access_token(&token, &config.jwt_secret)
        .unwrap()
        .claims
        .user_id()
        .unwrap();
    let address = spawn(config, pool.clone()).await;
    let mut socket = authenticate(&address, chat, &token).await;

    let saved = Uuid::new_v4();
    actions::publish(
        chat,
        json!({
            "id": saved,
            "role": "assistant",
            "content": "A background action completed",
            "metadata": {"source": "contract-test"}
        }),
    );
    let delivered = next_json(&mut socket).await.expect("saved action delivery");
    assert_eq!(delivered["type"], "message_saved");
    assert_eq!(delivered["message_id"], saved.to_string());
    assert_eq!(delivered["metadata"]["source"], "contract-test");

    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    actions::publish(
        chat,
        json!({"id": Uuid::new_v4(), "role": "assistant", "content": "hidden"}),
    );
    // The authorization tick refuses the connection out loud before closing it,
    // so a refusal may arrive ahead of the close. What must never arrive is the
    // action itself.
    if let Some(frame) = next_json(&mut socket).await {
        assert_eq!(
            frame,
            json!({"type": "error", "message": "Access revoked"}),
            "a revoked member gets the refusal or nothing, never the action"
        );
        assert!(
            next_json(&mut socket).await.is_none(),
            "the refusal is the last thing a revoked member is told"
        );
    }
}

#[tokio::test]
async fn invalid_media_clients_fail_each_direct_lane_without_model_fallback() {
    let pool = create_test_pool().await;
    let mut config = test_config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = String::new();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool).await;
    let mut socket = authenticate(&address, chat, &token).await;

    for (flag, expected) in [
        ("image_generation", "Image generation is not configured:"),
        ("video_generation", "Video generation is not configured:"),
        ("audio_generation", "Audio generation is not configured:"),
        ("upscale", "Upscaling is not configured:"),
    ] {
        let metadata = serde_json::Map::from_iter([(flag.to_string(), Value::Bool(true))]);
        socket
            .send(WsMessage::Text(
                json!({
                    "type": "send",
                    "content": "Run the explicitly selected media lane",
                    "metadata": metadata
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        loop {
            let frame = next_json(&mut socket).await.expect("media terminal");
            if frame["type"] == "error" {
                let message = frame["message"].as_str().unwrap();
                assert!(message.starts_with(expected), "{message}");
                break;
            }
            assert_ne!(frame["type"], "message_start", "{frame}");
            assert_ne!(frame["type"], "message_end", "{frame}");
        }
    }
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn queued_media_stops_when_its_chat_lease_is_fenced() {
    let comfy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_json(json!({"prompt_id": "held"})),
        )
        .expect(1)
        .mount(&comfy)
        .await;

    let pool = create_test_pool().await;
    let mut config = test_config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = comfy.uri();
    config.comfyui.request_timeout_secs = 10;
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (first_token, _, first_chat) = seed(&client).await;
    let (second_token, _, second_chat) = seed(&client).await;
    let address = spawn(config, pool.clone()).await;
    let mut first = authenticate(&address, first_chat, &first_token).await;
    let mut second = authenticate(&address, second_chat, &second_token).await;

    first
        .send(WsMessage::Text(
            json!({
                "type": "send",
                "content": "Generate the image that holds the shared worker",
                "metadata": {"image_generation": true}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    loop {
        let frame = next_json(&mut first).await.expect("first image progress");
        if frame["type"] == "status" {
            break;
        }
        assert_ne!(frame["type"], "error", "{frame}");
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    second
        .send(WsMessage::Text(
            json!({
                "type": "send",
                "content": "Generate the image waiting behind the first",
                "metadata": {"image_generation": true}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    loop {
        let frame = next_json(&mut second).await.expect("queued image progress");
        if frame["type"] == "status" {
            break;
        }
        assert_ne!(frame["type"], "error", "{frame}");
    }

    let fenced = sqlx::query("UPDATE chat_leases SET fence = fence + 1 WHERE chat_id = $1")
        .bind(second_chat)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(fenced.rows_affected(), 1, "the queued job must own a lease");

    loop {
        let frame = next_json(&mut second).await.expect("lease-loss terminal");
        if frame["type"] == "error" {
            assert!(
                frame["message"]
                    .as_str()
                    .unwrap()
                    .contains("ownership expired or changed"),
                "{frame}"
            );
            break;
        }
        assert_ne!(frame["type"], "message_start", "{frame}");
        assert_ne!(frame["type"], "message_end", "{frame}");
    }

    first
        .send(WsMessage::Text(
            json!({"type": "cancel"}).to_string().into(),
        ))
        .await
        .unwrap();
    loop {
        let Some(frame) = next_json(&mut first).await else {
            break;
        };
        if frame["type"] == "cancelled" || frame["type"] == "error" {
            break;
        }
    }
    assert_eq!(
        comfy.received_requests().await.unwrap().len(),
        1,
        "the fenced queued job must not be submitted to ComfyUI"
    );
    first.close(None).await.unwrap();
    second.close(None).await.unwrap();
}

const QUESTION_HEADER: &str = "Scope";
const RECOMMENDED_LABEL: &str = "Backfill";
const OTHER_LABEL: &str = "Other";
const ANSWER: &str = "Scope: Backfill";
const AWAITING_ANSWER: &str = "[Waiting for your answer]";
const AWAITING_DETAIL: &str = "Waiting for your answer\u{2026}";

fn ask_user_arguments() -> String {
    json!({
        "questions": [{
            "header": QUESTION_HEADER,
            "question": "How far back should the backfill run?",
            "options": [
                {"label": RECOMMENDED_LABEL, "description": "Rewrite every existing row."},
                {"label": "Forward only", "description": "Leave existing rows alone."}
            ],
            "required": true
        }]
    })
    .to_string()
}

fn sse(deltas: Vec<Value>) -> String {
    let mut body = String::new();
    for delta in deltas {
        let chunk = json!({
            "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
            "choices": [{"index": 0, "delta": delta, "finish_reason": null}]
        });
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    let end = json!({
        "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    });
    body.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
    body
}

/// Mirrors `assert_replay` in the agent tests: every call from the parked turn
/// has to come back with exactly one tool reply, or the provider rejects the
/// transcript the answer is appended to.
fn assert_replay(request: &Value, count: usize) {
    let messages = request["messages"].as_array().unwrap();
    let calls: Vec<&Value> = messages
        .iter()
        .filter(|message| message["tool_calls"].is_array())
        .collect();
    assert_eq!(calls.len(), count, "{request}");
    for message in calls {
        for call in message["tool_calls"].as_array().unwrap() {
            let id = call["id"].as_str().unwrap();
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| message["role"] == "tool" && message["tool_call_id"] == id)
                    .count(),
                1,
                "call {id} was not answered exactly once: {request}"
            );
        }
    }
}

async fn drain_turn(socket: &mut Socket) -> Vec<Value> {
    let mut frames = Vec::new();
    loop {
        let frame = next_json(socket).await.expect("the turn must terminate");
        assert_ne!(frame["type"], "error", "{frame}");
        assert_ne!(frame["type"], "cancelled", "{frame}");
        let ended = frame["type"] == "message_end";
        frames.push(frame);
        if ended {
            return frames;
        }
    }
}

#[tokio::test]
async fn a_turn_ending_question_is_published_stored_and_answered_by_an_ordinary_send() {
    let provider = MockServer::start().await;
    let rounds = Arc::new(Mutex::new(VecDeque::from(vec![
        sse(vec![json!({"tool_calls": [{
            "index": 0,
            "id": "call_ask",
            "type": "function",
            "function": {"name": "ask_user", "arguments": ask_user_arguments()}
        }]})]),
        sse(vec![json!({"content": "Backfilling every row."})]),
    ])));
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |_request: &wiremock::Request| {
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    rounds
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("extra completion"),
                )
        })
        .mount(&provider)
        .await;

    let mut config = test_config();
    config.litellm_host = provider.uri();
    config.comfyui.enabled = false;
    config.web_search.enabled = false;
    let pool = create_test_pool().await;
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _workspace, chat) = seed(&client).await;
    client
        .put_json_auth(
            &format!("/api/chats/{chat}"),
            &json!({"agent_enabled": true, "agent_sandboxed": true}),
            &token,
        )
        .await
        .assert_status(axum::http::StatusCode::OK);
    let address = spawn(config, pool).await;
    let mut socket = authenticate(&address, chat, &token).await;

    socket
        .send(WsMessage::Text(
            json!({"type": "send", "content": "Change the column type."})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let frames = drain_turn(&mut socket).await;

    let cards: Vec<&Value> = frames
        .iter()
        .filter(|frame| frame["type"] == "question_required")
        .collect();
    assert_eq!(
        cards.len(),
        1,
        "exactly one card per question turn: {frames:?}"
    );
    let card = cards[0];
    let call = frames
        .iter()
        .find(|frame| frame["type"] == "tool_call")
        .expect("the card follows the call that asked");
    assert_eq!(card["message_id"], call["message_id"]);
    assert_eq!(card["tool_call_id"], call["tool_call_id"]);
    assert_eq!(card["questions"][0]["header"], QUESTION_HEADER);
    assert_eq!(
        card["questions"][0]["choices"][0]["label"],
        RECOMMENDED_LABEL
    );
    assert_eq!(card["questions"][0]["choices"][0]["recommended"], true);
    let last = card["questions"][0]["choices"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(last["label"], OTHER_LABEL);
    assert_eq!(last["free_text"], true);

    let position = frames
        .iter()
        .position(|frame| frame["type"] == "question_required")
        .unwrap();
    assert_eq!(frames.last().unwrap()["type"], "message_end");
    assert!(
        frames[position..]
            .iter()
            .all(|frame| frame["type"] != "chunk"),
        "a question ends the turn, so nothing is streamed after it: {frames:?}"
    );

    let history = client
        .get_auth(&format!("/api/chats/{chat}"), &token)
        .await
        .json_value();
    let stored = history["chat"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("the parked turn is kept")
        .clone();
    let record = &stored["metadata"]["tool_calls"][0];
    assert_eq!(record["name"], "ask_user");
    assert_eq!(record["detail"], AWAITING_DETAIL);
    assert_eq!(record["questions"][0]["header"], QUESTION_HEADER);
    assert_eq!(record["questions"][0]["choices"][0]["recommended"], true);
    assert_eq!(
        stored["content"], AWAITING_ANSWER,
        "a turn waiting on the reader is not a turn that stopped"
    );

    // The live frame log is cleared by message_end, so a reader who rejoins
    // rebuilds the card from the stored message rather than from a replay.
    let mut rejoined = authenticate(&address, chat, &token).await;
    assert!(
        next_json(&mut rejoined).await.is_none(),
        "a finished question turn replays no frames"
    );
    rejoined.close(None).await.unwrap();

    socket
        .send(WsMessage::Text(
            json!({"type": "send", "content": ANSWER})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let answered = drain_turn(&mut socket).await;
    assert!(
        answered
            .iter()
            .all(|frame| frame["type"] != "question_required"),
        "the answer starts an ordinary turn: {answered:?}"
    );
    socket.close(None).await.unwrap();

    let requests: Vec<Value> = provider
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path() == "/chat/completions")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    assert_eq!(requests.len(), 2, "one provider round per turn");
    let second = &requests[1];
    assert!(
        second["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "user" && message["content"] == ANSWER),
        "the answer reaches the model as an ordinary user turn: {second}"
    );
    assert_replay(second, 1);
}

const JOB_ID: &str = "job_0123456789ab";
const JOB_LOG_PATH: &str = ".zone/jobs/job_0123456789ab.log";
const SPAWNING_CALL: &str = "call_run_shell";
const WAITING_CALL: &str = "call_wait_for";
const DEADLINE: &str = "2026-09-13T18:30:00Z";

/// The four frames the job and wait cards are drawn from, serialised straight
/// off the variant rather than provoked through a socket.
///
/// Provoking one needs a model that backgrounds a command or opens a wait, and
/// a turn that does either belongs to the integration tests that drive one. The
/// tag a variant travels under and the exact set of keys beside it are narrower
/// than that and are what the console's schemas are written against, so they
/// are pinned here where no model is needed to read them.
fn job_started() -> JobStarted {
    JobStarted {
        id: JOB_ID.to_string(),
        pid: 4242,
        log_path: JOB_LOG_PATH.to_string(),
    }
}

#[test]
fn a_job_started_frame_names_the_call_that_spawned_it_and_the_job_it_spawned() {
    let message = Uuid::new_v4();

    let frame = serde_json::to_value(ServerMessage::JobStarted {
        message_id: message,
        tool_call_id: SPAWNING_CALL.to_string(),
        job: job_started(),
    })
    .expect("the frame serialises");

    assert_eq!(
        frame,
        json!({
            "type": "job_started",
            "message_id": message,
            "tool_call_id": SPAWNING_CALL,
            "job": {"id": JOB_ID, "pid": 4242, "log_path": JOB_LOG_PATH},
        })
    );
}

#[test]
fn a_job_exited_frame_is_matched_by_its_job_and_omits_an_exit_code_it_never_had() {
    let message = Uuid::new_v4();
    let exited = |exit_code| ServerMessage::JobExited {
        message_id: message,
        job: JobExited {
            id: JOB_ID.to_string(),
            exit_code,
        },
    };

    assert_eq!(
        serde_json::to_value(exited(Some(0))).expect("the frame serialises"),
        json!({
            "type": "job_exited",
            "message_id": message,
            "job": {"id": JOB_ID, "exit_code": 0},
        }),
        "no tool_call_id: the call that observes an exit is rarely the one that started the job"
    );
    assert_eq!(
        serde_json::to_value(exited(None)).expect("the frame serialises"),
        json!({
            "type": "job_exited",
            "message_id": message,
            "job": {"id": JOB_ID},
        }),
        "a killed job has no exit code, and the key is absent rather than null"
    );
}

#[test]
fn a_wait_started_frame_says_what_is_waited_on_and_until_when() {
    let message = Uuid::new_v4();
    let started = |reference| ServerMessage::WaitStarted {
        message_id: message,
        tool_call_id: WAITING_CALL.to_string(),
        waiting: Waiting {
            kind: "job".to_string(),
            id: JOB_ID.to_string(),
            reference,
            deadline: DEADLINE.to_string(),
        },
    };

    assert_eq!(
        serde_json::to_value(started(Some("cargo test --workspace".to_string())))
            .expect("the frame serialises"),
        json!({
            "type": "wait_started",
            "message_id": message,
            "tool_call_id": WAITING_CALL,
            "waiting": {
                "kind": "job",
                "id": JOB_ID,
                "reference": "cargo test --workspace",
                "deadline": DEADLINE,
            },
        })
    );
    assert_eq!(
        serde_json::to_value(started(None)).expect("the frame serialises"),
        json!({
            "type": "wait_started",
            "message_id": message,
            "tool_call_id": WAITING_CALL,
            "waiting": {"kind": "job", "id": JOB_ID, "deadline": DEADLINE},
        }),
        "a wait on something that needs no reference omits the key rather than sending null"
    );
}

#[test]
fn a_wait_settled_frame_carries_the_call_it_settles_inside_its_own_payload() {
    let message = Uuid::new_v4();

    let frame = serde_json::to_value(ServerMessage::WaitSettled {
        message_id: message,
        settled: WaitSettled {
            tool_call_id: WAITING_CALL.to_string(),
            outcome: "Job job_0123456789ab exited 0.".to_string(),
            timed_out: false,
        },
    })
    .expect("the frame serialises");

    assert_eq!(
        frame,
        json!({
            "type": "wait_settled",
            "message_id": message,
            "settled": {
                "tool_call_id": WAITING_CALL,
                "outcome": "Job job_0123456789ab exited 0.",
                "timed_out": false,
            },
        }),
        "the call id rides inside settled, not beside it: the console matches the card on it"
    );
}

/// Every status read Zone offers answers instantly, so a model told to look
/// again learns nothing between calls and spends a round per glance. Six
/// strings said exactly that, and they are the instructions nearest the moment
/// the decision is made, so they beat any prompt section that disagrees.
///
/// The needles are those six strings' own earlier wording. They are phrases no
/// assertion needs to spell, so inline test modules are swept too rather than
/// skipped; a test file is not, and this test's own needles sit outside every
/// swept root.
const SWEPT: [&str; 3] = [
    "runner/zone_server/src",
    "runner/zone_core/src",
    "manager/frontend/src",
];

const SOURCE_EXTENSIONS: [&str; 3] = ["rs", "ts", "tsx"];

const TEST_DIRECTORIES: [&str; 3] = ["test", "tests", "__tests__"];

const TEST_INFIXES: [&str; 2] = [".test.", ".spec."];

const POLLING: [&str; 5] = [
    "poll get_task_run",
    "poll tail_task_log",
    "poll get_build_status",
    "check again in a later call",
    "monitor start_task progress",
];

/// One of the six strings, where it lives, how to find it, and the name it has
/// to point at now. The two schema strings interpolate the tool's name from a
/// constant, which `WAIT_FOR_BINDING` holds to `wait_for` separately.
struct Rewritten {
    file: &'static str,
    opens: &'static str,
    closes: &'static str,
    names: &'static str,
}

const WAIT_FOR: &str = "wait_for";
const WAIT_FOR_PLACEHOLDER: &str = "{WAIT_FOR_TOOL}";
const WAIT_FOR_BINDING: &str = "const WAIT_FOR_TOOL: &str = \"wait_for\";";
const COMMAND_RS: &str = "runner/zone_core/src/tools/command.rs";
const ACTIONS_RS: &str = "runner/zone_server/src/agent/actions.rs";
const LITERAL_END: &str = "\";";
const FORMAT_END: &str = ")";

const REWRITTEN: [Rewritten; 6] = [
    Rewritten {
        file: "runner/zone_server/src/agent/prompt/section/workspace.rs",
        opens: "const START_TASK: &str = ",
        closes: LITERAL_END,
        names: WAIT_FOR,
    },
    Rewritten {
        file: ACTIONS_RS,
        opens: "const START_TASK_DESCRIPTION: &str = ",
        closes: LITERAL_END,
        names: WAIT_FOR,
    },
    Rewritten {
        file: ACTIONS_RS,
        opens: "const TAIL_TASK_LOG_DESCRIPTION: &str = ",
        closes: LITERAL_END,
        names: WAIT_FOR,
    },
    Rewritten {
        file: "runner/zone_server/src/db/actions.rs",
        opens: "const RUNNER_STARTED: &str = ",
        closes: LITERAL_END,
        names: WAIT_FOR,
    },
    Rewritten {
        file: COMMAND_RS,
        opens: "\"Shell command to run",
        closes: FORMAT_END,
        names: WAIT_FOR_PLACEHOLDER,
    },
    Rewritten {
        file: COMMAND_RS,
        opens: "\"This command sleeps for",
        closes: FORMAT_END,
        names: WAIT_FOR_PLACEHOLDER,
    },
];

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two directories under the repository")
        .to_path_buf()
}

fn is_test_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    TEST_INFIXES.iter().any(|infix| name.contains(infix))
        || name.ends_with("_test.rs")
        || name.ends_with("_tests.rs")
        || path.components().any(|component| {
            TEST_DIRECTORIES.contains(&component.as_os_str().to_str().unwrap_or(""))
        })
}

fn sources(directory: &Path, into: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("a directory entry is readable").path();
        if is_test_path(&path) {
            continue;
        }
        if path.is_dir() {
            sources(&path, into);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
        {
            into.push(path);
        }
    }
}

#[test]
fn nothing_still_tells_the_model_to_sample_a_status_in_a_loop() {
    let repository = repository();
    let mut swept = 0;

    for directory in SWEPT {
        let mut files = Vec::new();
        sources(&repository.join(directory), &mut files);
        assert!(
            !files.is_empty(),
            "{directory} contributed no source files, so the sweep proved nothing"
        );
        for file in &files {
            let source = fs::read_to_string(file)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", file.display()))
                .to_lowercase();
            for needle in POLLING {
                assert!(
                    !source.contains(needle),
                    "{} still says {needle:?}, which sends the model round the loop to look again",
                    file.display()
                );
            }
        }
        swept += files.len();
    }

    assert!(swept > 0, "the sweep walked nothing");
}

#[test]
fn every_rewritten_string_points_the_model_at_the_wait_tool() {
    let repository = repository();

    for rewritten in REWRITTEN {
        let path = repository.join(rewritten.file);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
        let opens = source.find(rewritten.opens).unwrap_or_else(|| {
            panic!(
                "{} no longer declares {:?}",
                rewritten.file, rewritten.opens
            )
        });
        let rest = &source[opens + rewritten.opens.len()..];
        let closes = rest.find(rewritten.closes).unwrap_or_else(|| {
            panic!(
                "{} does not close {:?} with {:?}",
                rewritten.file, rewritten.opens, rewritten.closes
            )
        });
        let string = &rest[..closes];

        assert!(
            string.contains(rewritten.names),
            "{} tells the model what to do after {:?} without naming {}: {string}",
            rewritten.file,
            rewritten.opens,
            rewritten.names
        );
    }

    let command = fs::read_to_string(repository.join(COMMAND_RS)).expect("command.rs is readable");
    assert!(
        command.contains(WAIT_FOR_BINDING),
        "the two schema strings name the tool through a constant, and that constant is what \
         binds the placeholder to {WAIT_FOR}"
    );
}
