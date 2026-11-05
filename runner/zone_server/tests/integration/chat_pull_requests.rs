//! The pull requests a chat opened: recorded when Zone's own tool opens one
//! and when a coding agent's shell does, never when one is only mentioned,
//! read and changed through the workspace's GitHub source, and out of reach of
//! anyone outside the chat's workspace.

use crate::common::context::{Socket, finish, send};
use crate::common::{
    TestClient, context_database_url, create_test_router, create_test_state, test_config,
    test_email, test_password,
};

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::AgentKind;
use zone_server::config::{Config, ModelBackend};
use zone_server::db::chat_pull_requests::{self, NewPullRequest};
use zone_server::db::sources;
use zone_server::services::chat::pull_request::PENDING_RELEASE;
use zone_server::state::AppState;

const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);
const RECORD_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_TOKEN: &str = "ghp_chat_pull_requests_test";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const TITLE: &str = "Add widgets";

/// A stand-in for the host's `claude`, held open until the test has done what
/// it needs to during the turn, then replaying the lines the test gave it.
struct Agent {
    executable: PathBuf,
    arguments: PathBuf,
    token: PathBuf,
    lines: PathBuf,
    release: PathBuf,
}

impl Agent {
    fn write(directory: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let agent = Self {
            executable: directory.join("agent"),
            arguments: directory.join("arguments"),
            token: directory.join("token"),
            lines: directory.join("lines"),
            release: directory.join("release"),
        };
        std::fs::write(
            &agent.executable,
            format!(
                "#!/bin/sh\ncat > /dev/null\nprintf '%s' \"$ZONE_MCP_TOKEN\" > {token}\nprintf \
                 '%s' \"$*\" > {arguments}.partial\nmv {arguments}.partial {arguments}\nwhile [ \
                 ! -f {release} ]; do sleep 0.05; done\ncat {lines}\n",
                token = agent.token.display(),
                arguments = agent.arguments.display(),
                release = agent.release.display(),
                lines = agent.lines.display(),
            ),
        )
        .expect("the stand-in agent");
        std::fs::set_permissions(&agent.executable, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in agent to be executable");
        agent
    }

    async fn spawned(&self) -> (String, String) {
        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(arguments) = std::fs::read_to_string(&self.arguments) {
                return (
                    arguments,
                    std::fs::read_to_string(&self.token).unwrap_or_default(),
                );
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the CLI backend never spawned its agent");
    }

    /// Lets the agent print `lines`, and then the result that ends its turn.
    fn finish(&self, lines: &[Value]) {
        let mut stream: Vec<String> = lines.iter().map(Value::to_string).collect();
        stream.push(json!({"type": "result", "subtype": "success", "is_error": false}).to_string());
        std::fs::write(&self.lines, stream.join("\n") + "\n").expect("the agent's lines");
        std::fs::write(&self.release, "go").expect("releasing the agent");
    }
}

/// A workspace with one chat, served by a router whose GitHub is `github`.
struct Workspace {
    client: TestClient,
    state: AppState,
    token: String,
    workspace: Uuid,
    chat: Uuid,
}

impl Workspace {
    async fn new(config: Config, pool: PgPool) -> Self {
        let state = create_test_state(config, pool);
        let client = TestClient::new(create_test_router(state.clone()));
        let (token, workspace) = member(&client).await;
        let created = client
            .post_json_auth(
                "/api/chats",
                &json!({
                    "workspace_id": workspace,
                    "title": "Changes",
                    "model_name": "sonnet",
                    "agent_enabled": true,
                    "agent_sandboxed": false,
                    "auto_approve": true,
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
            workspace,
            chat,
        }
    }

    async fn connect_github(&self) -> Uuid {
        let encrypted = zone_server::crypto::encrypt(self.state.encryption_key(), GITHUB_TOKEN)
            .expect("an encrypted token");
        sources::create_source(
            self.state.db(),
            self.workspace,
            &format!("widgets-{}", Uuid::new_v4()),
            "github",
            json!({"owner": "acme", "repo": "widgets", "branch": "main"}),
            None,
            None,
            Some(&encrypted),
        )
        .await
        .expect("a GitHub source")
        .id
    }

    async fn record(&self, url: &str) -> Uuid {
        chat_pull_requests::record(
            self.state.db(),
            &NewPullRequest {
                chat_id: self.chat,
                owner: "acme",
                repo: "widgets",
                number: 12,
                url,
                title: Some(TITLE),
                source_id: None,
            },
        )
        .await
        .expect("a recorded pull request")
        .id
    }

    fn pull_requests(&self) -> String {
        format!("/api/chats/{}/pull-requests", self.chat)
    }

    fn pull_request(&self, id: Uuid) -> String {
        format!("{}/{id}", self.pull_requests())
    }

    async fn listed(&self, token: &str) -> Value {
        let response = self.client.get_auth(&self.pull_requests(), token).await;
        response.assert_status(StatusCode::OK);
        response.json_value()["pull_requests"].clone()
    }

    async fn update(&self, id: Uuid, body: Value) -> (StatusCode, Value) {
        let response = self
            .client
            .patch_json_auth(&self.pull_request(id), &body, &self.token)
            .await;
        (response.status, response.json_value())
    }
}

/// A user who owns a new organization's workspace: their token, and the
/// workspace.
async fn member(client: &TestClient) -> (String, Uuid) {
    let suffix = Uuid::new_v4();
    let token = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value()["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Changes", "slug": format!("changes-{suffix}")}),
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
            &json!({"name": "Changes", "slug": format!("changes-{suffix}")}),
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

fn config(github: &MockServer, database: String) -> Config {
    let mut config = test_config();
    config.database_url = database;
    config.github_api_url = github.uri();
    config.github_api_allows_http = true;
    config.comfyui.enabled = false;
    config.web_search.enabled = false;
    config
}

fn link(github: &MockServer, number: u64) -> String {
    format!("{}/acme/widgets/pull/{number}", github.uri())
}

/// GitHub as it describes pull request 12: open, its checks passing, and its
/// repository allowing auto-merge.
async fn describe(github: &MockServer, draft: bool) {
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/12"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "node_id": "PR_kwDOchat",
            "number": 12,
            "title": TITLE,
            "state": "open",
            "draft": draft,
            "merged": false,
            "mergeable": true,
            "mergeable_state": if draft { "draft" } else { "blocked" },
            "head": {"ref": "feature", "sha": HEAD},
            "base": {"ref": "main", "sha": HEAD},
            "html_url": link(github, 12),
            "additions": 40,
            "deletions": 3,
            "changed_files": 2,
            "commits": 1,
        })))
        .mount(github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/acme/widgets/commits/{HEAD}/check-runs"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_count": 1,
            "check_runs": [{"name": "ci", "status": "completed", "conclusion": "success"}],
        })))
        .mount(github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/widgets/commits/{HEAD}/status")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "state": "pending",
            "total_count": 0,
            "statuses": [],
        })))
        .mount(github)
        .await;
}

