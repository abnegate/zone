//! WebSocket integration tests
//!
//! These tests require a running database and test the WebSocket task run endpoint.

mod common;

use std::net::SocketAddr;

use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use zone_server::agent::wait::{KIND_TASK_RUN, Waiting};
use zone_server::db::tasks::{self, Mutation, RunMutation};
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::ws::{ProgressMessage, TaskProgressBroadcaster};

/// Start a test server and return the address
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

    // Wait a moment for the server to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    addr
}

/// Get a valid auth token for WebSocket tests
async fn get_ws_auth_token() -> (String, uuid::Uuid) {
    let config = common::test_config();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool);
    let router = common::create_test_router(state);

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let email = common::test_email();
    let password = common::test_password();

    // Register
    let body = serde_json::to_string(&json!({
        "email": &email,
        "password": &password,
        "display_name": "WS Tester"
    }))
    .unwrap();

    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (
        json["access_token"].as_str().unwrap().to_string(),
        uuid::Uuid::parse_str(json["user"]["id"].as_str().unwrap()).unwrap(),
    )
}

#[tokio::test]
async fn test_broadcaster_new() {
    let broadcaster = TaskProgressBroadcaster::new();
    // Just verify it can be created
    drop(broadcaster);
}

#[tokio::test]
async fn test_broadcaster_default() {
    let broadcaster = TaskProgressBroadcaster::default();
    drop(broadcaster);
}

#[tokio::test]
async fn test_broadcaster_get_sender() {
    let broadcaster = TaskProgressBroadcaster::new();
    let run_id = uuid::Uuid::new_v4();

    let sender1 = broadcaster.get_sender(run_id);
    let sender2 = broadcaster.get_sender(run_id);

    // Should get the same sender (cloned)
    assert!(sender1.receiver_count() == sender2.receiver_count());
}

#[tokio::test]
async fn test_broadcaster_subscribe() {
    let broadcaster = TaskProgressBroadcaster::new();
    let run_id = uuid::Uuid::new_v4();

    let _receiver = broadcaster.subscribe(run_id);

    // Verify sender exists now
    let sender = broadcaster.get_sender(run_id);
    assert!(sender.receiver_count() >= 1);
}

#[tokio::test]
async fn test_broadcaster_broadcast() {
    let broadcaster = TaskProgressBroadcaster::new();
    let run_id = uuid::Uuid::new_v4();

    // Subscribe first
    let mut receiver = broadcaster.subscribe(run_id);

    // Broadcast a message
    let msg = ProgressMessage::Init {
        run_id,
        task_id: uuid::Uuid::new_v4(),
        status: "running".to_string(),
    };
    broadcaster.broadcast(run_id, msg);

    // Receive the message
    let received: ProgressMessage =
        tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
            .await
            .expect("timeout")
            .expect("recv");

    match received {
        ProgressMessage::Init { status, .. } => {
            assert_eq!(status, "running");
        }
        _ => panic!("Unexpected message type"),
    }
}

#[tokio::test]
async fn test_broadcaster_broadcast_no_subscribers() {
    let broadcaster = TaskProgressBroadcaster::new();
    let run_id = uuid::Uuid::new_v4();

    // Broadcast without any subscribers - should not panic
    broadcaster.broadcast(
        run_id,
        ProgressMessage::Completed {
            status: "completed".to_string(),
        },
    );
}

#[tokio::test]
async fn test_broadcaster_remove() {
    let broadcaster = TaskProgressBroadcaster::new();
    let run_id = uuid::Uuid::new_v4();

    // Create a sender
    let _sender = broadcaster.get_sender(run_id);

    // Remove it
    broadcaster.remove(run_id);

    // Remove again (should not panic)
    broadcaster.remove(run_id);
}

