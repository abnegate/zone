//! Background tasks retain their initiating workspace and never expose foreign runs.
mod common;

use chrono::{Duration, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;
use zone_server::agent::ChatTools;
use zone_server::auth::jwt::create_session_access_token;
use zone_server::db::{sessions, task_access, tasks, workspace_members};
use zone_server::state::AppState;

/// Mint an access token backed by a live session.
///
/// Auth refuses a token whose session is missing or revoked, so a raw JWT is
/// no longer enough to reach these routes: every actor needs a session of its
/// own, the way a real sign-in would leave one.
async fn session_token(pool: &PgPool, actor: Uuid, secret: &str) -> String {
    let session = sessions::create_session(
        pool,
        actor,
        &format!("refresh-{}", Uuid::new_v4()),
        None,
        None,
        None,
        (Utc::now() + Duration::hours(1)).naive_utc(),
    )
    .await
    .expect("the actor gets a session");
    create_session_access_token(
        actor,
        "scope@example.com",
        vec![],
        vec![],
        false,
        session.id,
        secret,
        Duration::minutes(1),
    )
    .expect("the session token is signed")
}

async fn fixture() -> (PgPool, AppState, Uuid, Uuid, Uuid) {
    let pool = common::create_test_pool().await;
    let (organization, workspace, user) = common::setup_test_data(&pool).await;
    workspace_members::add_member(
        &pool,
        workspace,
        user,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .unwrap();
    let state = common::create_test_state(common::test_config(), pool.clone());
    (pool, state, organization, workspace, user)
}

async fn cleanup(pool: &PgPool, organization: Uuid, user: Uuid) {
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_scope_checks_writer_each_time_and_never_invents_a_chat() {
    let (pool, state, organization, workspace, user) = fixture().await;
    let tools = ChatTools::for_task(&state, std::env::temp_dir(), workspace, Some(user)).await;
    for name in [
        "list_tasks",
        "create_task",
        "create_document",
        "list_projects",
        "list_sources",
    ] {
        assert!(tools.names().contains(&name.to_string()), "missing {name}");
    }
    for name in [
        "send_message",
        "create_reminder",
        "generate_image",
        "generate_audio",
        "read_chat_evidence",
    ] {
        assert!(
            !tools.names().contains(&name.to_string()),
            "chat-only {name}"
        );
    }
    assert!(
        !tools
            .execute(
                "search_chat_history",
                r#"{"query":"scope","this_chat_only":true}"#
            )
            .await
            .success
    );
    let result = tools
        .execute(
            "create_task",
            r#"{"title":"Scoped action","description":"Written as the initiating actor"}"#,
        )
        .await;
    assert!(result.success, "{:?}", result.error);
    let written: Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
    assert_eq!(written["workspace_id"], workspace.to_string());
    assert_eq!(written["created_by"], user.to_string());
    let (_, _, foreign_organization, foreign_workspace, foreign_user) = fixture().await;
    let foreign =
        ChatTools::for_task(&state, std::env::temp_dir(), foreign_workspace, Some(user)).await;
    assert!(!foreign.names().contains(&"create_document".into()));
    sqlx::query(
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    assert!(!tools.execute("list_tasks", "{}").await.success);
    let inactive = ChatTools::for_task(&state, std::env::temp_dir(), workspace, Some(user)).await;
    assert!(!inactive.names().contains(&"create_task".into()));
    sqlx::query(
        "UPDATE workspace_members SET is_active = true WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    cleanup(&pool, foreign_organization, foreign_user).await;

    sqlx::query(
        "UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        !tools
            .execute("create_task", r#"{"title":"Denied"}"#)
            .await
            .success
    );
    let viewer = ChatTools::for_task(&state, std::env::temp_dir(), workspace, Some(user)).await;
    assert!(!viewer.names().contains(&"create_task".into()));
    workspace_members::remove_member(&pool, workspace, user)
        .await
        .unwrap();
    assert!(!tools.execute("list_tasks", "{}").await.success);
    let missing = ChatTools::for_task(&state, std::env::temp_dir(), workspace, Some(user)).await;
    let legacy = ChatTools::for_task(&state, std::env::temp_dir(), workspace, None).await;
    let foreign =
        ChatTools::for_task(&state, std::env::temp_dir(), Uuid::new_v4(), Some(user)).await;
    for denied in [missing, legacy, foreign] {
        assert!(!denied.names().contains(&"create_document".into()));
        assert!(denied.names().contains(&"read_file".into()));
    }
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let deleted = ChatTools::for_task(&state, std::env::temp_dir(), workspace, Some(user)).await;
    assert!(!deleted.names().contains(&"create_task".into()));
    cleanup(&pool, organization, user).await;
}

async fn websocket_case(status: &str, authorized: bool, token_valid: bool) {
    let (pool, state, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Secret task",
        "secret content",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    tasks::add_task_run_log(
        &pool,
        run.id,
        "acting",
        "tool",
        "info",
        "DO NOT LEAK",
        Some(json!({"action_receipt": {"action": "create_task", "actor_id": user}})),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE task_runs SET status = $2 WHERE id = $1")
        .bind(run.id)
        .bind(status)
        .execute(&pool)
        .await
        .unwrap();
    let config = common::test_config();
    let (_, _, foreign_organization, foreign_workspace, foreign_user) = fixture().await;
    assert_ne!(workspace, foreign_workspace);
    let actor = if authorized { user } else { foreign_user };
    let token = if token_valid {
        session_token(&pool, actor, &config.jwt_secret).await
    } else {
        "invalid-token".into()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, common::create_test_router(state))
            .await
            .unwrap()
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/tasks/runs/{}", run.id))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"type":"auth","token":token}).to_string().into(),
        ))
        .await
        .unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    if authorized && token_valid {
        let response: Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
        assert_eq!(response["type"], "init");
        assert_eq!(response["task_id"], task.id.to_string());
        let log = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let log: Value = serde_json::from_str(log.to_text().unwrap()).unwrap();
        assert_eq!(log["type"], "log");
        assert_eq!(
            log["metadata"]["action_receipt"]["actor_id"],
            user.to_string()
        );

        if status == "running" {
            sqlx::query("UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2")
                .bind(workspace).bind(user).execute(&pool).await.unwrap();
            let denied = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while let Some(Ok(Message::Text(message))) = socket.next().await {
                    let event: Value = serde_json::from_str(&message).unwrap();
                    if event["type"] == "error" {
                        return event;
                    }
                }
                panic!("revoked socket closed without a forbidden response");
            })
            .await
            .unwrap();
            assert_eq!(denied["message"], "Authorization expired or revoked");
        }
    } else {
        // A refused socket is closed without task data. Whether the refusal is
        // named in an error frame first or the socket simply closes, nothing
        // about the run may cross it.
        match &response {
            Message::Text(text) => {
                let event: Value = serde_json::from_str(text).unwrap();
                assert_eq!(
                    event["type"], "error",
                    "foreign or invalid actor received task data: {event}"
                );
                assert!(!text.contains("DO NOT LEAK"));
                if token_valid {
                    assert_eq!(event["message"], "Authorization expired or revoked");
                }
                let closed = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
                    .await
                    .unwrap();
                assert!(matches!(closed, None | Some(Ok(Message::Close(_)))));
            }
            Message::Close(_) => {}
            other => panic!("a refused socket sent {other:?} instead of closing"),
        }
    }
    socket.close(None).await.ok();
    server.abort();
    let _ = server.await;
    cleanup(&pool, organization, user).await;
    cleanup(&pool, foreign_organization, foreign_user).await;
}

#[tokio::test]
async fn foreign_workspace_socket_cannot_read_running_or_terminal_runs() {
    for status in ["running", "completed", "failed", "cancelled"] {
        websocket_case(status, false, true).await;
    }
}

#[tokio::test]
async fn member_socket_gets_initial_state_and_invalid_token_gets_no_data() {
    websocket_case("completed", true, true).await;
    websocket_case("running", true, true).await;
    websocket_case("running", true, false).await;
}

#[tokio::test]
async fn later_smaller_log_uuid_is_streamed_once_before_completion() {
    let (pool, state, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Stream receipts",
        "Task",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    let first = Uuid::from_u128(Uuid::new_v4().as_u128() | (255_u128 << 120));
    let later = Uuid::from_u128(Uuid::new_v4().as_u128() & ((1_u128 << 120) - 1));
    assert!(later < first);
    sqlx::query("INSERT INTO task_run_logs (id, task_run_id, phase, agent_type, log_level, message) VALUES ($1, $2, 'acting', 'tool', 'info', 'first')")
        .bind(first).bind(run.id).execute(&pool).await.unwrap();
    let config = common::test_config();
    let token = session_token(&pool, user, &config.jwt_secret).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, common::create_test_router(state))
            .await
            .unwrap()
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/tasks/runs/{}", run.id))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"type":"auth","token":token}).to_string().into(),
        ))
        .await
        .unwrap();
    for expected in ["init", "log"] {
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let response: Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
        assert_eq!(response["type"], expected);
        if expected == "log" {
            assert_eq!(response["id"], first.to_string());
        }
    }
    sqlx::query("INSERT INTO task_run_logs (id, task_run_id, phase, agent_type, log_level, message, metadata) VALUES ($1, $2, 'acting', 'tool', 'info', 'later receipt', $3)")
        .bind(later).bind(run.id).bind(json!({"action_receipt":{"success":true}})).execute(&pool).await.unwrap();
    sqlx::query("UPDATE task_runs SET status = 'completed' WHERE id = $1")
        .bind(run.id)
        .execute(&pool)
        .await
        .unwrap();
    let mut count = 0;
    let mut completed = false;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(Message::Text(response))) = socket.next().await {
            let response: Value = serde_json::from_str(&response).unwrap();
            if response["type"] == "completed" {
                completed = true;
                break;
            }
            assert_eq!(response["type"], "log");
            assert_eq!(
                response["id"],
                later.to_string(),
                "initial log was duplicated or later UUID suppressed"
            );
            assert_eq!(response["metadata"]["action_receipt"]["success"], true);
            count += 1;
        }
    })
    .await
    .unwrap();
    assert!(completed, "socket closed before the terminal event");
    assert_eq!(
        count, 1,
        "terminal state was sent before the final receipt log"
    );
    socket.close(None).await.ok();
    server.abort();
    let _ = server.await;
    cleanup(&pool, organization, user).await;
}

