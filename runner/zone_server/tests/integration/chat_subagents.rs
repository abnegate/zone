//! The subagents a chat's coding agent started: listed in the order they
//! started, their steps read one subagent at a time, out of reach of anyone
//! outside the chat's workspace, removed with the message that started them,
//! stopped with the turn that started them when it is interrupted, and stopped
//! by the boot and periodic sweeps once the turn that started them no longer
//! runs.

use crate::common::{
    Database, TestClient, create_test_router, create_test_state, test_config, test_email,
    test_password,
};

use std::time::Duration;

use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;
use zone_core::llm::{StepStatus, SubagentStatus, SubagentStep};
use zone_server::db::chat_subagents::{self, SubagentChange};
use zone_server::db::chats::{self, create_chat};
use zone_server::db::context::Store;
use zone_server::state::AppState;

const TURN_RUNNING: &str = "running";
const TURN_COMPLETED: &str = "completed";
const TURN_INTERRUPTED: &str = "interrupted";

/// A workspace with one chat, served by a fresh router.
struct Workspace {
    client: TestClient,
    state: AppState,
    token: String,
    chat: Uuid,
}

impl Workspace {
    async fn new() -> Self {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("a test database"))
            .await
            .expect("the test database");
        let state = create_test_state(test_config(), pool);
        let client = TestClient::new(create_test_router(state.clone()));
        let (token, workspace) = member(&client).await;
        let created = client
            .post_json_auth(
                "/api/chats",
                &json!({
                    "workspace_id": workspace,
                    "title": "Subagents",
                    "model_name": "sonnet",
                    "automatic_title": false,
                }),
                &token,
            )
            .await
            .json_value();
        let chat = created["chat"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("chat creation failed: {created}"))
            .parse()
            .expect("a chat id");
        Self {
            client,
            state,
            token,
            chat,
        }
    }

    fn subagents(&self) -> String {
        format!("/api/chats/{}/subagents", self.chat)
    }

    fn steps(&self, subagent: Uuid) -> String {
        format!("/api/chats/{}/subagents/{subagent}/steps", self.chat)
    }

    async fn subagent(&self, external_id: &str, message_id: Option<Uuid>) -> Uuid {
        chat_subagents::record(
            self.state.db(),
            &SubagentChange {
                chat_id: self.chat,
                message_id,
                external_id,
                ..SubagentChange::default()
            },
        )
        .await
        .expect("a subagent")
        .id
    }
}

fn step(id: &str, status: StepStatus) -> SubagentStep {
    SubagentStep {
        id: id.to_string(),
        name: "Bash".to_string(),
        subject: Some("cargo test".to_string()),
        status,
    }
}