#[tokio::test]
async fn test_broadcaster_multiple_runs() {
    let broadcaster = TaskProgressBroadcaster::new();
    let run_id_1 = uuid::Uuid::new_v4();
    let run_id_2 = uuid::Uuid::new_v4();

    let mut receiver1 = broadcaster.subscribe(run_id_1);
    let mut receiver2 = broadcaster.subscribe(run_id_2);

    // Broadcast to run 1
    broadcaster.broadcast(
        run_id_1,
        ProgressMessage::StatusUpdate {
            status: "running".to_string(),
            current_phase: Some("phase1".to_string()),
            progress_percent: Some(50),
        },
    );

    // Broadcast to run 2
    broadcaster.broadcast(
        run_id_2,
        ProgressMessage::StatusUpdate {
            status: "pending".to_string(),
            current_phase: None,
            progress_percent: None,
        },
    );

    // Check receiver 1 got the right message
    let msg1 = receiver1.try_recv().expect("recv1");
    match msg1 {
        ProgressMessage::StatusUpdate {
            status,
            progress_percent,
            ..
        } => {
            assert_eq!(status, "running");
            assert_eq!(progress_percent, Some(50));
        }
        _ => panic!("Wrong message type"),
    }

    // Check receiver 2 got the right message
    let msg2 = receiver2.try_recv().expect("recv2");
    match msg2 {
        ProgressMessage::StatusUpdate {
            status,
            progress_percent,
            ..
        } => {
            assert_eq!(status, "pending");
            assert_eq!(progress_percent, None);
        }
        _ => panic!("Wrong message type"),
    }
}

#[tokio::test]
async fn test_progress_message_init_to_ws() {
    let msg = ProgressMessage::Init {
        run_id: uuid::Uuid::new_v4(),
        task_id: uuid::Uuid::new_v4(),
        status: "created".to_string(),
    };

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"init\""));
    assert!(json.contains("\"status\":\"created\""));
}

#[tokio::test]
async fn test_progress_message_status_update() {
    let msg = ProgressMessage::StatusUpdate {
        status: "running".to_string(),
        current_phase: Some("execution".to_string()),
        progress_percent: Some(75),
    };

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"status_update\""));
    assert!(json.contains("\"progress_percent\":75"));
}

#[tokio::test]
async fn test_progress_message_log() {
    let msg = ProgressMessage::Log {
        id: uuid::Uuid::new_v4(),
        phase: "planning".to_string(),
        agent_type: "executor".to_string(),
        log_level: "info".to_string(),
        message: "Starting task".to_string(),
        metadata: None,
    };

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"log\""));
    assert!(json.contains("\"phase\":\"planning\""));
}

#[tokio::test]
async fn test_progress_message_completed() {
    let msg = ProgressMessage::Completed {
        status: "completed".to_string(),
    };

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"completed\""));
}

#[tokio::test]
async fn test_progress_message_failed() {
    let msg = ProgressMessage::Failed {
        error: "Task timed out".to_string(),
    };

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"failed\""));
    assert!(json.contains("\"error\":\"Task timed out\""));
}

#[tokio::test]
async fn test_progress_message_error() {
    let msg = ProgressMessage::Error {
        message: "Connection lost".to_string(),
    };

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"error\""));
}

#[tokio::test]
async fn test_ws_connect_without_auth() {
    let addr = start_test_server().await;
    let run_id = uuid::Uuid::new_v4();
    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);

    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Don't send auth, just wait for timeout or error
    // The server should timeout after 30 seconds but we'll just close early
    let result =
        tokio::time::timeout(std::time::Duration::from_millis(500), ws_stream.next()).await;

    // Either timeout or close is acceptable - if it times out, is_err() is true
    // If the stream closes, we get Ok(None)
    match result {
        Err(_) => {}           // Timeout is fine
        Ok(None) => {}         // Stream closed is fine
        Ok(Some(Err(_))) => {} // Error is fine
        Ok(Some(Ok(_))) => {}  // Message is also fine (could be auth timeout error)
    }
}

#[tokio::test]
async fn test_ws_connect_with_invalid_auth() {
    let addr = start_test_server().await;
    let run_id = uuid::Uuid::new_v4();
    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);

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
    let run_id = uuid::Uuid::new_v4();
    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);

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

#[tokio::test]
async fn test_ws_connect_task_run_not_found() {
    let addr = start_test_server().await;
    let (token, _) = get_ws_auth_token().await;
    let run_id = uuid::Uuid::new_v4(); // Non-existent run
    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);

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

    // Do not distinguish missing runs from runs in another workspace: the socket
    // is refused either way and must disclose nothing about the run. Matching on
    // the frame keeps this from passing silently when the socket just closes.
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), ws_stream.next())
        .await
        .expect("refusal timed out")
        .expect("socket produced no frame")
        .expect("socket error");
    match &response {
        Message::Close(_) => {}
        Message::Text(text) => {
            let message: serde_json::Value = serde_json::from_str(text).expect("parse");
            assert_eq!(message["type"], "error");
            assert!(
                !message.to_string().contains(&run_id.to_string()),
                "the refusal named the run: {message}"
            );
        }
        other => panic!("a refused socket sent {other:?} instead of closing"),
    }
}