/// A server listening for the coding agent it spawns to call back into.
struct Served {
    workspace: Workspace,
    address: String,
    agent: Agent,
    _llm: MockServer,
    _directory: TempDir,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Served {
    async fn start(github: &MockServer) -> Self {
        let llm = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&llm)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
            .mount(&llm)
            .await;
        let directory = TempDir::new().expect("a temporary directory");
        let agent = Agent::write(directory.path());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener.local_addr().expect("the bound address");

        let database = context_database_url();
        let pool = PgPool::connect(&database).await.expect("the test database");
        let mut config = config(github, database);
        config.host = address.ip().to_string();
        config.port = address.port();
        config.litellm_host = llm.uri();
        config.ollama_host = llm.uri();
        config.model_backend = ModelBackend::Cli {
            agent: AgentKind::Claude,
            executable: Some(agent.executable.clone()),
        };
        let workspace = Workspace::new(config, pool).await;
        let router = create_test_router(workspace.state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self {
            workspace,
            address: address.to_string(),
            agent,
            _llm: llm,
            _directory: directory,
            server,
        }
    }

    async fn turn(&self, content: &str) -> tokio::task::JoinHandle<Vec<Value>> {
        let (mut socket, _): (Socket, _) = connect_async(format!(
            "ws://{}/ws/chats/{}",
            self.address, self.workspace.chat
        ))
        .await
        .expect("the chat socket");
        send(
            &mut socket,
            json!({"type": "auth", "token": self.workspace.token}),
        )
        .await;
        let content = content.to_string();
        tokio::spawn(async move {
            send(&mut socket, json!({"type": "send", "content": content})).await;
            finish(&mut socket).await
        })
    }

    /// The chat's pull requests once `count` are recorded.
    async fn recorded(&self, count: usize) -> Value {
        let deadline = Instant::now() + RECORD_TIMEOUT;
        loop {
            let listed = self.workspace.listed(&self.workspace.token).await;
            if listed.as_array().is_some_and(|rows| rows.len() >= count)
                || Instant::now() > deadline
            {
                return listed;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn endpoint(arguments: &str) -> String {
    let rest = arguments
        .split_once(r#""url":""#)
        .expect("zone named its MCP endpoint")
        .1;
    rest.split('"').next().expect("an endpoint").to_string()
}

async fn rpc(endpoint: &str, token: &str, params: Value) -> Value {
    reqwest::Client::new()
        .post(endpoint)
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params}))
        .send()
        .await
        .expect("zone's MCP endpoint answers")
        .json()
        .await
        .expect("a JSON-RPC answer")
}

fn bash(id: &str, command: &str, printed: &str, is_error: bool) -> [Value; 2] {
    [
        json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}},
        ]}}),
        json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": printed, "is_error": is_error},
        ]}}),
    ]
}

