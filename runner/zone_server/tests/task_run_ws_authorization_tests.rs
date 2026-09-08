mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use tower::ServiceExt;
use uuid::Uuid;
use zone_server::db::{sessions, tasks, workspace_members};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct Credentials {
    access: String,
    refresh: String,
    session: Uuid,
    user: Uuid,
}

struct Fixture {
    address: SocketAddr,
    credentials: Credentials,
    pool: PgPool,
    router: Router,
    run: Uuid,
    workspace: Uuid,
}

async fn register(router: &Router, pool: &PgPool) -> Credentials {
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "email": common::test_email(),
                "password": common::test_password(),
                "display_name": "Task stream tester"
            })
            .to_string(),
        ))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let response: Value = serde_json::from_slice(&body).unwrap();
    let user = Uuid::parse_str(response["user"]["id"].as_str().unwrap()).unwrap();
    let session = sqlx::query_scalar(
        "SELECT id FROM sessions WHERE user_id = $1 AND revoked_at IS NULL ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap();
    let access = response["access_token"].as_str().unwrap().to_string();
    let binding =
        zone_server::auth::validate_access_token(&access, common::test_config().jwt_secret())
            .unwrap();
    assert_eq!(binding.session_id, Some(session));

    Credentials {
        access,
        refresh: response["refresh_token"].as_str().unwrap().to_string(),
        session,
        user,
    }
}

async fn fixture(lifetime: u64) -> Fixture {
    let mut config = common::test_config();
    config.jwt_access_lifetime = lifetime;
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool.clone());
    let router = common::create_test_router(state);
    let credentials = register(&router, &pool).await;

    let (_, workspace, _) = common::setup_test_data(&pool).await;
    workspace_members::add_member(
        &pool,
        workspace,
        credentials.user,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .unwrap();
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Authorization fixture",
        "Protect the task stream",
        None,
        None,
        false,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap().id;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = router.clone();
    tokio::spawn(async move {
        axum::serve(listener, server).await.unwrap();
    });

    Fixture {
        address,
        credentials,
        pool,
        router,
        run,
        workspace,
    }
}

async fn connect(address: SocketAddr, run: Uuid, token: &str) -> Socket {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/tasks/runs/{run}"))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({ "type": "auth", "token": token }).to_string().into(),
        ))
        .await
        .unwrap();
    socket
}

async fn message(socket: &mut Socket) -> Option<Value> {
    match tokio::time::timeout(Duration::from_secs(3), socket.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => Some(serde_json::from_str(&text).unwrap()),
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => None,
        Ok(Some(Ok(message))) => panic!("unexpected WebSocket message: {message:?}"),
        Ok(Some(Err(error))) => panic!("WebSocket error: {error}"),
        Err(_) => panic!("timed out waiting for WebSocket authorization result"),
    }
}

async fn assert_denied(socket: &mut Socket) {
    if let Some(message) = message(socket).await {
        assert_eq!(
            message["type"], "error",
            "sensitive event leaked: {message}"
        );
    }
}

async fn expect_init(socket: &mut Socket) {
    let initial = message(socket).await.expect("expected initial task state");
    assert_eq!(initial["type"], "init");
}

async fn add_log(pool: &PgPool, run: Uuid) {
    tasks::add_task_run_log(
        pool,
        run,
        "execution",
        "executor",
        "info",
        "sensitive task output",
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn foreign_user_cannot_stream_a_task_run() {
    let fixture = fixture(900).await;
    let foreign = register(&fixture.router, &fixture.pool).await;
    let mut socket = connect(fixture.address, fixture.run, &foreign.access).await;

    assert_denied(&mut socket).await;
}

#[tokio::test]
async fn refresh_token_cannot_authenticate_a_task_run_stream() {
    let fixture = fixture(900).await;
    let mut socket = connect(fixture.address, fixture.run, &fixture.credentials.refresh).await;

    assert_denied(&mut socket).await;
}

#[tokio::test]
async fn rotated_refresh_token_cannot_authenticate_a_task_run_stream() {
    let fixture = fixture(900).await;
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/refresh")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "refresh_token": fixture.credentials.refresh }).to_string(),
        ))
        .unwrap();
    let response = fixture.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let mut socket = connect(fixture.address, fixture.run, &fixture.credentials.refresh).await;
    assert_denied(&mut socket).await;
}

#[tokio::test]
async fn membership_revocation_stops_an_established_task_run_stream() {
    let fixture = fixture(900).await;
    let mut socket = connect(fixture.address, fixture.run, &fixture.credentials.access).await;
    expect_init(&mut socket).await;

    workspace_members::remove_member(&fixture.pool, fixture.workspace, fixture.credentials.user)
        .await
        .unwrap();
    add_log(&fixture.pool, fixture.run).await;

    assert_denied(&mut socket).await;
}

#[tokio::test]
async fn session_revocation_stops_an_established_task_run_stream() {
    let fixture = fixture(900).await;
    let mut socket = connect(fixture.address, fixture.run, &fixture.credentials.access).await;
    expect_init(&mut socket).await;

    sessions::revoke_session(&fixture.pool, fixture.credentials.session)
        .await
        .unwrap();
    add_log(&fixture.pool, fixture.run).await;

    assert_denied(&mut socket).await;
}

#[tokio::test]
async fn token_expiry_stops_an_established_task_run_stream() {
    let fixture = fixture(900).await;
    let access = zone_server::auth::create_session_access_token(
        fixture.credentials.user,
        "expiry@example.com",
        vec![],
        vec![],
        false,
        fixture.credentials.session,
        common::test_config().jwt_secret(),
        chrono::Duration::seconds(2),
    )
    .unwrap();
    let mut socket = connect(fixture.address, fixture.run, &access).await;
    expect_init(&mut socket).await;

    tokio::time::sleep(Duration::from_secs(3)).await;
    add_log(&fixture.pool, fixture.run).await;

    assert_denied(&mut socket).await;
}

#[tokio::test]
async fn database_failure_stops_an_established_task_run_stream() {
    let fixture = fixture(900).await;
    let mut socket = connect(fixture.address, fixture.run, &fixture.credentials.access).await;
    expect_init(&mut socket).await;

    fixture.pool.close().await;

    assert_denied(&mut socket).await;
}