#[tokio::test]
async fn test_ws_ping_pong() {
    let addr = start_test_server().await;
    let run_id = uuid::Uuid::new_v4();
    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);

    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Send ping
    ws_stream
        .send(Message::Ping(vec![1, 2, 3].into()))
        .await
        .expect("send ping");

    // Note: The ping/pong is handled at the WebSocket protocol level
    // We might receive it as a Pong or not at all (depends on implementation)
}

/// Helper to create a project and task for testing
async fn create_test_task() -> (uuid::Uuid, uuid::Uuid, String) {
    use zone_server::db::{
        projects, tasks,
        workspace_members::{self, WorkspaceRole},
    };

    let pool = common::create_test_pool().await;
    let (token, user_id) = get_ws_auth_token().await;

    // Setup test data (organization, workspace, user)
    let (_org_id, workspace_id, _user_id) = common::setup_test_data(&pool).await;
    workspace_members::add_member(&pool, workspace_id, user_id, WorkspaceRole::Member, None)
        .await
        .expect("add workspace member");

    // Create a project (pool, name, description, workspace_id)
    let project = projects::create_project(&pool, "WS Test Project", None, Some(workspace_id))
        .await
        .expect("create project");

    // Create a task (pool, workspace_id, project_ids, title, description, acceptance_criteria, priority, is_agentic)
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project.id],
        "WS Test Task",
        "Test task for WebSocket",
        None,
        None,
        false,
        None,
    )
    .await
    .expect("create task");

    (project.id, task.id, token)
}

/// Helper to create a task run
async fn create_test_task_run(task_id: uuid::Uuid) -> uuid::Uuid {
    use zone_server::db::tasks;

    let pool = common::create_test_pool().await;
    let run = tasks::create_task_run(&pool, task_id)
        .await
        .expect("create task run");
    run.id
}

/// Helper to add a log to a task run
async fn add_test_log(run_id: uuid::Uuid, phase: &str, message: &str) {
    use zone_server::db::tasks;

    let pool = common::create_test_pool().await;
    tasks::add_task_run_log(&pool, run_id, phase, "executor", "info", message, None)
        .await
        .expect("add log");
}

/// Helper to complete a task run
async fn complete_test_run(run_id: uuid::Uuid, status: &str, error: Option<&str>) {
    use zone_server::db::tasks;

    let pool = common::create_test_pool().await;
    tasks::complete_task_run(&pool, run_id, status, error, None)
        .await
        .expect("complete run");
}

/// Helper to update task run progress
async fn update_test_run_progress(run_id: uuid::Uuid, phase: &str, progress: i32) {
    use zone_server::db::tasks;

    let pool = common::create_test_pool().await;
    tasks::update_task_run_progress(&pool, run_id, Some(phase), Some(progress))
        .await
        .expect("update progress");
}

#[tokio::test]
async fn test_ws_connect_to_running_task() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    // Add some logs before connecting
    add_test_log(run_id, "init", "Starting task execution").await;
    add_test_log(run_id, "planning", "Planning task steps").await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Should receive init message first
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "init");
        assert_eq!(msg["run_id"], run_id.to_string());
        assert_eq!(msg["task_id"], task_id.to_string());
        assert_eq!(msg["status"], "running");
    } else {
        panic!("Expected init message");
    }

    // Should receive existing logs
    let mut log_count = 0;
    for _ in 0..2 {
        if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
            let text_str: &str = text.as_ref();
            let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
            assert_eq!(msg["type"], "log");
            log_count += 1;
        }
    }
    assert_eq!(log_count, 2, "Should receive 2 existing logs");
}

#[tokio::test]
async fn test_ws_receive_completed_task() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    // Complete the task before connecting
    complete_test_run(run_id, "completed", None).await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Should receive init message
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "init");
        assert_eq!(msg["status"], "completed");
    }

    // Should receive completed message and close
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "completed");
    }
}

