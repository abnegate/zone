//! The tasks a chat's agent created or started: recorded against the chat,
//! listed with the run each is on, and out of reach of anyone outside the
//! chat's workspace.

use crate::common::{
    TestClient, create_test_router, create_test_state, test_config, test_email, test_password,
};

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::agent::{ChatTools, WorkspaceScope};
use zone_server::db::actions::{self, StartTask};
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::state::AppState;

struct Registered {
    token: String,
    user: Uuid,
}

/// A workspace with one chat, served by a fresh router.
struct Workspace {
    client: TestClient,
    state: AppState,
    owner: Registered,
    workspace: Uuid,
    chat: Uuid,
}

impl Workspace {
    async fn new() -> Self {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("a test database"))
            .await
            .expect("the test database");
        let state = create_test_state(test_config(), pool);
        let client = TestClient::new(create_test_router(state.clone()));
        let (owner, workspace) = member(&client).await;
        let chat = Self::chat(&client, &owner.token, workspace).await;
        Self {
            client,
            state,
            owner,
            workspace,
            chat,
        }
    }

    async fn chat(client: &TestClient, token: &str, workspace: Uuid) -> Uuid {
        let created = client
            .post_json_auth(
                "/api/chats",
                &json!({
                    "workspace_id": workspace,
                    "title": "Tasks",
                    "model_name": "sonnet",
                    "automatic_title": false,
                }),
                token,
            )
            .await
            .json_value();
        created["chat"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("chat creation failed: {created}"))
            .parse()
            .expect("a chat id")
    }

    async fn agent(&self, chat: Uuid) -> ChatTools {
        ChatTools::build(WorkspaceScope {
            state: self.state.clone(),
            workspace_id: self.workspace,
            chat_id: Some(chat),
            user_id: self.owner.user,
            offline: false,
        })
        .await
    }

    async fn start(&self, title: &str) -> Value {
        let input: StartTask =
            serde_json::from_value(json!({"title": title, "description": "Do the work"}))
                .expect("a start request");
        actions::start_task(
            self.state.db(),
            self.workspace,
            self.owner.user,
            Some(self.chat),
            input,
        )
        .await
        .expect("a started task")
    }

    fn tasks(&self) -> String {
        format!("/api/chats/{}/tasks", self.chat)
    }

    async fn listed(&self, token: &str) -> Vec<Value> {
        let response = self.client.get_auth(&self.tasks(), token).await;
        response.assert_status(StatusCode::OK);
        response.json_value()["tasks"]
            .as_array()
            .expect("a task list")
            .clone()
    }

    async fn join(&self, role: WorkspaceRole) -> Registered {
        let joined = register(&self.client).await;
        workspace_members::add_member(self.state.db(), self.workspace, joined.user, role, None)
            .await
            .expect("a member");
        joined
    }
}

async fn register(client: &TestClient) -> Registered {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    Registered {
        token: registered["access_token"]
            .as_str()
            .expect("registration returns an access token")
            .to_string(),
        user: registered["user"]["id"]
            .as_str()
            .expect("a user id")
            .parse()
            .expect("a uuid"),
    }
}

/// A user who owns a new organization's workspace, and that workspace.
async fn member(client: &TestClient) -> (Registered, Uuid) {
    let suffix = Uuid::new_v4();
    let registered = register(client).await;
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Tasks", "slug": format!("tasks-{suffix}")}),
            &registered.token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("an organization")
        .to_string();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Tasks", "slug": format!("tasks-{suffix}")}),
            &registered.token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .parse()
        .expect("a workspace id");
    (registered, workspace)
}

#[tokio::test]
async fn a_task_the_chats_agent_creates_is_listed_in_that_chat() {
    let workspace = Workspace::new().await;
    let created = workspace
        .agent(workspace.chat)
        .await
        .execute(
            "create_task",
            r#"{"title":"Write the notes","description":"From the chat"}"#,
        )
        .await;
    assert!(created.success, "{:?}", created.error);
    let created: Value =
        serde_json::from_str(created.output.as_deref().expect("output")).expect("a task");

    let listed = workspace.listed(&workspace.owner.token).await;

    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["id"], created["id"]);
    assert_eq!(listed[0]["title"], "Write the notes");
    assert_eq!(listed[0]["status"], "created");
    assert_eq!(listed[0]["run"], Value::Null);
    assert_eq!(listed[0]["pull_request"], Value::Null);
    assert_eq!(
        listed[0]["href"],
        format!("/tasks?id={}", created["id"].as_str().expect("an id"))
    );
}

