//! The subagents a chat's coding agent started: listed in the order they
//! started, out of reach of anyone outside the chat's workspace, and stopped
//! at boot once the turn that started them is gone.

use crate::common::{
    Database, TestClient, create_test_router, create_test_state, test_config, test_email,
    test_password,
};

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_core::llm::{StepStatus, SubagentStatus, SubagentStep};
use zone_server::db::chat_subagents::{self, SubagentChange};
use zone_server::db::chats::create_chat;
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
    let steps = [SubagentStep {
        id: "toolu_x".to_string(),
        name: "Bash".to_string(),
        subject: Some("cargo test".to_string()),
        status: StepStatus::Succeeded,
    }];
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
            steps: Some(&steps),
            ..SubagentChange::default()
        },
    )
    .await
    .expect("a finished subagent");
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
            "started_at",
            "status",
            "steps",
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
    assert_eq!(
        finished["steps"],
        json!([{"id": "toolu_x", "name": "Bash", "subject": "cargo test", "status": "succeeded"}])
    );
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
    assert_eq!(running["steps"], json!([]));
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
async fn boot_stops_the_subagents_of_every_turn_that_is_not_still_running() {
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
    started(pool, chat, "completed", Some(completed)).await;
    started(pool, chat, "interrupted", Some(interrupted)).await;
    started(pool, chat, "missing", Some(Uuid::new_v4())).await;
    started(pool, chat, "unnamed", None).await;
    started(pool, chat, "running", Some(running)).await;
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

    assert_eq!(stopped, 4);
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
        chat_subagents::abandon(pool).await.expect("a second sweep"),
        0
    );
    database.cleanup().await;
}