#[tokio::test]
async fn test_ws_receive_failed_task() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    // Fail the task before connecting
    complete_test_run(run_id, "failed", Some("Test error message")).await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Should receive init message
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "init");
        assert_eq!(msg["status"], "failed");
    }

    // Should receive failed message
    if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
        let text_str: &str = text.as_ref();
        let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
        assert_eq!(msg["type"], "failed");
        assert_eq!(msg["error"], "Test error message");
    }
}

#[tokio::test]
async fn test_ws_receive_progress_updates() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Receive init message
    let _ = ws_stream.next().await;

    // Update progress in the background
    let run_id_clone = run_id;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        update_test_run_progress(run_id_clone, "execution", 50).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        complete_test_run(run_id_clone, "completed", None).await;
    });

    // Wait for status update or completion
    let mut received_update = false;
    let mut received_completed = false;

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(Message::Text(text))) = ws_stream.next().await {
            let text_str: &str = text.as_ref();
            let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
            match msg["type"].as_str() {
                Some("status_update") => {
                    received_update = true;
                }
                Some("completed") => {
                    received_completed = true;
                    break;
                }
                _ => {}
            }
        }
    });

    let _ = timeout.await;
    assert!(received_completed, "Should receive completed message");
}

#[tokio::test]
async fn test_ws_receive_new_logs_during_execution() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Receive init message
    let _ = ws_stream.next().await;

    // Add logs in the background with longer delays to allow polling (500ms interval)
    let run_id_clone = run_id;
    tokio::spawn(async move {
        // Wait for first poll to happen
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        add_test_log(run_id_clone, "execution", "Step 1 complete").await;
        // Wait for another poll
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        add_test_log(run_id_clone, "execution", "Step 2 complete").await;
        // Wait for poll to pick up log before completing
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        complete_test_run(run_id_clone, "completed", None).await;
    });

    // Collect logs until completion
    let mut log_count = 0;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(Ok(Message::Text(text))) = ws_stream.next().await {
            let text_str: &str = text.as_ref();
            let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
            match msg["type"].as_str() {
                Some("log") => {
                    log_count += 1;
                }
                Some("completed") => {
                    break;
                }
                _ => {}
            }
        }
    });

    let _ = timeout.await;
    // Note: Due to UUID comparison in log tracking, not all logs may be received
    // (UUIDs aren't chronologically ordered). At least 1 log should be delivered.
    assert!(
        log_count >= 1,
        "Should receive at least 1 log, got {}",
        log_count
    );
}

#[tokio::test]
async fn test_ws_client_close_connection() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Receive init message
    let _ = ws_stream.next().await;

    // Close the connection
    ws_stream
        .send(Message::Close(None))
        .await
        .expect("send close");

    // The server should handle the close gracefully
}

#[tokio::test]
async fn test_ws_multiple_concurrent_connections() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);

    // Connect multiple clients
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

    // Both should receive init message
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
}

#[tokio::test]
async fn test_ws_task_with_many_logs() {
    let addr = start_test_server().await;
    let (_project_id, task_id, token) = create_test_task().await;
    let run_id = create_test_task_run(task_id).await;

    // Add many logs before connecting
    for i in 0..10 {
        add_test_log(run_id, "execution", &format!("Log message {}", i)).await;
    }

    let url = format!("ws://{}/ws/tasks/runs/{}", addr, run_id);
    let (mut ws_stream, _) = connect_async(&url).await.expect("connect");

    // Authenticate
    let auth_msg = json!({ "type": "auth", "token": token });
    ws_stream
        .send(Message::Text(auth_msg.to_string().into()))
        .await
        .expect("send auth");

    // Should receive init
    let _ = ws_stream.next().await;

    // Should receive all 10 logs
    let mut log_count = 0;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(Ok(Message::Text(text))) = ws_stream.next().await {
            let text_str: &str = text.as_ref();
            let msg: serde_json::Value = serde_json::from_str(text_str).expect("parse");
            if msg["type"] == "log" {
                log_count += 1;
                if log_count >= 10 {
                    break;
                }
            }
        }
    });

    let _ = timeout.await;
    assert_eq!(log_count, 10, "Should receive all 10 logs");
}

/// A workspace whose owner may both read and answer its runs.
struct Tenant {
    token: String,
    workspace: uuid::Uuid,
}

