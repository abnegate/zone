//! A CLI-backed chat turn, and the tools it serves the agent it spawns.
//!
//! The pieces are covered on their own elsewhere: `mcp::Turn` decides a call,
//! `CliSettings` carries a toolset to a child process, and the chat routes
//! carry `agent_sandboxed`. What is only true end to end is that a turn mints
//! a lease at all, points the agent at this server, keeps the token alive for
//! exactly as long as the agent may call with it, and shows the reader what
//! the agent did. This drives that through the real websocket, against a
//! stand-in for the host's `claude` that calls back the way the real one does.

mod common;

use common::context::{finish, send};
use common::{
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
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::{AgentKind, CliSettings, LlmBackend};
use zone_server::agent::ASK_USER;
use zone_server::agent::wait::WAIT_FOR;
use zone_server::config::ModelBackend;
use zone_server::db::chats;
use zone_server::services::chat::session::{self, Mode};

/// Longer than the thirty minutes a coding agent's turn gets by default.
const CHAT_TIMEOUT: Duration = Duration::from_secs(3600);

/// How long the test waits for the spawned agent to report how it was invoked.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

const ANSWER: &str = "Written.";

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
        };
        let mut file = std::fs::File::create(&agent.executable).expect("the stand-in agent");
        writeln!(
            file,
            "#!/bin/sh\ncat > {prompt}\nprintf '%s' \"$ZONE_MCP_TOKEN\" > {token}\nprintf \
             '%s' \"$*\" > {arguments}\nwhile [ ! -f {release} ]; do sleep 0.05; \
             done\necho '{assistant}'\necho '{result}'",
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
}

/// The MCP server definition zone passed on the command line, as the agent
/// would have read it: where to reach zone, and which tools it may call.
fn served(arguments: &str) -> (String, Vec<String>) {
    let endpoint = between(arguments, r#""url":""#, '"').expect("zone named its MCP endpoint");
    let allowed = match arguments.split("--allowedTools ").nth(1) {
        Some(rest) => rest
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .split(',')
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    };
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
            agent: AgentKind::Claude,
            executable: Some(agent.executable.clone()),
        };

        let router = create_test_router(create_test_state(config, pool));
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
        let organization = client
            .post_json_auth(
                "/api/organizations",
                &json!({"name": "Agent tools", "slug": format!("agent-{suffix}")}),
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
            token,
            chat,
            address: address.to_string(),
            agent,
            _provider: provider,
            _directory: directory,
            server,
        }
    }

    async fn turn(&self, content: &str) -> tokio::task::JoinHandle<Vec<Value>> {
        let (mut socket, _) =
            connect_async(format!("ws://{}/ws/chats/{}", self.address, self.chat))
                .await
                .expect("the chat socket");
        send(&mut socket, json!({"type": "auth", "token": self.token})).await;
        let content = content.to_string();
        tokio::spawn(async move {
            send(&mut socket, json!({"type": "send", "content": content})).await;
            finish(&mut socket).await
        })
    }
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
    let prompt = std::fs::read_to_string(&harness.agent.prompt).expect("the agent's prompt");
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

    let agent = session::build(
        &state,
        &chat,
        Uuid::new_v4(),
        None,
        Mode::Generation(LlmBackend::cli(AgentKind::Claude, CliSettings::default())),
    )
    .await
    .expect("a turn on an agent is prepared");
    let endpoint = session::build(
        &state,
        &chat,
        Uuid::new_v4(),
        None,
        Mode::Generation(LlmBackend::Http),
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
    let LlmBackend::Cli { settings, .. } = &agent.llm.config().backend else {
        panic!("the turn was moved off its agent");
    };
    assert_eq!(settings.timeout, CHAT_TIMEOUT);
}
