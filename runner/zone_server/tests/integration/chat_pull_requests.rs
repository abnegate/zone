//! The pull requests a chat opened: recorded when Zone's own tool opens one
//! and when a coding agent's shell does, never when one is only mentioned,
//! read and changed through the workspace's GitHub source, and out of reach of
//! anyone outside the chat's workspace.

use crate::common::context::{Socket, finish, send};
use crate::common::{
    TestClient, context_database_url, create_test_router, create_test_state, test_config,
    test_email, test_password,
};

use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::AgentKind;
use zone_server::config::{Config, ModelBackend};
use zone_server::db::chat_pull_requests::{self, NewPullRequest};
use zone_server::db::sources;
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::services::chat::pull_request::AutoMerge;
use zone_server::state::AppState;

const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);
const RECORD_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_TOKEN: &str = "ghp_chat_pull_requests_test";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const TITLE: &str = "Add widgets";
const NODE: &str = "PR_kwDOchat";

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
        let encrypted = encrypt_value(&SecretValue::new(GITHUB_TOKEN), self.state.master_key())
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

/// Pull request 12 as GitHub's REST API describes it: open, and a draft when
/// `draft` is.
fn detail(github: &MockServer, draft: bool) -> Value {
    json!({
        "node_id": NODE,
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
    })
}

/// GitHub as it describes pull request 12: open, its checks passing, and its
/// repository allowing auto-merge.
async fn describe(github: &MockServer, draft: bool) {
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/12"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail(github, draft)))
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

fn threads_read() -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .and(body_string_contains("reviewThreads"))
        .and(|request: &wiremock::Request| {
            !String::from_utf8_lossy(&request.body).contains("comments")
        })
}

/// GitHub's page of pull request 12's review threads, one resolved for each
/// `true` in `resolved`, read without their comments.
fn threads_answer(resolved: &[bool]) -> ResponseTemplate {
    let nodes: Vec<Value> = resolved
        .iter()
        .enumerate()
        .map(|(index, resolved)| {
            json!({
                "id": format!("PRRT_{index}"),
                "isResolved": resolved,
            })
        })
        .collect();
    ResponseTemplate::new(200).set_body_json(json!({"data": {"repository": {
        "pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": false, "endCursor": null},
            "nodes": nodes,
        }},
    }}}))
}

async fn discuss(github: &MockServer, resolved: &[bool]) {
    threads_read()
        .respond_with(threads_answer(resolved))
        .mount(github)
        .await;
}

/// How GitHub's GraphQL API describes pull request 12's auto-merge and
/// draft state, and its repository's merge settings.
#[derive(Clone, Copy)]
struct Automation {
    allowed: bool,
    draft: bool,
    enabled: bool,
}

impl Default for Automation {
    fn default() -> Self {
        Self {
            allowed: true,
            draft: false,
            enabled: false,
        }
    }
}

impl Automation {
    fn answer(self) -> ResponseTemplate {
        let request = match self.enabled {
            true => json!({"mergeMethod": "SQUASH", "enabledBy": {"login": "zone"}}),
            false => Value::Null,
        };
        ResponseTemplate::new(200).set_body_json(json!({"data": {"repository": {
            "autoMergeAllowed": self.allowed,
            "squashMergeAllowed": true,
            "mergeCommitAllowed": true,
            "rebaseMergeAllowed": false,
            "pullRequest": {
                "id": NODE,
                "isDraft": self.draft,
                "autoMergeRequest": request,
                "viewerCanEnableAutoMerge": true,
                "viewerCanDisableAutoMerge": true,
            },
        }}}))
    }
}

fn automation_read() -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .and(body_string_contains("autoMergeAllowed"))
}

/// GitHub answering every read of pull request 12's automation as `automation`.
async fn automate(github: &MockServer, automation: Automation) {
    automation_read()
        .respond_with(automation.answer())
        .mount(github)
        .await;
}