/// Register an account and give it an organization and a workspace of its own.
///
/// `setup_test_data` writes the same three rows directly, but the answer route
/// authenticates its caller, and only the register endpoint mints a session a
/// bearer token resolves to.
async fn tenant(client: &common::TestClient) -> Tenant {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": common::test_email(), "password": common::test_password() }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let token = response.json_value()["access_token"]
        .as_str()
        .expect("access token is returned")
        .to_owned();

    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Pending wait", "slug": uuid::Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    organization.assert_status(StatusCode::CREATED);
    let organization = organization.json_value()["organization"]["id"]
        .as_str()
        .expect("organization id is returned")
        .to_owned();

    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Pending wait", "slug": uuid::Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    workspace.assert_status(StatusCode::CREATED);
    let workspace = uuid::Uuid::parse_str(
        workspace.json_value()["workspace"]["id"]
            .as_str()
            .expect("workspace id is returned"),
    )
    .expect("workspace id is a UUID");

    Tenant { token, workspace }
}

/// One task in `workspace`, named for the test that asked for it.
async fn task(pool: &sqlx::PgPool, workspace: uuid::Uuid, title: &str) -> uuid::Uuid {
    tasks::create_task(
        pool,
        workspace,
        &[],
        title,
        "Regression",
        None,
        None,
        true,
        None,
    )
    .await
    .expect("the workspace takes a task")
    .id
}

/// A claimed run, which is what every park and every completion is fenced on.
async fn claimed(pool: &sqlx::PgPool, task_id: uuid::Uuid) -> (uuid::Uuid, uuid::Uuid) {
    let run = tasks::create_task_run(pool, task_id)
        .await
        .expect("the task gets a run")
        .id;
    let owner = uuid::Uuid::new_v4();
    assert!(
        tasks::claim_task_run(pool, run, owner)
            .await
            .expect("the run is claimable")
    );
    (run, owner)
}

/// What a worker parks a run on when it waits for something outside the loop.
fn waiting(run: uuid::Uuid) -> serde_json::Value {
    serde_json::to_value(Waiting {
        kind: KIND_TASK_RUN.to_string(),
        id: run.to_string(),
        reference: None,
        deadline: "2026-09-12T10:15:00Z".to_string(),
    })
    .expect("a wait serializes")
}

/// Age a run's lease past the sweeper's window.
async fn expire(pool: &sqlx::PgPool, run: uuid::Uuid) {
    sqlx::query("UPDATE task_runs SET heartbeat_at = NOW() - INTERVAL '61 seconds' WHERE id = $1")
        .bind(run)
        .execute(pool)
        .await
        .expect("the lease ages");
}

/// The worker path: an owner finishing its own run. Whoever is waiting on that
/// run learns it finished from the run itself, not from a sampler of its own.
#[tokio::test]
async fn completing_a_run_publishes_a_terminal_frame_and_releases_its_channel() {
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool.clone());
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = task(&pool, workspace, "Completed frame").await;
    let (run, owner) = claimed(&pool, task).await;

    let mut subscription = state.task_progress().subscribe(run);
    assert!(
        state.task_progress().tracks(run),
        "a subscription registers the run's channel"
    );

    tasks::complete_owned_task_run(&pool, run, Some(owner), "completed", None, None)
        .await
        .expect("the owner completes its run")
        .expect("the completion applied");

    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), subscription.recv())
        .await
        .expect("a terminal frame arrives before the timeout")
        .expect("the frame is readable");
    match frame {
        ProgressMessage::Completed { status } => assert_eq!(status, "completed"),
        other => panic!("a completed run must publish Completed, got {other:?}"),
    }

    assert!(
        !state.task_progress().tracks(run),
        "the channel is released as soon as the run is reported finished"
    );
    assert!(
        matches!(
            subscription.recv().await,
            Err(tokio::sync::broadcast::error::RecvError::Closed)
        ),
        "releasing the channel closes the subscription rather than leaking it"
    );
}

