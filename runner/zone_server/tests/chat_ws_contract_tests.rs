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
use zone_server::{
    auth::{jwt::Claims, validate_token},
    config::Config,
    db::{actions, chats},
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
    let invalid_claims = Claims {
        sub: "not-a-uuid".to_string(),
        email: test_email(),
        roles: Vec::new(),
        permissions: Vec::new(),
        exp: (now + ChronoDuration::minutes(5)).timestamp(),
        iat: now.timestamp(),
        jti: Uuid::new_v4().to_string(),
        is_admin: false,
    };
    let invalid_subject = encode(
        &Header::default(),
        &invalid_claims,
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
    assert_error(&mut socket, "That tool call is not waiting for approval.").await;

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

#[tokio::test]
async fn a_send_after_membership_revocation_is_rejected_before_persistence() {
    let pool = create_test_pool().await;
    let config = test_config();
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, workspace, chat) = seed(&client).await;
    let user = validate_token(&token, &config.jwt_secret)
        .unwrap()
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
    assert_error(&mut socket, "Workspace access denied").await;
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE chat_id = $1")
        .bind(chat)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
    socket.close(None).await.unwrap();
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
    let mut config = test_config();
    config.chat.recheck = 2;
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, workspace, chat) = seed(&client).await;
    let user = validate_token(&token, &config.jwt_secret)
        .unwrap()
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

    // The interval's first tick lands immediately, so a recheck budget of two
    // puts the query on the second tick and the single advance below decides
    // when it runs.
    tokio::time::pause();
    // Stop a millisecond short of the tick and resume: SQLx keeps its own
    // Tokio deadlines, so the query has to run on the real clock.
    tokio::time::advance(Duration::from_millis(29_999)).await;
    tokio::time::resume();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_error(&mut socket, "Access revoked").await;
    assert!(next_json(&mut socket).await.is_none());
}

#[tokio::test]
async fn periodic_authorization_recheck_keeps_an_active_member_connected() {
    let pool = create_test_pool().await;
    let mut config = test_config();
    config.chat.recheck = 2;
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool).await;
    let mut socket = authenticate(&address, chat, &token).await;

    // The interval's first tick lands immediately, so a recheck budget of two
    // puts the query on the second tick and the single advance below decides
    // when it runs.
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(29_999)).await;
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
    let mut config = test_config();
    config.chat.recheck = 1;
    let client = TestClient::new(create_test_router(create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let (token, _, chat) = seed(&client).await;
    let address = spawn(config, pool.clone()).await;
    let mut socket = authenticate(&address, chat, &token).await;
    pool.close().await;

    tokio::time::pause();
    // Rechecking on every tick puts the report MAX_CONSECUTIVE_ERRORS ticks
    // away. The bound is patience, not arithmetic -- the loop returns as soon
    // as the error lands.
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
    let user = validate_token(&token, &config.jwt_secret)
        .unwrap()
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
    assert!(
        next_json(&mut socket).await.is_none(),
        "revoked members must be disconnected before an action is forwarded"
    );
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