#[tokio::test]
async fn zones_own_tool_records_the_pull_request_it_opens_with_its_status() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/widgets/pulls"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 12,
            "html_url": link(&github, 12),
            "title": TITLE,
            "state": "open",
        })))
        .expect(1)
        .mount(&github)
        .await;
    let served = Served::start(&github).await;
    let source = served.workspace.connect_github().await;

    let turn = served.turn("Open the pull request.").await;
    let (arguments, token) = served.agent.spawned().await;
    let called = rpc(
        &endpoint(&arguments),
        &token,
        json!({"name": "create_pull_request", "arguments": {
            "source_id": source,
            "title": TITLE,
            "head": "feature",
            "base": "main",
            "reason": "The reader asked for a pull request.",
        }}),
    )
    .await;
    assert_eq!(called["result"]["isError"], false, "{called}");
    served.agent.finish(&[]);
    turn.await.expect("the turn ends");

    let listed = served.recorded(1).await;
    let rows = listed.as_array().expect("a list");
    assert_eq!(rows.len(), 1, "{listed}");
    let row = &rows[0];
    assert_eq!(row["owner"], "acme");
    assert_eq!(row["repo"], "widgets");
    assert_eq!(row["number"], 12);
    assert_eq!(row["url"], link(&github, 12));
    assert_eq!(row["title"], TITLE);
    assert_eq!(row["dismissed"], false);
    assert_eq!(row["actionable"], true);
    let status = &row["status"];
    assert_eq!(status["state"], "open", "{row}");
    assert_eq!(status["draft"], false);
    assert_eq!(status["additions"], 40);
    assert_eq!(status["deletions"], 3);
    assert_eq!(status["checks"], "passing");
    assert_eq!(status["mergeable"], "blocked");
    assert_eq!(
        status["auto_merge"],
        json!({"enabled": false, "unavailable": PENDING_RELEASE})
    );
    assert!(
        !listed.to_string().contains(GITHUB_TOKEN),
        "a token reached the response: {listed}"
    );
}

#[tokio::test]
async fn a_pull_request_a_cli_agent_opened_is_recorded_but_one_it_only_mentioned_is_not() {
    let github = MockServer::start().await;
    let served = Served::start(&github).await;
    let opened = link(&github, 7);
    let viewed = link(&github, 8);
    let failed = link(&github, 9);
    let mentioned = link(&github, 10);

    let turn = served.turn("Ship it.").await;
    served.agent.spawned().await;
    let lines: Vec<Value> = [
        bash(
            "toolu_01",
            "git push -u origin feature && gh pr create --fill",
            &format!("Creating pull request for feature into main in acme/widgets\n\n{opened}\n"),
            false,
        ),
        bash("toolu_02", "gh pr view 8 --json url", &viewed, false),
        bash(
            "toolu_03",
            "gh pr create --fill",
            &format!("a pull request for branch \"other\" already exists:\n{failed}"),
            true,
        ),
    ]
    .into_iter()
    .flatten()
    .chain([
        json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
            {"type": "text", "text": format!("Opened {opened}; it builds on {mentioned}.")},
        ]}}),
    ])
    .collect();
    served.agent.finish(&lines);
    turn.await.expect("the turn ends");

    let listed = served.recorded(1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let listed_again = served.workspace.listed(&served.workspace.token).await;
    assert_eq!(listed, listed_again, "a later sighting was recorded");
    let urls: Vec<&str> = listed
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|row| row["url"].as_str())
        .collect();
    assert_eq!(urls, [opened.as_str()], "{listed}");
    assert_eq!(listed[0]["number"], 7);
    assert_eq!(listed[0]["status"], Value::Null, "no source can read it");
    assert_eq!(listed[0]["actionable"], false);
}