#[tokio::test]
async fn task_snapshot_rechecks_membership_after_prior_authorization() {
    let (pool, _, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Private task",
        "Private state",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    tasks::add_task_run_log(
        &pool,
        run.id,
        "acting",
        "tool",
        "info",
        "DO NOT LEAK",
        Some(json!({"action_receipt":{"success":true}})),
    )
    .await
    .unwrap();
    let member = workspace_members::get_member(&pool, workspace, user)
        .await
        .unwrap()
        .unwrap();
    assert!(member.is_active, "the earlier authorization succeeded");
    sqlx::query(
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let revoked = task_access::read(&pool, run.id, user).await.unwrap();
    cleanup(&pool, organization, user).await;
    assert!(
        revoked.is_none(),
        "revocation after authentication must prevent state and log disclosure"
    );
}

#[tokio::test]
async fn task_snapshot_orders_disclosure_after_pending_revocation() {
    let (pool, _, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Private task",
        "Private state",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    tasks::add_task_run_log(&pool, run.id, "acting", "tool", "info", "DO NOT LEAK", None)
        .await
        .unwrap();
    assert!(
        task_access::read(&pool, run.id, user)
            .await
            .unwrap()
            .is_some()
    );

    let application = format!("task-snapshot-{}", Uuid::new_v4());
    let reader = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            (*pool.connect_options())
                .clone()
                .application_name(&application),
        )
        .await
        .unwrap();
    let mut revocation = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&mut *revocation)
    .await
    .unwrap();
    let reading = {
        let reader = reader.clone();
        tokio::spawn(async move { task_access::read(&reader, run.id, user).await })
    };
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if reading.is_finished() {
                return false;
            }
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name = $1 AND wait_event_type = 'Lock')")
                .bind(&application).fetch_one(&pool).await.unwrap();
            if waiting {
                return true;
            }
            tokio::task::yield_now().await;
        }
    }).await;
    revocation.commit().await.unwrap();
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(5), reading)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    reader.close().await;
    cleanup(&pool, organization, user).await;
    assert!(
        blocked.unwrap(),
        "disclosure must wait for the pending membership mutation"
    );
    assert!(
        snapshot.is_none(),
        "the committed revocation must deny the waiting read"
    );
}