#[tokio::test]
async fn a_task_another_chats_agent_creates_is_not_listed() {
    let workspace = Workspace::new().await;
    let other = Workspace::chat(
        &workspace.client,
        &workspace.owner.token,
        workspace.workspace,
    )
    .await;
    let created = workspace
        .agent(other)
        .await
        .execute(
            "create_task",
            r#"{"title":"Elsewhere","description":"Another chat"}"#,
        )
        .await;
    assert!(created.success, "{:?}", created.error);

    assert!(workspace.listed(&workspace.owner.token).await.is_empty());
}

#[tokio::test]
async fn a_started_task_lists_its_run_and_the_pull_request_it_published() {
    let workspace = Workspace::new().await;
    let older = workspace.start("Fix the build").await;
    let newer = workspace.start("Ship the docs").await;
    sqlx::query(
        "UPDATE tasks SET created_at = created_at - INTERVAL '1 minute' WHERE id = $1::uuid",
    )
    .bind(older["task_id"].as_str().expect("a task"))
    .execute(workspace.state.db())
    .await
    .expect("an older task");
    sqlx::query("UPDATE task_runs SET current_phase = 'implement', progress_percent = 60 WHERE id = $1::uuid")
        .bind(newer["run_id"].as_str().expect("a run"))
        .execute(workspace.state.db())
        .await
        .expect("progress");
    sqlx::query("UPDATE tasks SET pr_url = 'https://github.com/acme/widgets/pull/9', pr_status = 'open' WHERE id = $1::uuid")
        .bind(newer["task_id"].as_str().expect("a task"))
        .execute(workspace.state.db())
        .await
        .expect("a published pull request");

    let listed = workspace.listed(&workspace.owner.token).await;

    let titles: Vec<&str> = listed
        .iter()
        .map(|task| task["title"].as_str().expect("a title"))
        .collect();
    assert_eq!(titles, ["Ship the docs", "Fix the build"]);
    let running = &listed[0];
    assert_eq!(running["is_agentic"], true);
    assert_eq!(running["run"]["id"], newer["run_id"]);
    assert_eq!(running["run"]["status"], "running");
    assert_eq!(running["run"]["phase"], "implement");
    assert_eq!(running["run"]["progress"], 60);
    assert!(running["run"]["started_at"].is_string(), "{running}");
    assert_eq!(
        running["pull_request"],
        json!({"url": "https://github.com/acme/widgets/pull/9", "status": "open"})
    );
    assert!(running["created_at"].is_string(), "{running}");
}

#[tokio::test]
async fn another_workspaces_member_cannot_see_a_chats_tasks() {
    let workspace = Workspace::new().await;
    workspace.start("Private").await;
    let (stranger, _) = member(&workspace.client).await;

    let listed = workspace
        .client
        .get_auth(&workspace.tasks(), &stranger.token)
        .await;

    assert_eq!(listed.status, StatusCode::FORBIDDEN, "{}", listed.text());
    assert!(!listed.text().contains("Private"), "{}", listed.text());
}

#[tokio::test]
async fn a_signed_out_reader_cannot_see_a_chats_tasks() {
    let workspace = Workspace::new().await;
    workspace.start("Private").await;

    let listed = workspace.client.get(&workspace.tasks()).await;

    assert_eq!(listed.status, StatusCode::UNAUTHORIZED, "{}", listed.text());
}

#[tokio::test]
async fn a_viewer_of_the_chats_workspace_can_read_its_tasks() {
    let workspace = Workspace::new().await;
    workspace.start("Shared").await;
    let viewer = workspace.join(WorkspaceRole::Viewer).await;

    let listed = workspace.listed(&viewer.token).await;

    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["title"], "Shared");
}

#[tokio::test]
async fn an_unknown_chat_has_no_tasks_to_read() {
    let workspace = Workspace::new().await;

    let listed = workspace
        .client
        .get_auth(
            &format!("/api/chats/{}/tasks", Uuid::new_v4()),
            &workspace.owner.token,
        )
        .await;

    assert_eq!(listed.status, StatusCode::NOT_FOUND, "{}", listed.text());
}