async fn register(client: &TestClient) -> String {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    registered["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string()
}

/// A user who owns a new organization's workspace, and that workspace.
async fn member(client: &TestClient) -> (String, Uuid) {
    let suffix = Uuid::new_v4();
    let token = register(client).await;
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Subagents", "slug": format!("subagents-{suffix}")}),
            &token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("an organization")
        .to_string();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Subagents", "slug": format!("subagents-{suffix}")}),
            &token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .parse()
        .expect("a workspace id");
    (token, workspace)
}

#[tokio::test]
async fn the_chats_reader_gets_its_subagents_in_the_order_they_started() {
    let workspace = Workspace::new().await;
    let turn = Uuid::new_v4();
    let later = chat_subagents::record(
        workspace.state.db(),
        &SubagentChange {
            chat_id: workspace.chat,
            external_id: "thread-2",
            ..SubagentChange::default()
        },
    )
    .await
    .expect("a later subagent");
    let first = chat_subagents::record(
        workspace.state.db(),
        &SubagentChange {
            chat_id: workspace.chat,
            message_id: Some(turn),
            external_id: "toolu_01Agent",
            description: Some("Ask for a review"),
            kind: Some("general-purpose"),
            model: Some("sonnet"),
            background: Some(true),
            status: Some(SubagentStatus::Completed),
            tool_uses: Some(12),
            duration_ms: Some(53_000),
            tokens: Some(18_000),
            last_tool: Some("Bash"),
            summary: Some("Looks good"),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("a finished subagent");
    chat_subagents::record_steps(
        workspace.state.db(),
        first.id,
        &[step("toolu_x", StepStatus::Succeeded)],
    )
    .await
    .expect("its step");
    sqlx::query(
        "UPDATE chat_subagents SET started_at = started_at - INTERVAL '1 minute' WHERE id = $1",
    )
    .bind(first.id)
    .execute(workspace.state.db())
    .await
    .expect("an earlier start");

    let response = workspace
        .client
        .get_auth(&workspace.subagents(), &workspace.token)
        .await;

    response.assert_status(StatusCode::OK);
    let listed = response.json_value()["subagents"]
        .as_array()
        .expect("a subagent list")
        .clone();
    let ids: Vec<&Value> = listed.iter().map(|subagent| &subagent["id"]).collect();
    assert_eq!(ids, [&json!(first.id), &json!(later.id)]);
    let finished = listed[0].as_object().expect("a subagent");
    let mut keys: Vec<&str> = finished.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "background",
            "chat_id",
            "description",
            "duration_ms",
            "error",
            "external_id",
            "finished_at",
            "id",
            "kind",
            "last_tool",
            "message_id",
            "model",
            "revision",
            "started_at",
            "status",
            "step_count",
            "summary",
            "tokens",
            "tool_uses",
            "updated_at",
        ]
    );
    assert_eq!(finished["chat_id"], json!(workspace.chat));
    assert_eq!(finished["message_id"], json!(turn));
    assert_eq!(finished["external_id"], "toolu_01Agent");
    assert_eq!(finished["description"], "Ask for a review");
    assert_eq!(finished["kind"], "general-purpose");
    assert_eq!(finished["model"], "sonnet");
    assert_eq!(finished["background"], true);
    assert_eq!(finished["status"], "completed");
    assert_eq!(finished["tool_uses"], 12);
    assert_eq!(finished["duration_ms"], 53_000);
    assert_eq!(finished["tokens"], 18_000);
    assert_eq!(finished["last_tool"], "Bash");
    assert_eq!(finished["summary"], "Looks good");
    assert_eq!(finished["error"], Value::Null);
    assert_eq!(finished["step_count"], 1);
    assert_eq!(finished["revision"], 0);
    for time in ["started_at", "finished_at", "updated_at"] {
        let sent = finished[time].as_str().expect("a time");
        assert!(
            chrono::DateTime::parse_from_rfc3339(sent).is_ok(),
            "{time}: {sent}"
        );
    }
    let running = &listed[1];
    assert_eq!(running["status"], "running");
    assert_eq!(running["message_id"], Value::Null);
    assert_eq!(running["description"], "");
    assert_eq!(running["background"], false);
    assert_eq!(running["finished_at"], Value::Null);
    assert_eq!(running["step_count"], 0);
}

#[tokio::test]
async fn the_chats_reader_gets_a_subagents_steps_in_the_order_it_took_them() {
    let workspace = Workspace::new().await;
    let subagent = workspace.subagent("toolu_01Agent", None).await;
    let idle = workspace.subagent("toolu_02Agent", None).await;
    for steps in [
        vec![
            step("toolu_a", StepStatus::Running),
            step("toolu_b", StepStatus::Running),
        ],
        vec![
            step("toolu_c", StepStatus::Running),
            step("toolu_a", StepStatus::Succeeded),
        ],
    ] {
        chat_subagents::record_steps(workspace.state.db(), subagent, &steps)
            .await
            .expect("its steps");
    }

    let response = workspace
        .client
        .get_auth(&workspace.steps(subagent), &workspace.token)
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({"steps": [
            {"id": "toolu_a", "name": "Bash", "subject": "cargo test", "status": "succeeded"},
            {"id": "toolu_b", "name": "Bash", "subject": "cargo test", "status": "running"},
            {"id": "toolu_c", "name": "Bash", "subject": "cargo test", "status": "running"},
        ]})
    );
    let empty = workspace
        .client
        .get_auth(&workspace.steps(idle), &workspace.token)
        .await;
    empty.assert_status(StatusCode::OK);
    assert_eq!(empty.json_value(), json!({"steps": []}));
}

#[tokio::test]
async fn a_subagent_of_another_chat_or_none_at_all_has_no_steps_to_read() {
    let workspace = Workspace::new().await;
    let elsewhere = Workspace::new().await;
    let foreign = elsewhere.subagent("toolu_01Agent", None).await;

    for subagent in [foreign, Uuid::new_v4()] {
        let read = workspace
            .client
            .get_auth(&workspace.steps(subagent), &workspace.token)
            .await;
        assert_eq!(read.status, StatusCode::NOT_FOUND, "{}", read.text());
    }
}