async fn http_revocation(logs: bool) {
    let (pool, _, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "DO NOT LEAK",
        "Private state",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    tasks::add_task_run_log(&pool, run.id, "acting", "tool", "info", "DO NOT LEAK", None)
        .await
        .unwrap();
    let application = format!("task-http-{}", Uuid::new_v4());
    let reader = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            (*pool.connect_options())
                .clone()
                .application_name(&application),
        )
        .await
        .unwrap();
    let config = common::test_config();
    let token = session_token(&pool, user, &config.jwt_secret).await;
    let client = common::TestClient::new(common::create_test_router(common::create_test_state(
        config,
        reader.clone(),
    )));
    let path = if logs {
        format!("/api/tasks/runs/{}/logs", run.id)
    } else {
        format!("/api/tasks/runs/{}", run.id)
    };
    client
        .get_auth(&path, &token)
        .await
        .assert_status(axum::http::StatusCode::OK);
    let mut revocation = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&mut *revocation)
    .await
    .unwrap();
    let request = tokio::spawn(async move { client.get_auth(&path, &token).await });
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if request.is_finished() {
                return false;
            }
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name = $1 AND wait_event_type = 'Lock')")
                .bind(&application).fetch_one(&pool).await.unwrap();
            if waiting {
                return true;
            }
            tokio::task::yield_now().await;
        }
    }).await;
    revocation.commit().await.unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    reader.close().await;
    cleanup(&pool, organization, user).await;
    response.assert_status(axum::http::StatusCode::NOT_FOUND);
    assert_eq!(response.json_value()["error"], "Task run not found");
    assert!(
        blocked.unwrap(),
        "HTTP disclosure must wait for pending revocation"
    );
}