/// GitHub answering the read a change starts from as `before`, and every read
/// after it as `after`.
async fn automate_change(github: &MockServer, before: Automation, after: Automation) {
    automation_read()
        .respond_with(before.answer())
        .up_to_n_times(1)
        .with_priority(1)
        .mount(github)
        .await;
    automate(github, after).await;
}

/// GitHub carrying out the GraphQL mutation `name` on pull request 12, once.
async fn mutation(github: &MockServer, name: &str, variables: Value) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .and(body_string_contains(name))
        .and(body_partial_json(json!({"variables": variables})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {name: {"clientMutationId": null}}})),
        )
        .expect(1)
        .mount(github)
        .await;
}

async fn mutations(github: &MockServer) -> Vec<String> {
    github
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|request| String::from_utf8_lossy(&request.body).into_owned())
        .filter(|body| body.contains("mutation"))
        .collect()
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
    automate(&github, Automation::default()).await;
    discuss(&github, &[false, true, false]).await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/widgets/pulls"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 1,
            "number": 12,
            "html_url": link(&github, 12),
            "title": TITLE,
            "state": "open",
            "head": {"ref": "feature", "sha": HEAD},
            "base": {"ref": "main", "sha": HEAD},
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
    assert_eq!(row["writable"], true);
    assert_eq!(row["credentialed"], true);
    let status = &row["status"];
    assert_eq!(status["state"], "open", "{row}");
    assert_eq!(status["draft"], false);
    assert_eq!(status["additions"], 40);
    assert_eq!(status["deletions"], 3);
    assert_eq!(status["checks"], "passing");
    assert_eq!(status["mergeable"], "blocked");
    assert_eq!(
        status["auto_merge"],
        json!({"enabled": false, "unavailable": null})
    );
    assert_eq!(status["draft_unavailable"], Value::Null);
    assert_eq!(status["threads"], json!({"open": 2, "resolved": 1}));
    assert!(
        !listed.to_string().contains(GITHUB_TOKEN),
        "a token reached the response: {listed}"
    );
}

#[tokio::test]
async fn a_create_github_refuses_as_already_open_answers_with_the_open_pull_request() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/widgets/pulls"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Validation Failed",
            "errors": [{"message": "A pull request already exists for acme:feature."}],
        })))
        .expect(1)
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls"))
        .and(wiremock::matchers::query_param("head", "acme:feature"))
        .and(wiremock::matchers::query_param("base", "main"))
        .and(wiremock::matchers::query_param("state", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "node_id": NODE,
            "number": 12,
            "html_url": link(&github, 12),
            "state": "open",
            "title": TITLE,
            "draft": false,
            "head": {"ref": "feature", "label": "acme:feature", "sha": HEAD},
            "base": {"ref": "main", "label": "acme:main", "sha": HEAD},
        }])))
        .mount(&github)
        .await;
    let served = Served::start(&github).await;
    let source = served.workspace.connect_github().await;

    let turn = served.turn("Open the pull request again.").await;
    let (arguments, token) = served.agent.spawned().await;
    let called = rpc(
        &endpoint(&arguments),
        &token,
        json!({"name": "create_pull_request", "arguments": {
            "source_id": source,
            "title": TITLE,
            "head": "feature",
            "base": "main",
            "reason": "The first attempt timed out.",
        }}),
    )
    .await;
    served.agent.finish(&[]);
    turn.await.expect("the turn ends");

    assert_eq!(called["result"]["isError"], false, "{called}");
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("the tool's answer");
    let answer: Value = serde_json::from_str(text).expect("a JSON answer");
    assert_eq!(answer["already_open"], true, "{answer}");
    assert_eq!(answer["pull_request"]["number"], 12, "{answer}");
    assert_eq!(answer["pull_request"]["title"], TITLE, "{answer}");
    let listed = served.recorded(1).await;
    assert_eq!(listed[0]["number"], 12, "{listed}");
    assert_eq!(listed[0]["url"], link(&github, 12), "{listed}");
}

