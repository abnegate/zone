//! A CLI-backed chat turn, and the tools it serves the agent it spawns.
//!
//! The pieces are covered on their own elsewhere: `mcp::Turn` decides a call,
//! `CliSettings` carries a toolset to a child process, and the chat routes
//! carry `agent_sandboxed`. What is only true end to end is that a turn mints
//! a lease at all, points the agent at this server, keeps the token alive for
//! exactly as long as the agent may call with it, and shows the reader what
//! the agent did. This drives that through the real websocket, against a
//! stand-in for the host's `claude` that calls back the way the real one does.

use crate::common;

use crate::common::context::{Socket, finish, next, send, successful};
use crate::common::stand_in::{MCP_CONFIGURATION, MCP_TOKEN};
use crate::common::{
    TestClient, context_database_url, create_test_router, create_test_state, test_config,
    test_email, test_password,
};

use abnegate_agent_cli::Occupancy;
use abnegate_agent_cli::{AgentKind, CliSettings};
use axum::http::StatusCode;
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::stat;
use nix::unistd::mkfifo;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::{LlmBackend, Toolset};
use zone_server::agent::ASK_USER;
use zone_server::agent::wait::WAIT_FOR;
use zone_server::config::{AgentConfig, ModelBackend};
use zone_server::db::agent_logins;
use zone_server::db::ai_settings;
use zone_server::db::chats;
use zone_server::db::chats::occupancy;
use zone_server::services::chat::session::{self, Mode};
use zone_server::services::route::Route;

/// Longer than the thirty minutes a coding agent's turn gets by default.
const CHAT_TIMEOUT: Duration = Duration::from_secs(3600);

/// A chat's whole time, short enough for a test to outlast.
const SHORT_CHAT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the test waits for the spawned agent to report how it was invoked.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long Stop has to end a turn, far past what a turn at rest takes.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

const ANSWER: &str = "Written.";

/// What a stand-in claude had written of a later turn when it exited.
const UNFINISHED: &str = "Still writ";

/// Where the server keeps its organizations' agent homes, under the test's directory.
const AGENTS: &str = "agents";

/// What a Codex login holds in its home once signed in.
const CODEX_CREDENTIALS: &str = r#"{"tokens":{"access_token":"fake-codex-access-token","refresh_token":"fake-codex-refresh-token","account_id":"fake-codex-account"}}"#;
const CODEX_CREDENTIALS_FILE: &str = "auth.json";

/// The thread a stand-in Codex announces, and the day its rollout is kept under.
const THREAD: &str = "019b2c41-0000-7000-8000-0000000000c8";
const SESSIONS: &str = "sessions/2026/11/07";

/// The window Codex's rollout reports for [`THREAD`], and how much of it the turn filled.
const CODEX_WINDOW: u64 = 475_000;
const CODEX_FILLED: u64 = 24_638;

/// How long a stand-in Codex lingers after the line that finished its turn before it exits.
const LINGER: Duration = Duration::from_secs(1);

/// A stand-in for the host's `claude`.
///
/// It records how zone invoked it, then holds the turn open until the test
/// releases it, so the token is exercised while the turn that minted it is
/// still running -- which is the only moment at which it is supposed to work.
struct Agent {
    executable: PathBuf,
    prompt: PathBuf,
    arguments: PathBuf,
    token: PathBuf,
    release: PathBuf,
    directory: PathBuf,
}