#[tokio::test]
async fn http_run_waits_for_revocation_before_disclosing() {
    http_revocation(false).await;
}

#[tokio::test]
async fn http_logs_wait_for_revocation_before_disclosing() {
    http_revocation(true).await;
}

#[tokio::test]
async fn http_admission_waits_for_revocation() {
    let (pool, _, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "DO NOT LEAK",
        "Private state",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let before: Value = sqlx::query_scalar("SELECT to_jsonb(tasks) FROM tasks WHERE id = $1")
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let application = format!("task-http-{}", Uuid::new_v4());
    let reader = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            (*pool.connect_options())
                .clone()
                .application_name(&application),
        )
        .await
        .unwrap();
    let config = common::test_config();
    let token = session_token(&pool, user, &config.jwt_secret).await;
    let client = common::TestClient::new(common::create_test_router(common::create_test_state(
        config,
        reader.clone(),
    )));
    let path = format!("/api/tasks/{}/runs", task.id);
    let mut revocation = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&mut *revocation)
    .await
    .unwrap();
    let request =
        tokio::spawn(async move { client.post_json_auth(&path, &json!({}), &token).await });
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if request.is_finished() {
                return false;
            }
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name = $1 AND wait_event_type = 'Lock')")
                .bind(&application).fetch_one(&pool).await.unwrap();
            if waiting {
                return true;
            }
            tokio::task::yield_now().await;
        }
    }).await;
    revocation.commit().await.unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    let after: Value = sqlx::query_scalar("SELECT to_jsonb(tasks) FROM tasks WHERE id = $1")
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM task_runs WHERE task_id = $1")
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    reader.close().await;
    cleanup(&pool, organization, user).await;
    assert_eq!(before, after, "denied admission must leave task unchanged");
    assert_eq!(runs, 0, "denied admission must not create a run");
    assert!(after["active_run_id"].is_null());
    response.assert_status(axum::http::StatusCode::NOT_FOUND);
    assert_eq!(response.json_value()["error"], "Task not found");
    assert!(
        blocked.unwrap(),
        "HTTP admission must wait for pending revocation"
    );
}