#[tokio::test]
async fn a_pull_requests_checks_whose_rows_shift_between_pages_read_as_pending() {
    const PAGE_ROWS: u64 = 100;
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/12"))
        .and(header("Authorization", format!("Bearer {GITHUB_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(detail(&github, false)))
        .mount(&github)
        .await;
    let run = |id: u64| json!({"id": id, "name": format!("ci-{id}"), "status": "completed", "conclusion": "success"});
    for (page, runs) in [
        ("1", (1..=PAGE_ROWS).map(run).collect::<Vec<_>>()),
        ("2", vec![run(PAGE_ROWS)]),
    ] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/repos/acme/widgets/commits/{HEAD}/check-runs"
            )))
            .and(wiremock::matchers::query_param("page", page))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": PAGE_ROWS + 1,
                "check_runs": runs,
            })))
            .expect(1..)
            .mount(&github)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/widgets/commits/{HEAD}/status")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "state": "success",
            "total_count": 0,
            "statuses": [],
        })))
        .mount(&github)
        .await;
    automate(&github, Automation::default()).await;
    discuss(&github, &[]).await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;

    assert_eq!(listed[0]["status"]["state"], "open", "{listed}");
    assert_eq!(
        listed[0]["status"]["checks"], "pending",
        "every run passed, but a run listed on both pages means runs moved between them, so \
         one could have been missed: {listed}"
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
    assert_eq!(listed[0]["writable"], true);
    assert_eq!(listed[0]["credentialed"], false);
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
async fn a_viewer_can_read_and_diff_a_chats_pull_requests_but_not_change_them() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/12"))
        .and(header("Accept", "application/vnd.github.diff"))
        .respond_with(ResponseTemplate::new(200).set_body_string("diff --git a/x b/x\n"))
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;
    let registered = workspace
        .client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let viewer = registered["access_token"]
        .as_str()
        .expect("a viewer token")
        .to_string();
    let user: Uuid = registered["user"]["id"]
        .as_str()
        .expect("a viewer id")
        .parse()
        .expect("a user id");
    workspace_members::add_member(
        workspace.state.db(),
        workspace.workspace,
        user,
        WorkspaceRole::Viewer,
        None,
    )
    .await
    .expect("a viewer");

    let listed = workspace.listed(&viewer).await;
    assert_eq!(listed[0]["writable"], false, "{listed}");
    assert_eq!(listed[0]["credentialed"], true, "{listed}");
    let diff = workspace
        .client
        .get_auth(&format!("{}/diff", workspace.pull_request(id)), &viewer)
        .await;
    assert_eq!(diff.status, StatusCode::OK, "{}", diff.text());
    let changed = workspace
        .client
        .patch_json_auth(
            &workspace.pull_request(id),
            &json!({"dismissed": true}),
            &viewer,
        )
        .await;
    assert_eq!(changed.status, StatusCode::FORBIDDEN, "{}", changed.text());
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
async fn merge_when_ready_sets_auto_merge_on_github_and_lists_it_as_enabled() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate_change(
        &github,
        Automation::default(),
        Automation {
            enabled: true,
            ..Automation::default()
        },
    )
    .await;
    mutation(
        &github,
        "enablePullRequestAutoMerge",
        json!({"input": {"pullRequestId": NODE, "mergeMethod": "SQUASH", "expectedHeadOid": HEAD}}),
    )
    .await;
    threads_read()
        .respond_with(threads_answer(&[true]))
        .expect(1)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let (status, changed) = workspace.update(id, json!({"auto_merge": true})).await;

    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        changed["status"]["auto_merge"],
        json!({"enabled": true, "unavailable": null}),
        "{changed}"
    );
    let listed = workspace.listed(&workspace.token).await;
    assert_eq!(
        listed[0]["status"]["auto_merge"]["enabled"], true,
        "{listed}"
    );
    assert_eq!(
        listed[0]["status"]["threads"],
        json!({"open": 0, "resolved": 1}),
        "{listed}"
    );
}