impl Agent {
    fn write(directory: &Path) -> Self {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let agent = Self {
            executable: directory.join("agent"),
            prompt: directory.join("prompt"),
            arguments: directory.join("arguments"),
            token: directory.join("token"),
            release: directory.join("release"),
            directory: directory.join("directory"),
        };
        let mut file = std::fs::File::create(&agent.executable).expect("the stand-in agent");
        writeln!(
            file,
            "#!/bin/sh\ncat > {prompt}\npwd -P > {directory}\nprintf '%s' \"{MCP_TOKEN}\" > \
             {token}\n{{ printf '%s' \"$*\"; {MCP_CONFIGURATION}; }} > {arguments}.partial\nmv \
             {arguments}.partial {arguments}\nwhile [ ! -f {release} ]; do sleep 0.05; done\necho \
             '{assistant}'\necho '{result}'",
            directory = agent.directory.display(),
            prompt = agent.prompt.display(),
            arguments = agent.arguments.display(),
            token = agent.token.display(),
            release = agent.release.display(),
            assistant = json!({
                "type": "assistant",
                "message": {"content": [{"type": "text", "text": ANSWER}]},
            }),
            result = json!({"type": "result", "subtype": "success", "is_error": false}),
        )
        .expect("the stand-in agent body");
        drop(file);
        std::fs::set_permissions(&agent.executable, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in agent to be executable");
        agent
    }

    /// Wait until the agent has been spawned, and report what zone told it:
    /// its argument list, and the token in its environment, which is empty
    /// when zone served it no tools to reach with one.
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

    fn finish(&self) {
        std::fs::write(&self.release, "go").expect("releasing the agent");
    }

    /// Turns the stand-in into one that answers at once and reports its
    /// model's window the way claude's stream does.
    fn reporting(&self) {
        let assistant = json!({
            "type": "assistant",
            "message": {
                "model": "claude-sonnet-4-6",
                "content": [{"type": "text", "text": ANSWER}],
                "usage": {
                    "input_tokens": 3,
                    "cache_creation_input_tokens": 120,
                    "cache_read_input_tokens": 16_500,
                    "output_tokens": 9,
                },
            },
            "parent_tool_use_id": null,
        });
        let result = json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "modelUsage": {
                "claude-sonnet-4-6": {"contextWindow": 200_000, "maxOutputTokens": 32_000},
                "claude-fable-5-1": {"contextWindow": 1_000_000, "maxOutputTokens": 64_000},
            },
        });
        std::fs::write(
            &self.executable,
            format!("#!/bin/sh\ncat > /dev/null\necho '{assistant}'\necho '{result}'\n"),
        )
        .expect("the reporting stand-in");
    }

    /// Turns the stand-in into a claude that answers, begins a later turn and exits while it is
    /// still writing it, which leaves the answer it gave cut short.
    fn exiting_mid_turn(&self) {
        let lines = [
            json!({"type": "system", "subtype": "init", "session_id": "s"}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": ANSWER}]}, "parent_tool_use_id": null}),
            json!({"type": "result", "subtype": "success", "is_error": false, "result": ANSWER}),
            json!({"type": "system", "subtype": "init", "session_id": "s"}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": UNFINISHED}]}, "parent_tool_use_id": null}),
        ]
        .map(|line| format!("echo '{line}'"))
        .join("\n");
        std::fs::write(
            &self.executable,
            format!("#!/bin/sh\ncat > /dev/null\n{lines}\n"),
        )
        .expect("the stand-in that exits mid-turn");
    }

    /// Turns the stand-in into a claude that answers while a background command it started runs
    /// on, and never ends.
    fn answering_over_endless_background_work(&self) {
        let lines = [
            json!({"type": "system", "subtype": "init", "session_id": "s"}),
            json!({"type": "system", "subtype": "background_tasks_changed", "tasks": [{"task_id": "bg1", "task_type": "local_bash", "description": "server"}], "session_id": "s"}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": ANSWER}]}, "parent_tool_use_id": null}),
            json!({"type": "result", "subtype": "success", "is_error": false, "result": ANSWER}),
        ]
        .map(|line| format!("echo '{line}'"))
        .join("\n");
        std::fs::write(
            &self.executable,
            format!("#!/bin/sh\ncat > /dev/null\n{lines}\nexec sleep 600\n"),
        )
        .expect("the stand-in whose background work never ends");
    }

    /// Turns the stand-in into a Codex that answers at once, lingers after the line that finishes
    /// its turn, and keeps its window in the rollout under its home, as Codex does: read only
    /// once the agent has exited, after that line.
    fn reporting_once_it_exits(&self) {
        let rollout = json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {"total_tokens": CODEX_FILLED},
                    "model_context_window": CODEX_WINDOW,
                },
            },
        });
        let lines = [
            json!({"type": "thread.started", "thread_id": THREAD}),
            json!({"type": "turn.started"}),
            json!({"type": "item.completed", "item": {"id": "item_0", "type": "agent_message", "text": ANSWER}}),
            json!({"type": "turn.completed", "usage": {"input_tokens": 1, "cached_input_tokens": 0, "output_tokens": 1}}),
        ]
        .iter()
        .map(|line| format!("echo '{line}'\n"))
        .collect::<String>();
        std::fs::write(
            &self.executable,
            format!(
                "#!/bin/sh\ncat > /dev/null\nmkdir -p \"$CODEX_HOME/{SESSIONS}\"\necho \
                 '{rollout}' > \"$CODEX_HOME/{SESSIONS}/rollout-2026-11-07T10-00-00-{THREAD}.jsonl\"\n\
                 {lines}sleep {linger}\n",
                linger = LINGER.as_secs(),
            ),
        )
        .expect("the reporting codex stand-in");
    }

    /// Turns the stand-in into one that forks a child of its own before it
    /// holds the turn open, and says which processes those are.
    fn forking(&self) -> Processes {
        use std::os::unix::fs::PermissionsExt;

        let directory = self.executable.parent().expect("the stand-in's directory");
        let processes = Processes {
            leader: directory.join("leader"),
            forked: directory.join("forked"),
        };
        std::fs::write(
            &self.executable,
            format!(
                "#!/bin/sh\ncat > /dev/null\nprintf '%s' \"{MCP_TOKEN}\" > {token}\n{{ printf \
                 '%s' \"$*\"; {MCP_CONFIGURATION}; }} > {arguments}\nsleep 600 > /dev/null 2>&1 \
                 &\nprintf '%s' \"$!\" > \
                 {forked}\nprintf '%s' \"$$\" > {leader}\nwhile :; do sleep 0.05; done\n",
                token = self.token.display(),
                arguments = self.arguments.display(),
                forked = processes.forked.display(),
                leader = processes.leader.display(),
            ),
        )
        .expect("the forking stand-in");
        std::fs::set_permissions(&self.executable, std::fs::Permissions::from_mode(0o755))
            .expect("the forking stand-in to be executable");
        processes
    }
}

/// Where a forking stand-in wrote its own pid and its child's.
struct Processes {
    leader: PathBuf,
    forked: PathBuf,
}

