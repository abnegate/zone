//! A runner fleet on one server: an [`AppState`] per runner, each with an identity of its own
//! and registered, sharing one database and one model provider that answers every turn. Fleet
//! tables are server-wide, so every fixture has a database of its own. The first account
//! registered is the server admin, and owns the organization, workspace and chat the work runs
//! in.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_enterprise::fleet::{Labels, Refusal, Version};
use zone_server::config::Config;
use zone_server::db::fleet::minimum::Minimum;
use zone_server::db::fleet::state::State;
use zone_server::db::fleet::{eligibility, labels, policy};
use zone_server::db::tasks::{self, TaskRunRow};
use zone_server::db::{licenses, migrations};
use zone_server::services::fleet::identity::version;
use zone_server::services::fleet::{Identity, registration};
use zone_server::state::AppState;
use zone_server::workers::task;

use super::context::{Socket, finish, next, send};
use super::sso::{self, Account, Audit};
use super::{Database, TestClient, TestResponse, serve, test_config, test_email, test_password};

/// The model chats and reminder turns ask.
pub const CHAT_MODEL: &str = "fleet-test";
/// The model task runs ask.
pub const TASK_MODEL: &str = "gpt-4";
/// What the provider answers every turn with, unless it was scripted otherwise.
pub const ANSWER: &str = "Finished the work.";
pub const COMPLETED: &str = "completed";
pub const FAILED: &str = "failed";
pub const RUNNING: &str = "running";
pub const WAITING: &str = "waiting";

const COMPLETIONS: &str = "/chat/completions";
const CONTEXT_LENGTH: u64 = 32_768;
const POLL: Duration = Duration::from_millis(50);
const SETTLES_WITHIN: Duration = Duration::from_secs(60);

/// What one runner registers as: its version (this build's when `None`) and the labels it
/// carries, as `ZONE_RUNNER_LABELS` would list them.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub version: Option<&'static str>,
    pub labels: &'static str,
}

impl Spec {
    pub const CURRENT: Self = Self {
        version: None,
        labels: "",
    };

    pub const fn labelled(labels: &'static str) -> Self {
        Self {
            version: None,
            labels,
        }
    }

    pub const fn on(version: &'static str) -> Self {
        Self {
            version: Some(version),
            labels: "",
        }
    }

    fn identity(self, index: usize) -> Identity {
        Identity {
            name: format!("runner-{index}-{}", Uuid::new_v4().simple()),
            version: self.version.map_or_else(version, |version| {
                Version::parse(version).expect("a runner version")
            }),
            labels: Labels::parse_list(self.labels).expect("runner labels"),
        }
    }
}

/// One server process of the fleet.
#[derive(Clone)]
pub struct Runner {
    pub state: AppState,
    pub id: Uuid,
}

/// A completion the provider serves in place of [`ANSWER`]: a tool call.
struct Call {
    tool: String,
    arguments: String,
}

/// What the provider answers streamed turns with: queued tool calls first, then [`ANSWER`],
/// every response held back by the current delay.
#[derive(Clone, Default)]
struct Script {
    calls: Arc<Mutex<VecDeque<Call>>>,
    delay: Arc<AtomicU64>,
    round: Arc<AtomicU64>,
}

impl Script {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        if body["stream"] != json!(true) {
            return ResponseTemplate::new(200).set_body_json(json!({
                "id": "completion", "object": "chat.completion", "created": 0, "model": body["model"],
                "choices": [{"index": 0, "message": {"role": "assistant", "content": ANSWER}, "finish_reason": "stop"}]
            }));
        }
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        let deltas = match self.calls.lock().unwrap().pop_front() {
            Some(Call { tool, arguments }) => vec![json!({"tool_calls": [{
                "index": 0, "id": format!("call-{round}"), "type": "function",
                "function": {"name": tool, "arguments": arguments}
            }]})],
            None => vec![json!({"content": ANSWER})],
        };
        let delay = Duration::from_millis(self.delay.load(Ordering::SeqCst));
        streamed(&body["model"], deltas).set_delay(delay)
    }
}