#[tokio::test]
async fn another_workspaces_member_cannot_see_a_subagents_steps() {
    let workspace = Workspace::new().await;
    let subagent = workspace.subagent("toolu_01Agent", None).await;
    chat_subagents::record_steps(
        workspace.state.db(),
        subagent,
        &[SubagentStep {
            subject: Some("Private".to_string()),
            ..step("toolu_a", StepStatus::Running)
        }],
    )
    .await
    .expect("a step");
    let (stranger, _) = member(&workspace.client).await;

    let read = workspace
        .client
        .get_auth(&workspace.steps(subagent), &stranger)
        .await;
    let signed_out = workspace.client.get(&workspace.steps(subagent)).await;

    assert_eq!(read.status, StatusCode::FORBIDDEN, "{}", read.text());
    assert!(!read.text().contains("Private"), "{}", read.text());
    assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED);
}

async fn serve(state: AppState) -> String {
    let router = create_test_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let address = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    address.to_string()
}

/// The next frame of `kind` the socket is sent, skipping any other.
async fn frame<S>(socket: &mut S, kind: &str) -> Value
where
    S: StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(WsMessage::Text(text))) => {
                    let frame: Value = serde_json::from_str(&text).expect("a json frame");
                    if frame["type"] == kind {
                        return frame;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("the socket ended before a {kind} frame: {other:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {kind} frame in time"))
}