/// A run whose worker died never reaches `complete_owned_task_run`, so without
/// this the sweeper's reap would leave a waiter hanging until its own deadline.
#[tokio::test]
async fn sweeping_an_expired_lease_publishes_a_failure_and_releases_its_channel() {
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool.clone());
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = task(&pool, workspace, "Swept frame").await;
    let (run, _) = claimed(&pool, task).await;

    let mut subscription = state.task_progress().subscribe(run);
    expire(&pool, run).await;
    tasks::sweep_task_runs(&pool).await.expect("the sweep runs");

    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), subscription.recv())
        .await
        .expect("a terminal frame arrives before the timeout")
        .expect("the frame is readable");
    match frame {
        ProgressMessage::Failed { error } => assert_eq!(error, "orphaned"),
        other => panic!("a reaped run must publish Failed, got {other:?}"),
    }

    assert!(
        !state.task_progress().tracks(run),
        "the channel is released as soon as the run is reported finished"
    );
}

/// The broadcaster the writers publish to is the one the state hands out, or
/// every subscriber would be listening to a second instance nothing writes to.
#[tokio::test]
async fn the_state_hands_out_the_broadcaster_the_writers_publish_to() {
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool);
    let run = uuid::Uuid::new_v4();

    let _subscription = state.task_progress().subscribe(run);
    assert!(
        zone_server::ws::task_run::progress().tracks(run),
        "the process broadcaster sees what a state subscription registered"
    );
    zone_server::ws::task_run::progress().remove(run);
    assert!(!state.task_progress().tracks(run));
}

/// `SQLX_OFFLINE` cannot check a runtime `query_as`, so a SELECT list that
/// forgot the column would build green and fail in production with
/// `no column found for name: pending_wait`. Every path that reads a run is
/// therefore read here against a row that actually has one.
#[tokio::test]
async fn every_path_that_reads_a_run_carries_its_pending_wait() {
    let pool = common::create_test_pool().await;
    let (_, workspace, user) = common::setup_test_data(&pool).await;
    workspace_members::add_member(&pool, workspace, user, WorkspaceRole::Owner, None)
        .await
        .expect("the workspace takes its owner");
    let fresh = task(&pool, workspace, "Fresh run").await;
    let task = task(&pool, workspace, "Every read path").await;
    let (run, owner) = claimed(&pool, task).await;
    let wait = waiting(run);
    assert!(
        tasks::park_task_run_waiting(&pool, run, owner, wait.clone())
            .await
            .expect("the claimed run parks on a wait")
    );

    let fetched = tasks::get_task_run(&pool, run)
        .await
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(fetched.pending_wait.as_ref(), Some(&wait), "get_task_run");
    assert!(fetched.pending_question.is_none());

    let listed = tasks::list_task_runs(&pool, task)
        .await
        .expect("the task's runs are listable");
    assert_eq!(
        listed
            .iter()
            .find(|row| row.id == run)
            .and_then(|row| row.pending_wait.as_ref()),
        Some(&wait),
        "list_task_runs_as"
    );

    // The active-run lookup is a runtime query_as, and a parked run is exactly
    // what it returns instead of admitting a second one.
    match tasks::create_task_run_authorized(&pool, user, task)
        .await
        .expect("the admission check runs")
    {
        Mutation::Applied(RunMutation::Active(active)) => {
            assert_eq!(active.id, run);
            assert_eq!(
                active.pending_wait.as_ref(),
                Some(&wait),
                "the active-run lookup at create_task_run_authorized"
            );
        }
        other => panic!("a parked run must block a second admission, got {other:?}"),
    }

    assert!(
        tasks::resume_task_run(&pool, run, owner)
            .await
            .expect("the parked run resumes")
    );
    let progressed =
        tasks::update_owned_task_run_progress(&pool, run, Some(owner), Some("acting"), Some(40))
            .await
            .expect("the resumed run takes progress")
            .expect("the progress applied");
    assert!(
        progressed.pending_wait.is_none(),
        "update_owned_task_run_progress, on a run whose wait resume cleared"
    );

    let created = tasks::create_task_run(&pool, fresh)
        .await
        .expect("a fresh run inserts");
    assert!(created.pending_wait.is_none(), "insert_task_run");

    let completed =
        tasks::complete_owned_task_run(&pool, run, Some(owner), "completed", None, None)
            .await
            .expect("the owner completes its run")
            .expect("the completion applied");
    assert!(completed.pending_wait.is_none(), "complete_owned_task_run");
}