fn streamed(model: &Value, deltas: Vec<Value>) -> ResponseTemplate {
    let chunk = |delta: Value, finish: Value| {
        json!({"id": "completion", "object": "chat.completion.chunk", "created": 0, "model": model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
    };
    let mut body = String::new();
    for delta in deltas {
        body.push_str(&format!("data: {}\n\n", chunk(delta, Value::Null)));
    }
    let end = chunk(json!({}), json!("stop"));
    let usage = json!({"model": model, "choices": [],
        "usage": {"prompt_tokens": 37, "completion_tokens": 9, "total_tokens": 46}});
    body.push_str(&format!("data: {end}\n\ndata: {usage}\n\ndata: [DONE]\n\n"));
    ResponseTemplate::new(200)
        .insert_header("Content-Type", "text/event-stream")
        .set_body_string(body)
}

pub struct Fleet {
    pub database: Database,
    pub provider: MockServer,
    pub config: Config,
    pub runners: Vec<Runner>,
    /// The routes, served by the first runner.
    pub client: TestClient,
    pub admin: Account,
    pub organization: Uuid,
    pub workspace: Uuid,
    pub chat: Uuid,
    script: Script,
}

impl Fleet {
    /// A runner registered for each of `specs`, in order, on a server without a license.
    pub async fn new(prefix: &'static str, specs: &[Spec]) -> Self {
        let database = Database::new(prefix).await;
        migrations::run(&database.pool).await.unwrap();
        let script = Script::default();
        let provider = provider(&script).await;
        let mut config = test_config();
        config.litellm_host = provider.uri();
        config.ollama_host = provider.uri();
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        let mut runners = Vec::with_capacity(specs.len());
        for (index, spec) in specs.iter().enumerate() {
            let state = Self::start(&config, &database.pool, spec.identity(index)).await;
            let id = registration::register(&state).await.unwrap();
            runners.push(Runner { state, id });
        }
        let client = TestClient::with_state(runners[0].state.clone());
        let admin = register(&client).await;
        let organization = create_organization(&client, &admin).await;
        let workspace = create_workspace(&client, &admin, organization).await;
        let chat = create_chat(&client, &admin, workspace).await;
        Self {
            database,
            provider,
            config,
            runners,
            client,
            admin,
            organization,
            workspace,
            chat,
            script,
        }
    }

    /// A server process with `identity`, which has read the stored license and not yet
    /// registered.
    async fn start(config: &Config, pool: &PgPool, identity: Identity) -> AppState {
        let state = AppState::new(config.clone(), pool.clone(), None);
        state.disable_mcp();
        state.install_keyring(sso::keyring());
        state.set_identity(identity);
        state.licenses().load(pool).await.unwrap();
        state
    }

    /// Another server process on this fleet's database that hasn't registered as a runner.
    pub async fn unregistered(&self, spec: Spec) -> AppState {
        let identity = spec.identity(self.runners.len());
        Self::start(&self.config, self.pool(), identity).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    pub fn runner(&self, index: usize) -> &Runner {
        &self.runners[index]
    }

    /// Stores `key` and has every runner read it, as each would on its next license read.
    pub async fn license(&self, key: &str) {
        licenses::replace(self.pool(), key).await.unwrap();
        self.reload().await;
    }

    /// Removes the license through the admin route, as an admin would.
    pub async fn remove_license(&self) {
        self.client
            .delete_auth("/api/license", &self.admin.token)
            .await
            .assert_status(StatusCode::OK);
        self.reload().await;
    }

    async fn reload(&self) {
        for runner in &self.runners {
            runner.state.licenses().load(self.pool()).await.unwrap();
        }
    }

    /// Re-registers every runner, so each counts as connected however long the test has run.
    pub async fn heartbeat(&self) {
        for runner in &self.runners {
            registration::register(&runner.state).await.unwrap();
        }
    }

    /// Holds every streamed completion back by `delay`, so the turn asking it stays live.
    pub fn hold(&self, delay: Duration) {
        let millis = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
        self.script.delay.store(millis, Ordering::SeqCst);
    }

    /// The next streamed completion calls `tool` with `arguments` instead of answering.
    pub fn call(&self, tool: &str, arguments: Value) {
        self.script.calls.lock().unwrap().push_back(Call {
            tool: tool.to_string(),
            arguments: arguments.to_string(),
        });
    }

    /// How many streamed completions the provider was asked for: one per turn a runner ran.
    pub async fn streamed(&self) -> usize {
        self.provider
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == COMPLETIONS)
            .filter(|request| {
                serde_json::from_slice::<Value>(&request.body)
                    .is_ok_and(|body| body["stream"] == json!(true))
            })
            .count()
    }

    /// Changes a runner's state through the admin route, confirming past the lockout guard.
    pub async fn set_state(&self, runner: Uuid, state: State) -> Value {
        let response = self
            .client
            .patch_json_auth(
                &format!("/api/runners/{runner}"),
                &json!({"state": state.as_str(), "confirm": true}),
                &self.admin.token,
            )
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    /// Sets the fleet's minimum version through the admin route, confirming past the lockout
    /// guard, or clears it.
    pub async fn set_minimum(&self, minimum: Option<&str>) {
        self.client
            .patch_json_auth(
                "/api/fleet-policy",
                &json!({"minimum_version": minimum, "confirm": true}),
                &self.admin.token,
            )
            .await
            .assert_status(StatusCode::OK);
    }

    /// The labels `organization` requires, saved through its route.
    pub async fn require_organization(&self, organization: Uuid, required: &[&str]) {
        self.client
            .patch_json_auth(
                &format!("/api/organizations/{organization}/runner-labels"),
                &json!({"labels": required}),
                &self.admin.token,
            )
            .await
            .assert_status(StatusCode::OK);
    }

    /// The labels `workspace` of `organization` requires, saved through its route.
    pub async fn require_workspace(&self, organization: Uuid, workspace: Uuid, required: &[&str]) {
        self.client
            .patch_json_auth(
                &format!("/api/organizations/{organization}/workspaces/{workspace}/runner-labels"),
                &json!({"labels": required}),
                &self.admin.token,
            )
            .await
            .assert_status(StatusCode::OK);
    }

    /// Writes a runner's state as it is stored, past every route and license check.
    pub async fn seed_state(&self, runner: Uuid, state: State) {
        sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
            .bind(runner)
            .bind(state.as_str())
            .execute(self.pool())
            .await
            .unwrap();
    }

    /// Writes the fleet's minimum version as it is stored, past every route and license check.
    pub async fn seed_minimum(&self, minimum: &str) {
        let minimum = Minimum::parse(minimum).unwrap();
        let mut transaction = self.pool().begin().await.unwrap();
        policy::save(&mut transaction, Some(&minimum), self.admin.id)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }

    /// Writes the labels the fixture's workspace and its organization require, past every route
    /// and license check.
    pub async fn seed_labels(&self, organization: &str, workspace: &str) {
        let mut transaction = self.pool().begin().await.unwrap();
        labels::save_organization(
            &mut transaction,
            self.organization,
            &Labels::parse_list(organization).unwrap(),
            self.admin.id,
        )
        .await
        .unwrap();
        labels::save_workspace(
            &mut transaction,
            self.workspace,
            &Labels::parse_list(workspace).unwrap(),
            self.admin.id,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }

    pub async fn another_workspace(&self, organization: Uuid) -> Uuid {
        create_workspace(&self.client, &self.admin, organization).await
    }

    /// Another organization the admin owns, and a workspace in it.
    pub async fn another_organization(&self) -> (Uuid, Uuid) {
        let organization = create_organization(&self.client, &self.admin).await;
        let workspace = create_workspace(&self.client, &self.admin, organization).await;
        (organization, workspace)
    }

    /// An agentic task in `workspace` that asks [`TASK_MODEL`].
    pub async fn task(&self, workspace: Uuid) -> Uuid {
        let task = tasks::create_task_as(
            self.pool(),
            workspace,
            &[],
            "Fleet",
            "Ship the change",
            None,
            None,
            true,
            None,
            Some(self.admin.id),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1")
            .bind(task.id)
            .bind(TASK_MODEL)
            .execute(self.pool())
            .await
            .unwrap();
        task.id
    }

    /// A run of `task` the admin admitted, not yet claimed by any runner.
    pub async fn admit(&self, task: Uuid) -> TaskRunRow {
        tasks::create_task_run_as(self.pool(), task, Some(self.admin.id))
            .await
            .unwrap()
    }

    /// A run admitted for a new task in `workspace`.
    pub async fn admit_in(&self, workspace: Uuid) -> TaskRunRow {
        let task = self.task(workspace).await;
        self.admit(task).await
    }

    /// Runs `run` on `server` as an admitting runner does, through to its end.
    pub async fn execute(&self, server: &AppState, run: &TaskRunRow) {
        task::execute_task_run(server, run.id, run.task_id).await;
    }

    /// Starts `run` on `server` as an admitting runner does, without waiting for it.
    pub fn spawn(&self, server: &AppState, run: &TaskRunRow) -> tokio::task::JoinHandle<()> {
        let state = server.clone();
        let (run, task) = (run.id, run.task_id);
        tokio::spawn(async move { task::execute_task_run(&state, run, task).await })
    }

    pub async fn run(&self, run: Uuid) -> TaskRunRow {
        tasks::get_task_run(self.pool(), run)
            .await
            .unwrap()
            .expect("the run")
    }

    /// The owner that claimed `run` and the runner it claimed it on.
    pub async fn claimed(&self, run: Uuid) -> (Option<Uuid>, Option<Uuid>) {
        sqlx::query_as("SELECT owner, runner_id FROM task_runs WHERE id = $1")
            .bind(run)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    /// The status of `task` and the run it holds reserved.
    pub async fn reservation(&self, task: Uuid) -> (String, Option<Uuid>) {
        sqlx::query_as("SELECT status, active_run_id FROM tasks WHERE id = $1")
            .bind(task)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    /// Ages `run` past the moment another runner may pick it up.
    pub async fn settle(&self, run: Uuid) {
        let aged = eligibility::PICKUP_AFTER + chrono::TimeDelta::seconds(1);
        sqlx::query("UPDATE task_runs SET heartbeat_at = now() - $2 WHERE id = $1")
            .bind(run)
            .bind(aged)
            .execute(self.pool())
            .await
            .unwrap();
    }

    /// The runs `runner` would pick up now.
    pub async fn candidates(&self, runner: Uuid) -> Vec<Uuid> {
        eligibility::candidates(self.pool(), runner, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.run)
            .collect()
    }

    /// Waits for `run` to reach `status`, failing with where it stands if it never does.
    pub async fn settles_on(&self, run: Uuid, status: &str) -> TaskRunRow {
        let deadline = tokio::time::Instant::now() + SETTLES_WITHIN;
        loop {
            let row = self.run(run).await;
            if row.status == status {
                return row;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the run never became {status}: {row:?}"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    /// Waits for `run` to be claimed, failing if it never is.
    pub async fn until_claimed(&self, run: Uuid) -> (Uuid, Uuid) {
        let deadline = tokio::time::Instant::now() + SETTLES_WITHIN;
        loop {
            if let (Some(owner), Some(runner)) = self.claimed(run).await {
                return (owner, runner);
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the run was never claimed"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    /// Serves `server`'s routes on a port of their own, for its chat socket.
    pub async fn serve(&self, server: &AppState) -> String {
        serve(server.clone()).await
    }

    /// Sends `content` to the fixture's chat over the socket `address` serves, and the frames
    /// up to the turn's end.
    pub async fn turn(&self, address: &str, content: &str) -> Vec<Value> {
        let mut socket = self.connect(address).await;
        send(&mut socket, json!({"type": "send", "content": content})).await;
        let frames = finish(&mut socket).await;
        let _ = socket.close(None).await;
        frames
    }

    /// A socket on the fixture's chat at `address`, signed in as the admin, past its first frame.
    pub async fn connect(&self, address: &str) -> Socket {
        let (mut socket, _) = connect_async(format!("ws://{address}/ws/chats/{}", self.chat))
            .await
            .unwrap();
        send(
            &mut socket,
            json!({"type": "auth", "token": self.admin.token}),
        )
        .await;
        let initial = next(&mut socket).await;
        assert_eq!(initial["type"], "init", "initial frame: {initial}");
        socket
    }

    /// Adds a message to the fixture's chat through `server`'s route, which leases the chat.
    pub async fn post_message(&self, server: &AppState, content: &str) -> TestResponse {
        TestClient::with_state(server.clone())
            .post_json_auth(
                &format!("/api/chats/{}/messages", self.chat),
                &json!({"role": "user", "content": content}),
                &self.admin.token,
            )
            .await
    }

    /// Every message the fixture's chat holds, as role and content, oldest first.
    pub async fn messages(&self) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT role, content FROM messages WHERE chat_id = $1 ORDER BY created_at, id",
        )
        .bind(self.chat)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    /// Chat leases `runner` holds that haven't expired.
    pub async fn live_leases(&self, runner: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_leases WHERE runner_id = $1 AND expires_at > clock_timestamp()",
        )
        .bind(runner)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    /// Task runs live on `runner`.
    pub async fn live_runs(&self, runner: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_runs WHERE runner_id = $1 AND status IN ('running', 'waiting')",
        )
        .bind(runner)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    pub async fn audits(&self, action: &str) -> Vec<Audit> {
        sqlx::query_as(
            "SELECT actor_id, organization_id, resource_type, resource_id, old_values, new_values
             FROM audit_logs WHERE action = $1 ORDER BY created_at",
        )
        .bind(action)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    pub async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

/// Whether the frames of a turn carry the denial `refusal` gives, with its message and code.
pub fn refused_with(frames: &[Value], refusal: &Refusal) -> bool {
    frames.iter().any(|frame| {
        frame["type"] == "denied"
            && frame["message"] == refusal.message()
            && frame["code"] == refusal.code()
    })
}

async fn provider(script: &Script) -> MockServer {
    let provider = MockServer::start().await;
    let responder = script.clone();
    Mock::given(method("POST"))
        .and(path(COMPLETIONS))
        .respond_with(move |request: &Request| responder.respond(request))
        .mount(&provider)
        .await;
    let uri = provider.uri();
    Mock::given(method("GET"))
        .and(path("/v2/model/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{
            "model_name": CHAT_MODEL,
            "litellm_params": {"model": format!("ollama_chat/{CHAT_MODEL}"), "api_base": uri, "num_ctx": CONTEXT_LENGTH},
            "model_info": {"id": "fleet-route"}
        }]})))
        .mount(&provider)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/ps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
        .mount(&provider)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "capabilities": ["completion", "tools"],
            "model_info": {"general.architecture": "test", "test.context_length": CONTEXT_LENGTH}
        })))
        .mount(&provider)
        .await;
    provider
}

async fn register(client: &TestClient) -> Account {
    let email = test_email();
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({"email": email, "password": test_password()}),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Account {
        id: body["user"]["id"].as_str().unwrap().parse().unwrap(),
        email,
        token: body["access_token"].as_str().unwrap().to_owned(),
    }
}

async fn create_organization(client: &TestClient, admin: &Account) -> Uuid {
    let slug = format!("fleet-{}", Uuid::new_v4().simple());
    let created = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Fleet", "slug": slug}),
            &admin.token,
        )
        .await
        .json_value();
    created["organization"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("the organization was not created: {created}"))
        .parse()
        .unwrap()
}

async fn create_workspace(client: &TestClient, admin: &Account, organization: Uuid) -> Uuid {
    let slug = format!("fleet-{}", Uuid::new_v4().simple());
    let created = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Fleet", "slug": slug}),
            &admin.token,
        )
        .await
        .json_value();
    created["workspace"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("the workspace was not created: {created}"))
        .parse()
        .unwrap()
}

async fn create_chat(client: &TestClient, admin: &Account, workspace: Uuid) -> Uuid {
    let created = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace, "title": "Fleet", "model_name": CHAT_MODEL,
                "agent_enabled": false, "automatic_title": false, "auto_approve": true}),
            &admin.token,
        )
        .await
        .json_value();
    created["chat"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("the chat was not created: {created}"))
        .parse()
        .unwrap()
}