#[tokio::test]
async fn deleting_a_message_removes_its_subagents_and_tells_the_chats_readers() {
    let workspace = Workspace::new().await;
    let created = workspace
        .client
        .post_json_auth(
            &format!("/api/chats/{}/messages", workspace.chat),
            &json!({"role": "assistant", "content": "Asked for a review"}),
            &workspace.token,
        )
        .await
        .json_value();
    let message: Uuid = created["message"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("message creation failed: {created}"))
        .parse()
        .expect("a message id");
    let started = workspace.subagent("toolu_01Agent", Some(message)).await;
    let kept = workspace
        .subagent("toolu_02Agent", Some(Uuid::new_v4()))
        .await;
    chat_subagents::record_steps(
        workspace.state.db(),
        started,
        &[step("toolu_a", StepStatus::Running)],
    )
    .await
    .expect("its step");
    let address = serve(workspace.state.clone()).await;
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/chats/{}", workspace.chat))
        .await
        .expect("a socket");
    socket
        .send(WsMessage::Text(
            json!({"type": "auth", "token": workspace.token})
                .to_string()
                .into(),
        ))
        .await
        .expect("the authentication");
    frame(&mut socket, "init").await;

    let deleted = workspace
        .client
        .delete_auth(
            &format!("/api/chats/{}/messages/{message}", workspace.chat),
            &workspace.token,
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    assert_eq!(
        frame(&mut socket, "subagents_removed").await,
        json!({"type": "subagents_removed", "chat_id": workspace.chat, "ids": [started]})
    );
    let left: Vec<Uuid> = chat_subagents::list(workspace.state.db(), workspace.chat)
        .await
        .expect("the chat's subagents")
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(left, [kept]);
    assert_eq!(
        chat_subagents::steps(workspace.state.db(), workspace.chat, started)
            .await
            .expect("a read"),
        None
    );
    let orphans: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_subagent_steps WHERE subagent_id = $1")
            .bind(started)
            .fetch_one(workspace.state.db())
            .await
            .expect("the count");
    assert_eq!(orphans, 0);
}

#[tokio::test]
async fn deleting_a_message_directly_removes_its_subagents_too() {
    let workspace = Workspace::new().await;
    let message: Uuid = sqlx::query_scalar(
        "INSERT INTO messages (chat_id, role, content) VALUES ($1, 'assistant', 'Done')
         RETURNING id",
    )
    .bind(workspace.chat)
    .fetch_one(workspace.state.db())
    .await
    .expect("a message");
    let started = workspace.subagent("toolu_01Agent", Some(message)).await;

    let removed = chats::delete_message(workspace.state.db(), workspace.chat, message)
        .await
        .expect("the deletion");
    let missing = chats::delete_message(workspace.state.db(), workspace.chat, message)
        .await
        .expect("a second deletion");

    assert_eq!(removed, Some(vec![started]));
    assert_eq!(missing, None);
    assert_eq!(
        chat_subagents::list(workspace.state.db(), workspace.chat)
            .await
            .expect("the chat's subagents"),
        []
    );
}

#[tokio::test]
async fn another_workspaces_member_cannot_see_a_chats_subagents() {
    let workspace = Workspace::new().await;
    chat_subagents::record(
        workspace.state.db(),
        &SubagentChange {
            chat_id: workspace.chat,
            external_id: "toolu_01Agent",
            description: Some("Private"),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("a subagent");
    let (stranger, _) = member(&workspace.client).await;

    let listed = workspace
        .client
        .get_auth(&workspace.subagents(), &stranger)
        .await;

    assert_eq!(listed.status, StatusCode::FORBIDDEN, "{}", listed.text());
    assert!(!listed.text().contains("Private"), "{}", listed.text());
}

#[tokio::test]
async fn a_signed_out_reader_cannot_see_a_chats_subagents() {
    let workspace = Workspace::new().await;

    let listed = workspace.client.get(&workspace.subagents()).await;

    assert_eq!(listed.status, StatusCode::UNAUTHORIZED, "{}", listed.text());
}

#[tokio::test]
async fn an_unknown_chat_has_no_subagents_to_read() {
    let workspace = Workspace::new().await;

    let listed = workspace
        .client
        .get_auth(
            &format!("/api/chats/{}/subagents", Uuid::new_v4()),
            &workspace.token,
        )
        .await;

    assert_eq!(listed.status, StatusCode::NOT_FOUND, "{}", listed.text());
}

/// A turn of `chat` in `status`, as the chat's store leaves it.
async fn turn(pool: &PgPool, chat: Uuid, status: &str) -> Uuid {
    let message = Uuid::new_v4();
    let turn = Uuid::new_v4();
    sqlx::query("INSERT INTO messages (id, chat_id, role, content) VALUES ($1, $2, 'user', 'Go')")
        .bind(message)
        .bind(chat)
        .execute(pool)
        .await
        .expect("a user message");
    sqlx::query(
        "INSERT INTO chat_turns (id, chat_id, user_message_id, fence, status) VALUES ($1, $2, $3, 1, $4)",
    )
    .bind(turn)
    .bind(chat)
    .bind(message)
    .bind(status)
    .execute(pool)
    .await
    .expect("a turn");
    turn
}

/// Gives `chat`'s lease to generation `fence` until `seconds` from now, or
/// since that long ago when negative.
async fn lease(pool: &PgPool, chat: Uuid, fence: i64, seconds: i64) {
    sqlx::query(
        "INSERT INTO chat_leases (chat_id, owner, fence, expires_at)
         VALUES ($1, gen_random_uuid(), $2, clock_timestamp() + $3 * interval '1 second')",
    )
    .bind(chat)
    .bind(fence)
    .bind(seconds)
    .execute(pool)
    .await
    .expect("a lease");
}

async fn started(pool: &PgPool, chat: Uuid, name: &str, turn: Option<Uuid>) {
    chat_subagents::record(
        pool,
        &SubagentChange {
            chat_id: chat,
            message_id: turn,
            external_id: name,
            ..SubagentChange::default()
        },
    )
    .await
    .expect("a running subagent");
}

#[tokio::test]
async fn the_sweep_stops_the_subagents_of_every_turn_that_no_longer_runs() {
    let database = Database::new("chat_subagents").await;
    sqlx::migrate!("./migrations")
        .run(&database.pool)
        .await
        .expect("the schema");
    let pool = &database.pool;
    let chat = create_chat(pool, None, "Subagents", "sonnet", false, true)
        .await
        .expect("a chat")
        .id;
    let completed = turn(pool, chat, TURN_COMPLETED).await;
    let interrupted = turn(pool, chat, TURN_INTERRUPTED).await;
    let running = turn(pool, chat, TURN_RUNNING).await;
    lease(pool, chat, 1, 60).await;
    let lapsed_chat = create_chat(pool, None, "Lapsed", "sonnet", false, true)
        .await
        .expect("a chat whose server died")
        .id;
    let lapsed = turn(pool, lapsed_chat, TURN_RUNNING).await;
    lease(pool, lapsed_chat, 1, -1).await;
    started(pool, lapsed_chat, "lapsed", Some(lapsed)).await;
    let taken_chat = create_chat(pool, None, "Taken", "sonnet", false, true)
        .await
        .expect("a chat another generation took")
        .id;
    let superseded = turn(pool, taken_chat, TURN_RUNNING).await;
    lease(pool, taken_chat, 2, 60).await;
    started(pool, taken_chat, "superseded", Some(superseded)).await;
    started(pool, chat, "completed", Some(completed)).await;
    started(pool, chat, "interrupted", Some(interrupted)).await;
    started(pool, chat, "missing", Some(Uuid::new_v4())).await;
    started(pool, chat, "unnamed", None).await;
    started(pool, chat, "running", Some(running)).await;
    let interrupted_subagent = chat_subagents::list(pool, chat)
        .await
        .expect("the list")
        .into_iter()
        .find(|row| row.external_id == "interrupted")
        .expect("the interrupted turn's subagent")
        .id;
    chat_subagents::record_steps(
        pool,
        interrupted_subagent,
        &[
            step("toolu_done", StepStatus::Succeeded),
            step("toolu_open", StepStatus::Running),
        ],
    )
    .await
    .expect("its steps");
    chat_subagents::record(
        pool,
        &SubagentChange {
            chat_id: chat,
            message_id: Some(completed),
            external_id: "failed",
            status: Some(SubagentStatus::Failed),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("a failed subagent");

    let stopped = chat_subagents::abandon(pool).await.expect("the sweep");

    let mut names: Vec<&str> = stopped
        .iter()
        .map(|stopped| stopped.subagent.external_id.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "completed",
            "interrupted",
            "lapsed",
            "missing",
            "superseded",
            "unnamed"
        ]
    );
    let failed: Vec<&[SubagentStep]> = stopped
        .iter()
        .filter(|stopped| stopped.subagent.id == interrupted_subagent)
        .map(|stopped| stopped.failed.0.as_slice())
        .collect();
    assert_eq!(
        failed,
        [[step("toolu_open", StepStatus::Failed)].as_slice()]
    );
    let mut statuses: Vec<(String, String, bool)> = chat_subagents::list(pool, chat)
        .await
        .expect("the list")
        .into_iter()
        .map(|row| (row.external_id, row.status, row.finished_at.is_some()))
        .collect();
    statuses.sort();
    let expected = [
        ("completed", "stopped", true),
        ("failed", "failed", true),
        ("interrupted", "stopped", true),
        ("missing", "stopped", true),
        ("running", "running", false),
        ("unnamed", "stopped", true),
    ]
    .map(|(name, status, finished)| (name.to_string(), status.to_string(), finished));
    assert_eq!(statuses, expected);
    assert_eq!(
        chat_subagents::steps(pool, chat, interrupted_subagent)
            .await
            .expect("a read")
            .expect("the subagent"),
        [
            step("toolu_done", StepStatus::Succeeded),
            step("toolu_open", StepStatus::Failed),
        ]
    );
    assert_eq!(
        chat_subagents::abandon(pool).await.expect("a second sweep"),
        []
    );
    database.cleanup().await;
}

/// A transaction of its own and the session it runs in.
async fn session(pool: &PgPool) -> (sqlx::Transaction<'static, sqlx::Postgres>, i32) {
    let mut transaction = pool.begin().await.expect("a transaction");
    let pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *transaction)
        .await
        .expect("its session");
    (transaction, pid)
}

/// Waits until some other session is waiting on a lock session `pid` holds.
async fn blocking(pool: &PgPool, pid: i32) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
            .await
            .expect("the lock waits");
            if blocked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a session waiting on the lock in time");
}

#[tokio::test]
async fn the_periodic_sweep_begun_before_a_newer_turn_took_a_subagent_leaves_that_run_alone() {
    let database = Database::new("chat_subagents").await;
    sqlx::migrate!("./migrations")
        .run(&database.pool)
        .await
        .expect("the schema");
    let pool = &database.pool;
    let chat = create_chat(pool, None, "Subagents", "sonnet", false, true)
        .await
        .expect("a chat")
        .id;
    let earlier = turn(pool, chat, TURN_COMPLETED).await;
    started(pool, chat, "thread-1", Some(earlier)).await;
    let subagent = chat_subagents::list(pool, chat).await.expect("the list")[0].id;
    let (mut successor, pid) = session(pool).await;
    sqlx::query("SELECT 1 FROM chat_subagents WHERE id = $1 FOR UPDATE")
        .bind(subagent)
        .execute(&mut *successor)
        .await
        .expect("the subagent held");
    let sweep = tokio::spawn({
        let pool = pool.clone();
        async move { chat_subagents::abandon(&pool).await }
    });
    blocking(pool, pid).await;

    chat_subagents::record(
        &mut *successor,
        &SubagentChange {
            chat_id: chat,
            message_id: Some(earlier),
            external_id: "thread-1",
            status: Some(SubagentStatus::Completed),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("the earlier run's outcome");
    let newer = Uuid::new_v4();
    sqlx::query(
        "WITH asked AS (
             INSERT INTO messages (chat_id, role, content) VALUES ($2, 'user', 'Again')
             RETURNING id
         )
         INSERT INTO chat_turns (id, chat_id, user_message_id, fence, status)
         SELECT $1, $2, id, 1, $3 FROM asked",
    )
    .bind(newer)
    .bind(chat)
    .bind(TURN_RUNNING)
    .execute(&mut *successor)
    .await
    .expect("the newer turn");
    sqlx::query(
        "INSERT INTO chat_leases (chat_id, owner, fence, expires_at)
         VALUES ($1, gen_random_uuid(), 1, clock_timestamp() + interval '1 minute')",
    )
    .bind(chat)
    .execute(&mut *successor)
    .await
    .expect("the newer turn's lease");
    chat_subagents::record(
        &mut *successor,
        &SubagentChange {
            chat_id: chat,
            message_id: Some(newer),
            external_id: "thread-1",
            status: Some(SubagentStatus::Running),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("the newer turn's run");
    chat_subagents::record_steps(
        &mut *successor,
        subagent,
        &[step("toolu_open", StepStatus::Running)],
    )
    .await
    .expect("the newer run's step");
    successor.commit().await.expect("the newer turn's commit");
    let stopped = sweep.await.expect("the sweep ran").expect("the sweep");

    assert_eq!(stopped, [], "the run the sweep saw had already ended");
    let row = &chat_subagents::list(pool, chat).await.expect("the list")[0];
    assert_eq!(
        (row.message_id, row.status.as_str()),
        (Some(newer), SubagentStatus::Running.as_str())
    );
    assert_eq!(
        chat_subagents::steps(pool, chat, subagent)
            .await
            .expect("a read")
            .expect("the subagent"),
        [step("toolu_open", StepStatus::Running)],
        "the newer run's steps run on"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn recovering_a_crashed_turn_stops_its_subagents_and_fails_their_running_steps() {
    let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("a test database"))
        .await
        .expect("the test database");
    let chat = create_chat(&pool, None, "Subagents", "sonnet", false, true)
        .await
        .expect("a chat")
        .id;
    let crashed = turn(&pool, chat, TURN_RUNNING).await;
    let completed = turn(&pool, chat, TURN_COMPLETED).await;
    let orphan = chat_subagents::record(
        &pool,
        &SubagentChange {
            chat_id: chat,
            message_id: Some(crashed),
            external_id: "crashed",
            background: Some(true),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("the crashed turn's subagent");
    chat_subagents::record_steps(
        &pool,
        orphan.id,
        &[
            step("toolu_done", StepStatus::Succeeded),
            step("toolu_open", StepStatus::Running),
        ],
    )
    .await
    .expect("its steps");
    started(&pool, chat, "completed", Some(completed)).await;
    let before = chat_subagents::list(&pool, chat)
        .await
        .expect("the list")
        .into_iter()
        .find(|row| row.id == orphan.id)
        .expect("the crashed turn's subagent");

    let store = Store::new(pool.clone(), chat, None);
    let lease = store
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .expect("the chat's lease");
    let recovered = store.recover(&lease).await.expect("the recovery");

    assert_eq!(recovered, 1);
    let rows = chat_subagents::list(&pool, chat).await.expect("the list");
    let stopped = rows
        .iter()
        .find(|row| row.id == orphan.id)
        .expect("the crashed turn's subagent");
    assert_eq!(stopped.status, SubagentStatus::Stopped.as_str());
    assert!(stopped.finished_at.is_some());
    assert!(stopped.revision > before.revision);
    assert_eq!(
        chat_subagents::steps(&pool, chat, orphan.id)
            .await
            .expect("a read")
            .expect("the subagent"),
        [
            step("toolu_done", StepStatus::Succeeded),
            step("toolu_open", StepStatus::Failed),
        ]
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.external_id == "completed")
            .expect("the completed turn's subagent")
            .status,
        SubagentStatus::Running.as_str(),
        "a completed turn's subagents are its keeper's or the next turn's to stop"
    );
    store.release(&lease).await.expect("the lease's release");
}