impl Processes {
    async fn pids(&self) -> [String; 2] {
        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while Instant::now() < deadline {
            let forked = std::fs::read_to_string(&self.forked).unwrap_or_default();
            let leader = std::fs::read_to_string(&self.leader).unwrap_or_default();
            if !forked.is_empty() && !leader.is_empty() {
                return [leader, forked];
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the forking stand-in never reported its processes");
    }
}

/// Whether `pid` still runs. A killed process its parent has not reaped yet is
/// a zombie, which is gone for this purpose.
fn running(pid: &str) -> bool {
    let state = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .expect("ps");
    let state = String::from_utf8_lossy(&state.stdout);
    !state.trim().is_empty() && !state.trim_start().starts_with('Z')
}

async fn gone(pid: &str) -> bool {
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        if !running(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

/// A pipe only the test writes to. A tool that reads it waits there until the
/// test lets it go, however fast the host's disk is.
struct Held {
    path: PathBuf,
    _directory: TempDir,
}

impl Held {
    fn new() -> Self {
        let directory = TempDir::new().expect("a directory for the held pipe");
        let path = directory.path().join("held");
        mkfifo(&path, stat::Mode::S_IRUSR | stat::Mode::S_IWUSR).expect("the held pipe");
        Self {
            path,
            _directory: directory,
        }
    }

    /// Wait until a tool opens the pipe to read it, and hand back its
    /// writing end: the reader waits for as long as the test keeps that open,
    /// and is let go once it is dropped.
    async fn reached(&self) -> Option<File> {
        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while Instant::now() < deadline {
            match OpenOptions::new()
                .write(true)
                .custom_flags(OFlag::O_NONBLOCK.bits())
                .open(&self.path)
            {
                Ok(writer) => return Some(writer),
                Err(error) if error.raw_os_error() == Some(Errno::ENXIO as i32) => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(error) => panic!("the held pipe could not be opened: {error}"),
            }
        }
        None
    }
}

/// The MCP server definition zone passed on the command line, as the agent
/// would have read it: where to reach zone, and which tools it may call.
fn served(arguments: &str) -> (String, Vec<String>) {
    let endpoint = between(arguments, r#""url":""#, '"').expect("zone named its MCP endpoint");
    let allowed = arguments
        .split("--allowedTools ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect();
    (endpoint, allowed)
}

fn between(haystack: &str, after: &str, until: char) -> Option<String> {
    let rest = haystack.split_once(after)?.1;
    Some(rest.split(until).next()?.to_string())
}

async fn rpc(endpoint: &str, token: &str, method: &str, params: Value) -> (StatusCode, Value) {
    let response = reqwest::Client::new()
        .post(endpoint)
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
        .send()
        .await
        .expect("zone's MCP endpoint answers");
    let status = response.status();
    (
        StatusCode::from_u16(status.as_u16()).expect("a status"),
        response.json().await.unwrap_or(Value::Null),
    )
}

struct Harness {
    pool: PgPool,
    agents: AgentConfig,
    organization: Uuid,
    workspace: String,
    token: String,
    chat: Uuid,
    address: String,
    agent: Agent,
    _provider: MockServer,
    _directory: TempDir,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Harness {
    /// The port is bound before the config is built, because the address the
    /// agent is pointed at is the one this server was configured to listen on.
    async fn start(agent_enabled: bool, sandboxed: bool) -> Self {
        Self::start_on(AgentKind::Claude, agent_enabled, sandboxed).await
    }

    async fn start_on(kind: AgentKind, agent_enabled: bool, sandboxed: bool) -> Self {
        Self::start_within(kind, agent_enabled, sandboxed, None).await
    }

    /// A harness whose chats get `timeout`, in place of the configured default.
    async fn start_within(
        kind: AgentKind,
        agent_enabled: bool,
        sandboxed: bool,
        timeout: Option<Duration>,
    ) -> Self {
        let provider = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&provider)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
            .mount(&provider)
            .await;
        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"rate_limit": {"primary_window": {"used_percent": 10.0}}}),
                ),
            )
            .mount(&provider)
            .await;

        let directory = TempDir::new().expect("a temporary directory");
        let agent = Agent::write(directory.path());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener.local_addr().expect("the bound address");

        let database = context_database_url();
        let pool = PgPool::connect(&database).await.expect("the test database");
        let mut config = test_config();
        config.host = address.ip().to_string();
        config.port = address.port();
        config.database_url = database;
        config.litellm_host = provider.uri();
        config.ollama_host = provider.uri();
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        config.model_backend = ModelBackend::Cli {
            agent: kind,
            executable: Some(agent.executable.clone()),
        };
        if let Some(timeout) = timeout {
            config.chat.timeout = timeout;
        }
        config.agents = AgentConfig {
            state: directory.path().join(AGENTS),
            codex_api_url: provider.uri(),
            ..config.agents
        };
        let agents = config.agents.clone();

        let router = create_test_router(create_test_state(config, pool.clone()));
        let client = TestClient::new(router.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

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
        let suffix = Uuid::new_v4();
        let organization: Uuid = client
            .post_json_auth(
                "/api/organizations",
                &json!({"name": "Agent tools", "slug": format!("agent-{suffix}")}),
                &token,
            )
            .await
            .json_value()["organization"]["id"]
            .as_str()
            .expect("an organization")
            .parse()
            .expect("an organization id");
        let workspace = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({"name": "Agent tools", "slug": format!("agent-{suffix}")}),
                &token,
            )
            .await
            .json_value()["workspace"]["id"]
            .as_str()
            .expect("a workspace")
            .to_string();
        let created = client
            .post_json_auth(
                "/api/chats",
                &json!({
                    "workspace_id": workspace,
                    "title": "Agent tools",
                    "model_name": "sonnet",
                    "agent_enabled": agent_enabled,
                    "agent_sandboxed": sandboxed,
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
            pool,
            agents,
            organization,
            workspace,
            token,
            chat,
            address: address.to_string(),
            agent,
            _provider: provider,
            _directory: directory,
            server,
        }
    }

    /// Puts the chat in a new project whose source is the folder `directory`.
    async fn belong_to_project_in(&self, directory: &Path) {
        let workspace = self.workspace.parse().expect("a workspace id");
        let source = zone_server::db::sources::create_source(
            &self.pool,
            zone_server::db::sources::NewSource {
                workspace_id: workspace,
                name: &Uuid::new_v4().to_string(),
                source_type: "filesystem",
                config: json!({"base_path": directory}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("a source");
        let project = zone_server::db::projects::create(
            &self.pool,
            zone_server::db::projects::NewProject {
                name: "Project",
                description: None,
                workspace_id: Some(workspace),
                source_id: Some(source.id),
                status: None,
            },
        )
        .await
        .expect("a project");
        sqlx::query("UPDATE chats SET project_id = $1 WHERE id = $2")
            .bind(project.id)
            .bind(self.chat)
            .execute(&self.pool)
            .await
            .expect("the chat in the project");
    }

    /// Runs the organization's turns on Codex, signed in once with a home of its own, as the
    /// stand-in, which the instance names as its codex.
    async fn sign_in_to_codex(&self) {
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2) \
             ON CONFLICT (organization_id) DO UPDATE SET provider = EXCLUDED.provider",
        )
        .bind(self.organization)
        .bind(ai_settings::provider(AgentKind::Codex))
        .execute(&self.pool)
        .await
        .expect("the organization's AI settings");
        let login = agent_logins::insert(
            &self.pool,
            &agent_logins::Insert {
                organization_id: self.organization,
                agent: AgentKind::Codex.as_str(),
                account: Some("codex"),
                credential: None,
                label: Some("codex"),
                expires_at: None,
            },
        )
        .await
        .expect("a stored login")
        .id;
        let home = self
            .agents
            .create_login_home(self.organization, AgentKind::Codex, login)
            .expect("the login's home");
        std::fs::write(home.join(CODEX_CREDENTIALS_FILE), CODEX_CREDENTIALS)
            .expect("the codex login's credentials");
    }

    async fn socket(&self) -> Socket {
        let (mut socket, _) =
            connect_async(format!("ws://{}/ws/chats/{}", self.address, self.chat))
                .await
                .expect("the chat socket");
        send(&mut socket, json!({"type": "auth", "token": self.token})).await;
        socket
    }

    async fn turn(&self, content: &str) -> tokio::task::JoinHandle<Vec<Value>> {
        let mut socket = self.socket().await;
        let content = content.to_string();
        tokio::spawn(async move {
            send(&mut socket, json!({"type": "send", "content": content})).await;
            finish(&mut socket).await
        })
    }
}

/// A coding agent's turn runs where the chat's tools and its prompt say it does: in the folder
/// its project's source names.
#[tokio::test]
async fn a_project_chats_agent_runs_in_the_directory_its_prompt_names() {
    let harness = Harness::start(true, false).await;
    let project = TempDir::new().expect("a project folder");
    let physical = std::fs::canonicalize(project.path()).expect("the project's real path");
    harness.belong_to_project_in(project.path()).await;

    let turn = harness.turn("Where are you?").await;
    harness.agent.spawned().await;
    let ran_in = std::fs::read_to_string(&harness.agent.directory).expect("the agent's directory");
    let prompt = zone_core::llm::cli::prompt(
        &std::fs::read_to_string(&harness.agent.prompt).expect("the agent's prompt"),
    );
    harness.agent.finish();
    turn.await.expect("the turn ends");

    assert_eq!(ran_in.trim(), physical.to_str().expect("a UTF-8 path"));
    assert!(
        prompt.contains(&format!(
            "Working directory {} on",
            project.path().display()
        )),
        "the prompt names another directory than the one the agent runs in"
    );
}

/// A chat with no project runs its agent where it always has, which the instance's own agent
/// leaves to the server.
#[tokio::test]
async fn a_chat_with_no_project_runs_its_agent_where_it_always_did() {
    let harness = Harness::start(true, false).await;

    let turn = harness.turn("Where are you?").await;
    harness.agent.spawned().await;
    let ran_in = std::fs::read_to_string(&harness.agent.directory).expect("the agent's directory");
    harness.agent.finish();
    turn.await.expect("the turn ends");

    let server = std::fs::canonicalize(std::env::current_dir().expect("the server's directory"))
        .expect("the server's real path");
    assert_eq!(ran_in.trim(), server.to_str().expect("a UTF-8 path"));
}

/// The whole arrangement, in one turn: zone mints a lease, points the agent at
/// its own MCP endpoint, runs what the agent calls under the chat's approval
/// policy, shows the reader the call, and revokes the token when the turn ends.
#[tokio::test]
async fn a_cli_turn_serves_its_tools_for_the_life_of_the_turn_and_no_longer() {
    let harness = Harness::start(true, true).await;
    let written = harness._directory.path().join("written.txt");

    let turn = harness.turn("Write the file.").await;
    let (arguments, token) = harness.agent.spawned().await;

    let (endpoint, allowed) = served(&arguments);
    assert_eq!(
        endpoint,
        format!("http://{}/mcp", harness.address),
        "the agent must be pointed at this server: {arguments}"
    );
    assert!(!token.is_empty(), "the agent must be given a turn token");
    assert!(
        allowed.contains(&"mcp__zone__write_file".to_string()),
        "zone's tools must be allowlisted for the turn: {allowed:?}"
    );
    assert!(
        arguments.contains("--strict-mcp-config") && arguments.contains("--tools "),
        "a sandboxed chat must withhold the agent's own tools: {arguments}"
    );

    let (status, listed) = rpc(&endpoint, &token, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("a tool catalog")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"write_file"), "{names:?}");

    // Over MCP a question or a wait returns at once, parking nothing, and the
    // turn would read that as an answer nobody gave. So the agent is neither
    // served one nor told of one, and asks the reader in its reply instead.
    let prompt = zone_core::llm::cli::prompt(
        &std::fs::read_to_string(&harness.agent.prompt).expect("the agent's prompt"),
    );
    for tool in [ASK_USER, WAIT_FOR] {
        assert!(!prompt.contains(tool), "the prompt teaches {tool}");
        assert!(!names.contains(&tool), "{tool} was listed: {names:?}");
        assert!(
            !allowed.contains(&format!("mcp__zone__{tool}")),
            "{tool} was allowlisted: {allowed:?}"
        );
    }

    let (status, called) = rpc(
        &endpoint,
        &token,
        "tools/call",
        json!({
            "name": "write_file",
            "arguments": {
                "path": written.to_string_lossy(),
                "content": "written",
                "reason": "The turn asked for this file.",
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{called}");
    assert_eq!(
        called["result"]["isError"], false,
        "an auto-approved call must run: {called}"
    );
    assert_eq!(
        std::fs::read_to_string(&written).expect("the agent's write"),
        "written"
    );

    harness.agent.finish();
    let frames = turn.await.expect("the turn").clone();

    assert!(
        frames.iter().any(|frame| frame["type"] == "message_end"),
        "the turn must complete: {frames:?}"
    );
    assert!(
        frames.iter().any(|frame| {
            frame["type"] == "tool_result"
                && frame["name"] == "write_file"
                && frame["success"] == true
        }),
        "the reader must see what the agent did: {frames:?}"
    );

    let (status, refused) = rpc(&endpoint, &token, "tools/list", json!({})).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a token that outlives its turn is a standing grant on this workspace: {refused}"
    );
}

/// Stop ends the turn from the reader's side and the agent's: the console is
/// told, and neither the agent nor anything it forked keeps running.
async fn stop_ends_the_turn_and_its_processes(kind: AgentKind) {
    let harness = Harness::start_on(kind, true, true).await;
    let processes = harness.agent.forking();
    let mut socket = harness.socket().await;
    send(
        &mut socket,
        json!({"type": "send", "content": "Run for ever."}),
    )
    .await;
    let [leader, forked] = processes.pids().await;
    assert!(
        running(&leader),
        "the stand-in must be running to be stopped"
    );
    assert!(running(&forked), "its child must be running to be stopped");

    send(&mut socket, json!({"type": "cancel"})).await;
    let frames = tokio::time::timeout(STOP_TIMEOUT, finish(&mut socket))
        .await
        .expect("Stop must end the turn promptly");

    assert_eq!(
        frames.last().map(|frame| frame["type"].clone()),
        Some(json!("cancelled")),
        "the console must be told the turn stopped: {frames:?}"
    );
    assert!(gone(&leader).await, "the agent outlived Stop");
    assert!(gone(&forked).await, "the agent's child outlived Stop");
}

#[tokio::test]
async fn stopping_a_claude_turn_kills_its_agent_and_everything_it_forked() {
    stop_ends_the_turn_and_its_processes(AgentKind::Claude).await;
}

#[tokio::test]
async fn stopping_a_codex_turn_kills_its_agent_and_everything_it_forked() {
    stop_ends_the_turn_and_its_processes(AgentKind::Codex).await;
}

/// A zone tool the agent called is exactly when a reader reaches for Stop, and
/// the slowest are the ones that walk a directory tree. A call that keeps its
/// worker also keeps the turn, whose task the call has just woken on that
/// worker, however many others are idle: the reader is shown nothing and Stop
/// goes unread until the call ends. The call here reads a pipe the test holds,
/// so it is still running when Stop arrives on any host.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_reaches_a_cli_turn_while_a_tool_it_called_still_runs() {
    let held = Held::new();
    let harness = Harness::start(true, true).await;
    let processes = harness.agent.forking();
    let mut socket = harness.socket().await;
    send(
        &mut socket,
        json!({"type": "send", "content": "Read the pipe."}),
    )
    .await;
    let [leader, forked] = processes.pids().await;
    let (arguments, token) = harness.agent.spawned().await;
    let (endpoint, _) = served(&arguments);

    let pipe = held.path.to_string_lossy().into_owned();
    let mut call = tokio::spawn(async move {
        rpc(
            &endpoint,
            &token,
            "tools/call",
            json!({
                "name": "run_command",
                "arguments": {"command": "cat", "args": [pipe], "reason": "Read the pipe."},
            }),
        )
        .await
    });
    let started = tokio::time::timeout(SPAWN_TIMEOUT, async {
        loop {
            let frame = next(&mut socket).await;
            if frame["type"] == "tool_call" && frame["name"] == "run_command" {
                return frame;
            }
        }
    })
    .await
    .expect("the reader must be shown the call while it runs");
    let Some(release) = held.reached().await else {
        let answered = match call.is_finished() {
            true => format!("{:?}", (&mut call).await),
            false => "nothing".to_string(),
        };
        panic!("the call never read the held pipe, so nothing kept it running: {answered}");
    };
    assert!(
        !call.is_finished(),
        "the call finished before the reader was shown it, so its walk held the runtime: \
         {started}"
    );

    send(&mut socket, json!({"type": "cancel"})).await;
    let frames = tokio::time::timeout(STOP_TIMEOUT, finish(&mut socket))
        .await
        .expect("Stop must end the turn promptly");

    assert_eq!(
        frames.last().map(|frame| frame["type"].clone()),
        Some(json!("cancelled")),
        "the console must be told the turn stopped: {frames:?}"
    );
    assert!(
        !call.is_finished(),
        "Stop was only read once the call had finished"
    );
    assert!(gone(&leader).await, "the agent outlived Stop");
    assert!(gone(&forked).await, "the agent's child outlived Stop");
    drop(release);
    let _ = tokio::time::timeout(STOP_TIMEOUT, call).await;
}

/// A chat whose agent the reader turned off gets no tools, and neither does
/// the CLI backend answering it: the switch withholds zone's registry from the
/// child exactly as it withholds it from the model.
#[tokio::test]
async fn a_chat_with_its_agent_turned_off_serves_nothing() {
    let harness = Harness::start(false, true).await;

    let turn = harness.turn("Say hello.").await;
    let (arguments, token) = harness.agent.spawned().await;

    assert!(
        !arguments.contains("--mcp-config"),
        "a chat with no agent must not be served zone's tools: {arguments}"
    );
    assert!(token.is_empty(), "no turn token belongs in that child");

    harness.agent.finish();
    let frames = turn.await.expect("the turn");
    assert!(
        frames.iter().any(|frame| frame["type"] == "message_end"),
        "the turn still answers: {frames:?}"
    );
}

/// The other half of `agent_sandboxed`: an unsandboxed chat leaves the agent
/// the file and shell tools it ships with, beside zone's.
#[tokio::test]
async fn an_unsandboxed_chat_leaves_the_agent_its_own_tools() {
    let harness = Harness::start(true, false).await;

    let turn = harness.turn("Write the file.").await;
    let (arguments, _) = harness.agent.spawned().await;

    let (endpoint, allowed) = served(&arguments);
    assert_eq!(endpoint, format!("http://{}/mcp", harness.address));
    assert!(
        allowed.contains(&"mcp__zone__write_file".to_string()),
        "{allowed:?}"
    );
    assert!(
        !arguments.contains("--tools "),
        "an unsandboxed chat must not withhold the agent's own tools: {arguments}"
    );

    harness.agent.finish();
    let frames = turn.await.expect("the turn");
    assert!(
        frames.iter().any(|frame| frame["type"] == "message_end"),
        "{frames:?}"
    );
}

fn system_prompt(preparation: &session::Preparation) -> String {
    preparation.context.entries[0]
        .message
        .content
        .clone()
        .expect("the turn's system prompt")
}

/// A chat turn on a coding agent gets the chat's whole time budget on the
/// agent's own clock too, which the operator may set past the thirty minutes
/// an agent's turn has by default. And it is prepared with no tool that would
/// park zone's own loop, in its catalog or in its prompt, where a turn on an
/// endpoint keeps both.
#[tokio::test]
async fn a_cli_turn_is_prepared_for_the_chats_budget_with_nothing_that_parks() {
    let harness = common::context::Harness::new(None, true, Vec::new()).await;
    let mut config = harness.config.clone();
    config.chat.timeout = CHAT_TIMEOUT;
    let state = create_test_state(config, harness.pool.clone());
    let chat = chats::get_chat(&harness.pool, harness.chat)
        .await
        .unwrap()
        .expect("the harness's chat");
    let route = Route::for_workspace(&state, chat.workspace_id.unwrap()).await;

    let agent = session::build(
        &state,
        &chat,
        Uuid::new_v4(),
        None,
        Mode::Generation(session::Generation::unrouted(LlmBackend::cli(
            AgentKind::Claude,
            CliSettings::default(),
        ))),
        route.endpoint().expect("a usable route").clone(),
        None,
    )
    .await
    .expect("a turn on an agent is prepared");
    let endpoint = session::build(
        &state,
        &chat,
        Uuid::new_v4(),
        None,
        Mode::Generation(session::Generation::unrouted(LlmBackend::Http)),
        route.into_endpoint().expect("a usable route"),
        None,
    )
    .await
    .expect("a turn on the endpoint is prepared");

    for tool in [ASK_USER, WAIT_FOR] {
        assert!(
            system_prompt(&endpoint).contains(tool),
            "a turn on the endpoint is no longer taught {tool}"
        );
        assert!(
            endpoint.tools.has(tool),
            "a turn on the endpoint lost {tool}"
        );
        assert!(
            !system_prompt(&agent).contains(tool),
            "a turn on an agent is taught {tool}"
        );
        assert!(!agent.tools.has(tool), "a turn on an agent holds {tool}");
    }
    let LlmBackend::Cli { settings, .. } = agent.llm.backend() else {
        panic!("the turn was moved off its agent");
    };
    assert!(
        settings.timeout < CHAT_TIMEOUT
            && settings.timeout >= CHAT_TIMEOUT - Duration::from_secs(60),
        "the agent's clock {:?} must stop it shortly before the chat's {CHAT_TIMEOUT:?}",
        settings.timeout
    );
}

const CREATE_PULL_REQUEST: &str = "create_pull_request";

/// The chat's system prompt as `backend` would be handed it.
async fn prompted_on(
    state: &zone_server::state::AppState,
    chat: &chats::ChatRow,
    backend: LlmBackend,
) -> String {
    let route = Route::for_workspace(state, chat.workspace_id.expect("a workspace")).await;
    let preparation = session::build(
        state,
        chat,
        Uuid::new_v4(),
        None,
        Mode::Generation(session::Generation::unrouted(backend)),
        route.into_endpoint().expect("a usable route"),
        None,
    )
    .await
    .expect("the turn is prepared");
    assert!(
        preparation.tools.has(CREATE_PULL_REQUEST),
        "the turn lost zone's pull request tool"
    );
    system_prompt(&preparation)
}

async fn add_github_source(pool: &PgPool, workspace: Uuid) -> Uuid {
    zone_server::db::sources::create_source(
        pool,
        zone_server::db::sources::NewSource {
            workspace_id: workspace,
            name: &Uuid::new_v4().to_string(),
            source_type: zone_server::db::sources::GITHUB,
            config: json!({"owner": "abnegate", "repo": "zone"}),
            description: None,
            url: None,
            credentials_encrypted: None,
            github_installation_id: None,
        },
    )
    .await
    .expect("a GitHub source")
    .id
}

/// Zone records the pull requests its own tool opens, and only guesses at one
/// a shell opened. So in a workspace with a GitHub source every turn is sent to
/// that tool, by the name its model calls it, and away from `gh pr create`;
/// without one, the tool could only fail and nothing is said about it.
#[tokio::test]
async fn every_turn_in_a_workspace_on_github_opens_pull_requests_through_zone() {
    let harness = common::context::Harness::new(None, true, Vec::new()).await;
    let state = create_test_state(harness.config.clone(), harness.pool.clone());
    let chat = chats::get_chat(&harness.pool, harness.chat)
        .await
        .unwrap()
        .expect("the harness's chat");
    let named = || {
        [
            (LlmBackend::Http, CREATE_PULL_REQUEST.to_string()),
            (
                LlmBackend::cli(AgentKind::Claude, CliSettings::default()),
                AgentKind::Claude.tool(Toolset::SERVER, CREATE_PULL_REQUEST),
            ),
            (
                LlmBackend::cli(AgentKind::Codex, CliSettings::default()),
                AgentKind::Codex.tool(Toolset::SERVER, CREATE_PULL_REQUEST),
            ),
        ]
    };

    let disabled = add_github_source(&harness.pool, harness.workspace).await;
    sqlx::query("UPDATE sources SET is_active = FALSE WHERE id = $1")
        .bind(disabled)
        .execute(&harness.pool)
        .await
        .expect("the source disabled");

    for (backend, _) in named() {
        let prompt = prompted_on(&state, &chat, backend).await;
        assert!(
            !prompt.contains("so Zone can track it"),
            "a disabled source is no source: {prompt}"
        );
        assert!(!prompt.contains("gh pr create"), "{prompt}");
    }

    add_github_source(&harness.pool, harness.workspace).await;

    for (backend, name) in named() {
        let prompt = prompted_on(&state, &chat, backend).await;
        assert!(
            prompt.contains(&format!("then calling {name} so Zone can track it")),
            "{name}: {prompt}"
        );
        assert!(
            prompt.contains("or use gh pr create, hub, the GitHub web UI or another server's tool"),
            "{name}: {prompt}"
        );
    }
}

/// The instruction is only worth giving where the tool is: a coding agent with
/// a GitHub source is served zone's pull request tool over MCP whether or not
/// it keeps its own tools, and the prompt on its stdin names it as it calls it.
#[tokio::test]
async fn a_coding_agent_on_github_is_served_zones_pull_request_tool_and_told_to_use_it() {
    for sandboxed in [true, false] {
        let harness = Harness::start(true, sandboxed).await;
        add_github_source(
            &harness.pool,
            harness.workspace.parse().expect("a workspace id"),
        )
        .await;

        let turn = harness.turn("Open a pull request for this branch.").await;
        let (arguments, token) = harness.agent.spawned().await;
        let (endpoint, allowed) = served(&arguments);
        let qualified = AgentKind::Claude.tool(Toolset::SERVER, CREATE_PULL_REQUEST);

        assert!(allowed.contains(&qualified), "{allowed:?}");
        let (status, listed) = rpc(&endpoint, &token, "tools/list", json!({})).await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .expect("a tool catalog")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(
            names.contains(&CREATE_PULL_REQUEST),
            "sandboxed {sandboxed}: {names:?}"
        );
        assert!(
            names.contains(&"comment_on_issue"),
            "sandboxed {sandboxed}: {names:?}"
        );

        let prompt = zone_core::llm::cli::prompt(
            &std::fs::read_to_string(&harness.agent.prompt).expect("the agent's prompt"),
        );
        assert!(
            prompt.contains(&format!("then calling {qualified} so Zone can track it")),
            "sandboxed {sandboxed}: {prompt}"
        );
        assert!(prompt.contains("or use gh pr create"), "{prompt}");

        harness.agent.finish();
        turn.await.expect("the turn ends");
    }
}

/// A finished agent turn keeps the window its agent reported for the chat's next turn, and the
/// turn's last context frame already shows it.
#[tokio::test]
async fn a_cli_turn_keeps_the_window_its_agent_reported_and_shows_it_as_it_ends() {
    let harness = Harness::start(false, true).await;
    harness.agent.reporting();

    let frames = harness
        .turn("How large is your window?")
        .await
        .await
        .expect("the turn ends");

    let usage = &frames
        .iter()
        .rev()
        .find(|frame| frame["type"] == "context")
        .unwrap_or_else(|| panic!("no context frame in {frames:?}"))["usage"];
    assert_eq!(usage["limit"], json!(200_000), "{usage}");
    assert_eq!(usage["source"], json!("runtime"), "{usage}");
    assert!(
        usage["used"].as_u64().expect("a used count") >= 3 + 120 + 16_500 + 9,
        "{usage}"
    );

    let kept = occupancy::get(&harness.pool, harness.chat)
        .await
        .expect("the chat's report is readable")
        .expect("the turn kept its agent's report");
    assert_eq!(kept.agent, AgentKind::Claude);
    assert_eq!(kept.model, "sonnet");
    assert_eq!(
        kept.occupancy,
        Occupancy::reported(200_000, Some(3 + 120 + 16_500 + 9)).expect("a reported window")
    );
}

/// Codex reports its window only once it has exited, after the line that finished its turn, so
/// a chat that stopped reading at the finish would keep nothing. The chat reads the turn to its
/// end, and the window is kept, and shown, all the same.
#[tokio::test]
async fn the_chat_reads_the_agent_stream_to_its_end() {
    let harness = Harness::start_on(AgentKind::Codex, false, true).await;
    harness.sign_in_to_codex().await;
    harness.agent.reporting_once_it_exits();

    let frames = harness
        .turn("How large is your window?")
        .await
        .await
        .expect("the turn ends");

    successful(&frames);
    let kept = occupancy::get(&harness.pool, harness.chat)
        .await
        .expect("the chat's report is readable")
        .expect("the turn kept the window codex reported once it exited");
    assert_eq!(kept.agent, AgentKind::Codex);
    assert_eq!(
        kept.occupancy,
        Occupancy::reported(CODEX_WINDOW, Some(CODEX_FILLED)).expect("a reported window")
    );
    let usage = &frames
        .iter()
        .rev()
        .find(|frame| frame["type"] == "context")
        .unwrap_or_else(|| panic!("no context frame in {frames:?}"))["usage"];
    assert_eq!(usage["limit"], json!(CODEX_WINDOW), "{usage}");
}

/// An agent stopped before it finished keeps the answer it gave, and the reader is told it was
/// cut short: the turn is neither an error nor a finished answer.
#[tokio::test]
async fn an_answer_the_agent_was_cut_short_after_is_kept_and_marked_cut_short() {
    let harness = Harness::start(false, true).await;
    harness.agent.exiting_mid_turn();

    let frames = harness
        .turn("Write it up.")
        .await
        .await
        .expect("the turn ends");

    successful(&frames);
    let ended = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .expect("a message_end");
    let shown = ended["content"].as_str().expect("the answer's content");
    assert_eq!(
        shown,
        format!(
            "{ANSWER}\n\n{UNFINISHED}\n\n[Cut short: the agent was stopped before it finished]"
        ),
        "{frames:?}"
    );
    let stored: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(harness.chat)
    .fetch_one(&harness.pool)
    .await
    .expect("the stored answer");
    assert_eq!(stored, shown, "a reload reads the answer differently");
}

/// A claude whose background work outlasts the chat's time still keeps the answer it gave: its
/// own clock stops it before the chat's does, so the answer ends cut short rather than lost to
/// the chat timing out.
#[tokio::test]
async fn an_answer_given_before_background_work_outlasted_the_turn_is_kept_cut_short() {
    let harness =
        Harness::start_within(AgentKind::Claude, false, true, Some(SHORT_CHAT_TIMEOUT)).await;
    harness.agent.answering_over_endless_background_work();

    let frames = harness
        .turn("Start the server and tell me when it is up.")
        .await
        .await
        .expect("the turn ends");

    successful(&frames);
    let ended = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .expect("a message_end");
    assert_eq!(
        ended["content"],
        format!("{ANSWER}\n\n[Cut short: the agent was stopped before it finished]"),
        "{frames:?}"
    );
}