#[tokio::test]
async fn cancelling_auto_merge_takes_it_off_on_github() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate_change(
        &github,
        Automation {
            enabled: true,
            ..Automation::default()
        },
        Automation::default(),
    )
    .await;
    mutation(&github, "disablePullRequestAutoMerge", json!({"id": NODE})).await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let (status, changed) = workspace.update(id, json!({"auto_merge": false})).await;

    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        changed["status"]["auto_merge"],
        json!({"enabled": false, "unavailable": null}),
        "{changed}"
    );
}

#[tokio::test]
async fn convert_to_draft_turns_an_open_pull_request_into_a_draft_on_github() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(&github, Automation::default()).await;
    mutation(&github, "convertPullRequestToDraft", json!({"id": NODE})).await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let (status, changed) = workspace.update(id, json!({"draft": true})).await;

    assert_eq!(status, StatusCode::OK, "{changed}");
}

#[tokio::test]
async fn ready_for_review_takes_a_draft_out_of_draft_on_github() {
    let github = MockServer::start().await;
    describe(&github, true).await;
    automate(
        &github,
        Automation {
            draft: true,
            ..Automation::default()
        },
    )
    .await;
    mutation(
        &github,
        "markPullRequestReadyForReview",
        json!({"id": NODE}),
    )
    .await;
    threads_read()
        .respond_with(threads_answer(&[]))
        .expect(2)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;
    assert_eq!(listed[0]["status"]["draft"], true, "{listed}");
    assert_eq!(
        listed[0]["status"]["auto_merge"],
        json!({"enabled": false, "unavailable": AutoMerge::DRAFT}),
        "{listed}"
    );
    let (status, changed) = workspace.update(id, json!({"draft": false})).await;

    assert_eq!(status, StatusCode::OK, "{changed}");
}

#[tokio::test]
async fn a_change_already_made_asks_nothing_of_github() {
    let github = MockServer::start().await;
    describe(&github, true).await;
    automate(
        &github,
        Automation {
            draft: true,
            ..Automation::default()
        },
    )
    .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    for body in [json!({"draft": true}), json!({"auto_merge": false})] {
        let (status, changed) = workspace.update(id, body.clone()).await;
        assert_eq!(status, StatusCode::OK, "{body}: {changed}");
    }
    assert_eq!(mutations(&github).await, Vec::<String>::new());
}

#[tokio::test]
async fn a_repository_without_auto_merge_refuses_before_github_is_asked() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(
        &github,
        Automation {
            allowed: false,
            ..Automation::default()
        },
    )
    .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;
    assert_eq!(
        listed[0]["status"]["auto_merge"],
        json!({"enabled": false, "unavailable": AutoMerge::NOT_ALLOWED}),
        "{listed}"
    );
    let (status, refused) = workspace.update(id, json!({"auto_merge": true})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| error.contains(AutoMerge::NOT_ALLOWED)),
        "{refused}"
    );
    assert_eq!(mutations(&github).await, Vec::<String>::new());
}

#[tokio::test]
async fn githubs_refusal_to_set_auto_merge_reaches_the_reader() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(&github, Automation::default()).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("enablePullRequestAutoMerge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"enablePullRequestAutoMerge": null},
            "errors": [{"message": "Pull request Pull request is in dirty status"}],
        })))
        .expect(1)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    let id = workspace.record(&link(&github, 12)).await;

    let (status, refused) = workspace.update(id, json!({"auto_merge": true})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| error.contains("dirty status")),
        "{refused}"
    );
}

#[tokio::test]
async fn auto_merge_reads_as_unknown_when_github_will_not_describe_it() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automation_read()
        .respond_with(ResponseTemplate::new(502))
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;

    assert_eq!(listed[0]["status"]["state"], "open", "{listed}");
    assert_eq!(
        listed[0]["status"]["auto_merge"],
        json!({"enabled": false, "unavailable": AutoMerge::UNREAD}),
        "{listed}"
    );
    assert_eq!(listed[0]["status"]["draft_unavailable"], Value::Null);
}