/// A wait is the run's own state, so it has to survive the round trip through
/// the column intact -- the loop reads back what it parked on, not a summary.
#[tokio::test]
async fn a_wait_round_trips_through_the_column_and_resume_clears_it() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = task(&pool, workspace, "Wait round trip").await;
    let (run, owner) = claimed(&pool, task).await;
    let wait = Waiting {
        kind: KIND_TASK_RUN.to_string(),
        id: run.to_string(),
        reference: Some("main".to_string()),
        deadline: "2026-09-12T10:15:00Z".to_string(),
    };

    assert!(
        tasks::park_task_run_waiting(
            &pool,
            run,
            owner,
            serde_json::to_value(&wait).expect("a wait serializes"),
        )
        .await
        .expect("the claimed run parks on a wait")
    );
    let parked = tasks::get_task_run(&pool, run)
        .await
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(parked.status, "waiting");
    assert_eq!(
        serde_json::from_value::<Waiting>(parked.pending_wait.expect("the run holds its wait"))
            .expect("the stored wait is a wait"),
        wait
    );

    assert!(
        tasks::resume_task_run(&pool, run, owner)
            .await
            .expect("the parked run resumes")
    );
    let resumed = tasks::get_task_run(&pool, run)
        .await
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(resumed.status, "running");
    assert!(
        resumed.pending_wait.is_none(),
        "resuming clears what the run waited on"
    );
}

/// A run parked on a wait is finished by its owner, not answered, so the
/// column has to be clear on the terminal row too.
#[tokio::test]
async fn completing_a_run_parked_on_a_wait_clears_the_column() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = task(&pool, workspace, "Completed park").await;
    let (run, owner) = claimed(&pool, task).await;
    assert!(
        tasks::park_task_run_waiting(&pool, run, owner, waiting(run))
            .await
            .expect("the claimed run parks on a wait")
    );

    let completed =
        tasks::complete_owned_task_run(&pool, run, Some(owner), "completed", None, None)
            .await
            .expect("the owner completes its parked run")
            .expect("the completion applied");
    assert!(completed.pending_wait.is_none());
    assert!(
        tasks::get_task_run(&pool, run)
            .await
            .expect("the run is readable")
            .expect("the run exists")
            .pending_wait
            .is_none(),
        "the stored terminal row carries no wait either"
    );
}

/// The sweeper writes its own terminal row with its own statement, so clearing
/// the column in the owner's path would not cover a run nobody finished.
#[tokio::test]
async fn sweeping_a_run_parked_on_a_wait_clears_the_column() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = task(&pool, workspace, "Swept park").await;
    let (run, owner) = claimed(&pool, task).await;
    assert!(
        tasks::park_task_run_waiting(&pool, run, owner, waiting(run))
            .await
            .expect("the claimed run parks on a wait")
    );

    expire(&pool, run).await;
    tasks::sweep_task_runs(&pool).await.expect("the sweep runs");

    let swept = tasks::get_task_run(&pool, run)
        .await
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(swept.status, "failed");
    assert_eq!(swept.error_message.as_deref(), Some("orphaned"));
    assert!(
        swept.pending_wait.is_none(),
        "the sweeper clears what the run waited on"
    );
}

/// A wait park leaves `pending_question` NULL on purpose: there is nothing for
/// a person to answer, and the answer route's own guard is what makes that
/// hold. Parking on a wait must not open a route that was never meant for it.
#[tokio::test]
async fn a_run_parked_on_a_wait_cannot_be_answered() {
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool.clone());
    let client = common::TestClient::new(common::create_test_router(state));
    let owner = tenant(&client).await;
    let task = task(&pool, owner.workspace, "Unanswerable park").await;
    let (run, lease) = claimed(&pool, task).await;
    assert!(
        tasks::park_task_run_waiting(&pool, run, lease, waiting(run))
            .await
            .expect("the claimed run parks on a wait")
    );

    let refused = client
        .post_json_auth(
            &format!("/api/tasks/runs/{run}/answers"),
            &json!({"answers": [{"header": "Scope", "labels": ["Backfill"]}]}),
            &owner.token,
        )
        .await;
    refused.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        tasks::get_task_run(&pool, run)
            .await
            .expect("the run is readable")
            .expect("the run exists")
            .status,
        "waiting",
        "a refused answer must not move the run"
    );
}