#[tokio::test]
async fn another_workspaces_member_can_neither_see_nor_change_a_chats_pull_requests() {
    let github = MockServer::start().await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    let id = workspace.record(&link(&github, 12)).await;
    let (stranger, _) = member(&workspace.client).await;

    let listed = workspace
        .client
        .get_auth(&workspace.pull_requests(), &stranger)
        .await;
    assert_eq!(listed.status, StatusCode::FORBIDDEN, "{}", listed.text());
    for body in [
        json!({"dismissed": true}),
        json!({"draft": true}),
        json!({"auto_merge": true}),
    ] {
        let changed = workspace
            .client
            .patch_json_auth(&workspace.pull_request(id), &body, &stranger)
            .await;
        assert_eq!(
            changed.status,
            StatusCode::FORBIDDEN,
            "{body}: {}",
            changed.text()
        );
    }
    let diff = workspace
        .client
        .get_auth(&format!("{}/diff", workspace.pull_request(id)), &stranger)
        .await;
    assert_eq!(diff.status, StatusCode::FORBIDDEN, "{}", diff.text());

    let row = chat_pull_requests::get(workspace.state.db(), workspace.chat, id)
        .await
        .expect("the row")
        .expect("still recorded");
    assert_eq!(row.dismissed_at, None, "a stranger dismissed it");
    assert!(
        github
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
}

#[tokio::test]
async fn a_dismissed_pull_request_leaves_the_chat_until_it_is_restored() {
    let github = MockServer::start().await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    let id = workspace.record(&link(&github, 12)).await;
    assert_eq!(
        workspace
            .listed(&workspace.token)
            .await
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let (status, dismissed) = workspace.update(id, json!({"dismissed": true})).await;
    assert_eq!(status, StatusCode::OK, "{dismissed}");
    assert_eq!(dismissed["dismissed"], true);
    assert_eq!(workspace.listed(&workspace.token).await, json!([]));

    let (status, restored) = workspace.update(id, json!({"dismissed": false})).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["dismissed"], false);
    let listed = workspace.listed(&workspace.token).await;
    assert_eq!(listed[0]["id"], id.to_string(), "{listed}");

    let (status, refused) = workspace.update(id, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let (status, missing) = workspace
        .update(Uuid::new_v4(), json!({"dismissed": true}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
}

#[tokio::test]
async fn draft_and_auto_merge_changes_wait_for_the_abnegate_vcs_release() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;
    assert_eq!(
        listed[0]["status"]["auto_merge"],
        json!({"enabled": false, "unavailable": PENDING_RELEASE}),
        "{listed}"
    );
    assert_eq!(listed[0]["status"]["draft_unavailable"], PENDING_RELEASE);
    for body in [json!({"draft": true}), json!({"auto_merge": true})] {
        let (status, refused) = workspace.update(id, body.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}: {refused}");
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|error| error.contains(PENDING_RELEASE)),
            "{refused}"
        );
    }
    let writes: Vec<_> = github
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request.method.as_str() != "GET")
        .collect();
    assert!(writes.is_empty(), "something wrote to GitHub: {writes:?}");
}

#[tokio::test]
async fn the_diff_is_read_with_the_sources_credential() {
    let github = MockServer::start().await;
    let diff = "diff --git a/src/widgets.rs b/src/widgets.rs\n+pub struct Widget;\n";
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/12"))
        .and(header("Accept", "application/vnd.github.diff"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(diff))
        .expect(1)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let response = workspace
        .client
        .get_auth(
            &format!("{}/diff", workspace.pull_request(id)),
            &workspace.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({"diff": diff, "truncated": false})
    );
}