#[tokio::test]
async fn http_admission_preserves_missing_and_conflict_statuses() {
    let (pool, state, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Existing run",
        "",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    tasks::create_task_run(&pool, task.id).await.unwrap();
    let config = common::test_config();
    let token = session_token(&pool, user, &config.jwt_secret).await;
    let client = common::TestClient::new(common::create_test_router(state));
    let missing = client
        .post_json_auth(
            &format!("/api/tasks/{}/runs", Uuid::new_v4()),
            &json!({}),
            &token,
        )
        .await;
    let conflict = client
        .post_json_auth(&format!("/api/tasks/{}/runs", task.id), &json!({}), &token)
        .await;
    cleanup(&pool, organization, user).await;
    missing.assert_status(axum::http::StatusCode::NOT_FOUND);
    conflict.assert_status(axum::http::StatusCode::CONFLICT);
    let error = conflict.json_value()["error"].as_str().unwrap().to_owned();
    assert!(
        error.starts_with("Task already has an active run") && error.contains("status: running"),
        "the conflict has to name the run holding the slot: {error}"
    );
}

/// A parked run is idle, not gone: admitting a second one beside it would race
/// two writers over the same task and violate the admission index.
#[tokio::test]
async fn admission_reports_a_parked_run_as_the_active_one() {
    let (pool, _, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Parked run",
        "Waiting on an answer",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    let owner = Uuid::new_v4();
    assert!(tasks::claim_task_run(&pool, run.id, owner).await.unwrap());
    assert!(
        tasks::park_task_run(
            &pool,
            run.id,
            owner,
            json!({"tool_call_id": "call_1", "questions": []}),
        )
        .await
        .unwrap()
    );
    let admission = tasks::create_task_run_authorized(&pool, user, task.id)
        .await
        .unwrap();
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM task_runs WHERE task_id = $1")
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    cleanup(&pool, organization, user).await;
    match admission {
        tasks::Mutation::Applied(tasks::RunMutation::Active(active)) => {
            assert_eq!(active.id, run.id);
            assert_eq!(active.status, "waiting");
            assert!(active.pending_question.is_some());
        }
        other => panic!("a parked run must block admission, got {other:?}"),
    }
    assert_eq!(
        runs, 1,
        "admission created a second run beside a parked one"
    );
}

/// Answering for a run is a write: a viewer may watch one and must not speak
/// for the workspace, and a stranger may do neither.
#[tokio::test]
async fn task_write_access_admits_writers_and_refuses_viewers() {
    let (pool, _, organization, workspace, user) = fixture().await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Answerable run",
        "Waiting on an answer",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    for role in ["owner", "admin", "member"] {
        sqlx::query(
            "UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace)
        .bind(user)
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
        let authorized = task_access::write(&pool, run.id, user).await.unwrap();
        assert_eq!(
            authorized.map(|authorized| authorized.id),
            Some(run.id),
            "a {role} must be able to answer a parked run"
        );
    }
    sqlx::query(
        "UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let viewer = task_access::write(&pool, run.id, user).await.unwrap();
    let stranger = task_access::write(&pool, run.id, Uuid::new_v4())
        .await
        .unwrap();
    cleanup(&pool, organization, user).await;
    assert!(viewer.is_none(), "a viewer answered for the workspace");
    assert!(stranger.is_none(), "a non-member answered for a run");
}