#[tokio::test]
async fn review_threads_read_as_unknown_when_github_will_not_list_them() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(&github, Automation::default()).await;
    threads_read()
        .respond_with(ResponseTemplate::new(502))
        .expect(1..)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;

    assert_eq!(listed[0]["status"]["state"], "open", "{listed}");
    assert_eq!(listed[0]["status"]["threads"], Value::Null, "{listed}");
}

#[tokio::test]
async fn review_threads_read_as_unknown_when_github_stops_short_of_every_thread() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(&github, Automation::default()).await;
    threads_read()
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": {"repository": {
                "pullRequest": {"reviewThreads": {
                    "pageInfo": {"hasNextPage": true, "endCursor": null},
                    "nodes": [{"id": "PRRT_0", "isResolved": true}],
                }},
            }}})),
        )
        .expect(1..)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;

    assert_eq!(listed[0]["status"]["state"], "open", "{listed}");
    assert_eq!(listed[0]["status"]["threads"], Value::Null, "{listed}");
}

#[tokio::test]
async fn a_merged_pull_request_is_listed_without_reading_its_review_threads() {
    let github = MockServer::start().await;
    let mut merged = detail(&github, false);
    merged["state"] = json!("closed");
    merged["merged"] = json!(true);
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/12"))
        .respond_with(ResponseTemplate::new(200).set_body_json(merged))
        .mount(&github)
        .await;
    threads_read()
        .respond_with(threads_answer(&[false]))
        .expect(0)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;

    assert_eq!(listed[0]["status"]["state"], "merged", "{listed}");
    assert_eq!(listed[0]["status"]["threads"], Value::Null, "{listed}");
}

#[tokio::test]
async fn review_threads_counted_moments_ago_are_listed_without_asking_github_again() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(&github, Automation::default()).await;
    threads_read()
        .respond_with(threads_answer(&[false, true]))
        .expect(1)
        .mount(&github)
        .await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let first = workspace.listed(&workspace.token).await;
    let second = workspace.listed(&workspace.token).await;

    for listed in [first, second] {
        assert_eq!(
            listed[0]["status"]["threads"],
            json!({"open": 1, "resolved": 1}),
            "{listed}"
        );
    }
}

#[tokio::test]
async fn a_pull_request_without_review_threads_counts_none() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    automate(&github, Automation::default()).await;
    discuss(&github, &[]).await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    workspace.connect_github().await;
    workspace.record(&link(&github, 12)).await;

    let listed = workspace.listed(&workspace.token).await;

    assert_eq!(
        listed[0]["status"]["threads"],
        json!({"open": 0, "resolved": 0}),
        "{listed}"
    );
}

#[tokio::test]
async fn a_source_without_a_credential_never_stands_in_for_one_that_has_one() {
    let github = MockServer::start().await;
    describe(&github, false).await;
    let database = context_database_url();
    let pool = PgPool::connect(&database).await.expect("the test database");
    let workspace = Workspace::new(config(&github, database), pool).await;
    let bare = sources::create_source(
        workspace.state.db(),
        workspace.workspace,
        &format!("widgets-bare-{}", Uuid::new_v4()),
        "github",
        json!({"owner": "acme", "repo": "widgets"}),
        None,
        None,
        None,
    )
    .await
    .expect("a source without a credential")
    .id;
    workspace.connect_github().await;
    let id = chat_pull_requests::record(
        workspace.state.db(),
        &NewPullRequest {
            chat_id: workspace.chat,
            owner: "acme",
            repo: "widgets",
            number: 12,
            url: &link(&github, 12),
            title: None,
            source_id: Some(bare),
        },
    )
    .await
    .expect("a pull request opened through the bare source")
    .id;

    let listed = workspace.listed(&workspace.token).await;
    assert_eq!(listed[0]["id"], id.to_string(), "{listed}");
    assert_eq!(listed[0]["credentialed"], true, "{listed}");
    assert_eq!(listed[0]["status"]["state"], "open", "{listed}");
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
