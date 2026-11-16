//! An organization's tool-tier ceiling and network allowlist, held on every chat turn and task
//! run from the route's one loaded policy. Auto-approve releases nothing above the ceiling; a run
//! nobody attends refuses such a call outright; a coding agent the ceiling can't reach runs
//! without its own tools; a restricted network takes the turn offline, readmitting only what it
//! can hold to its domains. Every call the policy refuses is audited, and none of it applies on
//! Community.

use std::path::{Path, PathBuf};
use std::time::Duration;

use abnegate_agent_cli::AgentKind;
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_enterprise::Tier;
use zone_enterprise::policy::Denial;
use zone_server::agent::approval::refusal::POLICY;
use zone_server::config::{AgentConfig, ModelBackend};
use zone_server::db::audit::{actions, resources};
use zone_server::db::{licenses, migrations, tasks};
use zone_server::state::AppState;

use crate::common::context::{MODEL, Script, Socket, answer, calls, next, send, tool};
use crate::common::sso::{Audit, active, keyring, lapsed};
use crate::common::{
    Database, TestClient, create_test_router, create_test_state, test_config, test_email,
    test_password,
};

const PREFIX: &str = "policy_tools";
const AGENTS: &str = "agents";
const STAND_IN: &str = "agent";
const ARGUMENTS: &str = "arguments";
const AGENT_MODEL: &str = "sonnet";
const ANSWER: &str = "Done.";
const NEWS: &str = "What is the latest news on Rust?";
const SEARCHING: &str = "Searching the web...";
const SEARCH_PATH: &str = "/search";
const COMPLETIONS_PATH: &str = "/chat/completions";

const APPROVAL_REQUIRED: &str = "tool_approval_required";
const STATUS: &str = "status";
const MESSAGE_END: &str = "message_end";

const RUN_COMMAND: &str = "run_command";
const WRITE_FILE: &str = "write_file";
const CREATE_REMINDER: &str = "create_reminder";
const FETCH_URL: &str = "fetch_url";
const WEB_SEARCH: &str = "web_search";
const CREATE_PULL_REQUEST: &str = "create_pull_request";

const ADMITTED: &str = "127.0.0.1";
const FOREIGN: &str = "example.com";

/// Claude withholds its own tools with an empty `--tools`.
const CLAUDE_WITHHELD: &str = "--tools ";
/// Codex withholds its own tools by disabling the features that bring them.
const CODEX_WITHHELD: &str = "shell_tool";

const AGENT_TIMEOUT: Duration = Duration::from_secs(30);
const AUDIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(25);

/// Where a turn goes: Zone's own loop over HTTP, or a coding agent the instance runs, stood in
/// for by a script that records how it was started.
#[derive(Clone, Copy)]
enum Backend {
    Http,
    Cli(AgentKind),
}

/// A server with a database of its own, since the license is server-wide, serving the chat
/// socket on a port of its own, with one organization and workspace owned by the server admin.
struct Server {
    database: Database,
    state: AppState,
    client: TestClient,
    provider: MockServer,
    script: Script,
    token: String,
    organization: Uuid,
    workspace: Uuid,
    address: String,
    directory: TempDir,
    server: JoinHandle<()>,
}

impl Server {
    async fn start(backend: Backend) -> Self {
        let provider = MockServer::start().await;
        let script = Script::new(Vec::new());
        mock(&provider, &script).await;
        let directory = TempDir::new().expect("a temporary directory");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener.local_addr().expect("the bound address");

        let mut config = test_config();
        config.host = address.ip().to_string();
        config.port = address.port();
        config.litellm_host = provider.uri();
        config.ollama_host = provider.uri();
        config.comfyui.enabled = false;
        config.web_search.enabled = true;
        config.web_search.query_url = format!("{}{SEARCH_PATH}?q=<query>", provider.uri());
        if let Backend::Cli(kind) = backend {
            config.model_backend = ModelBackend::Cli {
                agent: kind,
                executable: Some(stand_in(directory.path(), kind)),
            };
            config.agents = AgentConfig {
                state: directory.path().join(AGENTS),
                codex_api_url: provider.uri(),
                ..config.agents
            };
        }

        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let state = create_test_state(config, database.pool.clone());
        state.install_keyring(keyring());
        let router = create_test_router(state.clone());
        let client = TestClient::new(router.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let registered = client
            .post_json(
                "/api/auth/register",
                &json!({"email": test_email(), "password": test_password()}),
            )
            .await
            .json_value();
        let token = registered["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("registration returns an access token: {registered}"))
            .to_string();
        let organization = client
            .post_json_auth(
                "/api/organizations",
                &json!({"name": "Policy tools", "slug": Uuid::new_v4().to_string()}),
                &token,
            )
            .await
            .json_value();
        let organization = identifier(&organization["organization"]["id"]);
        let workspace = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({"name": "Policy tools", "slug": Uuid::new_v4().to_string()}),
                &token,
            )
            .await
            .json_value();
        let workspace = identifier(&workspace["workspace"]["id"]);

        Self {
            database,
            state,
            client,
            provider,
            script,
            token,
            organization,
            workspace,
            address: address.to_string(),
            directory,
            server,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    /// A chat in the workspace that auto-approves, with `overrides` on top.
    async fn chat(&self, model: &str, overrides: Value) -> Uuid {
        let mut body = json!({
            "workspace_id": self.workspace,
            "title": "Policy tools",
            "model_name": model,
            "agent_enabled": true,
            "agent_sandboxed": true,
            "auto_approve": true,
            "automatic_title": false,
        });
        for (key, value) in overrides.as_object().expect("overrides are an object") {
            body[key] = value.clone();
        }
        let created = self
            .client
            .post_json_auth("/api/chats", &body, &self.token)
            .await
            .json_value();
        assert!(created["chat"]["id"].is_string(), "the chat: {created}");
        identifier(&created["chat"]["id"])
    }

    async fn license(&self, key: &str) {
        licenses::replace(self.pool(), key).await.unwrap();
        self.state.licenses().load(self.pool()).await.unwrap();
    }

    async fn restrict(&self, rules: Value) {
        sqlx::query("INSERT INTO organization_policies (organization_id, rules) VALUES ($1, $2)")
            .bind(self.organization)
            .bind(rules)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn socket(&self, chat: Uuid) -> Socket {
        let (mut socket, _) = connect_async(format!("ws://{}/ws/chats/{chat}", self.address))
            .await
            .expect("the chat socket");
        send(&mut socket, json!({"type": "auth", "token": self.token})).await;
        let initial = next(&mut socket).await;
        assert_eq!(initial["type"], "init", "initial frame: {initial}");
        socket
    }

    /// Runs one turn of `chat`, refusing every call it is asked to approve, and returns every
    /// frame the turn sent.
    async fn turn(&self, chat: Uuid, content: &str, metadata: Option<Value>) -> Vec<Value> {
        let mut socket = self.socket(chat).await;
        send(
            &mut socket,
            json!({"type": "send", "content": content, "metadata": metadata}),
        )
        .await;
        let mut frames = Vec::new();
        loop {
            let frame = next(&mut socket).await;
            if frame["type"] == APPROVAL_REQUIRED {
                send(
                    &mut socket,
                    json!({"type": "approve_tool", "tool_call_id": frame["tool_call_id"], "approved": false}),
                )
                .await;
            }
            let terminal = matches!(
                frame["type"].as_str(),
                Some("message_end" | "error" | "cancelled")
            );
            frames.push(frame);
            if terminal {
                let _ = socket.close(None).await;
                assert!(
                    frames.iter().any(|frame| frame["type"] == MESSAGE_END),
                    "the turn must complete: {frames:?}"
                );
                return frames;
            }
        }
    }

    /// Every completion request the model was sent, in order.
    async fn completions(&self) -> Vec<Value> {
        self.received(COMPLETIONS_PATH)
            .await
            .into_iter()
            .map(|request| serde_json::from_slice(&request.body).expect("a JSON completion"))
            .collect()
    }

    async fn searches(&self) -> usize {
        self.received(SEARCH_PATH).await.len()
    }

    async fn received(&self, wanted: &str) -> Vec<Request> {
        self.provider
            .received_requests()
            .await
            .expect("the provider records its requests")
            .into_iter()
            .filter(|request| request.url.path() == wanted)
            .collect()
    }

    /// The arguments the stand-in agent was last started with.
    async fn arguments(&self) -> String {
        let recorded = self.directory.path().join(ARGUMENTS);
        let deadline = Instant::now() + AGENT_TIMEOUT;
        loop {
            if let Ok(arguments) = std::fs::read_to_string(&recorded) {
                std::fs::remove_file(&recorded).expect("the recorded arguments to be cleared");
                return arguments;
            }
            assert!(
                Instant::now() < deadline,
                "the CLI backend never started its agent"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    async fn cleanup(self) {
        self.server.abort();
        self.database.cleanup().await;
    }
}

async fn mock(provider: &MockServer, script: &Script) {
    let responder = script.clone();
    Mock::given(method("POST"))
        .and(path(COMPLETIONS_PATH))
        .respond_with(move |request: &Request| responder.respond(request))
        .mount(provider)
        .await;
    let uri = provider.uri();
    Mock::given(method("GET"))
        .and(path("/v2/model/info"))
        .respond_with(move |_: &Request| {
            ResponseTemplate::new(200).set_body_json(json!({"data": [{
                "model_name": MODEL,
                "litellm_params": {"model": format!("ollama_chat/{MODEL}"), "api_base": uri, "num_ctx": 32_768},
                "model_info": {"id": "policy-tools-route"},
            }]}))
        })
        .mount(provider)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/ps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
        .mount(provider)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "capabilities": ["completion", "tools"],
            "model_info": {"general.architecture": "test", "test.context_length": 32_768},
        })))
        .mount(provider)
        .await;
    Mock::given(method("GET"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .mount(provider)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"rate_limit": {"primary_window": {"used_percent": 10.0}}})),
        )
        .mount(provider)
        .await;
}

/// A coding agent of `kind` that records the arguments it was started with and answers at once.
fn stand_in(directory: &Path, kind: AgentKind) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let lines = match kind {
        AgentKind::Codex => vec![
            json!({"type": "thread.started", "thread_id": Uuid::new_v4().to_string()}),
            json!({"type": "turn.started"}),
            json!({"type": "item.completed", "item": {"id": "item_0", "type": "agent_message", "text": ANSWER}}),
            json!({"type": "turn.completed", "usage": {"input_tokens": 1, "cached_input_tokens": 0, "output_tokens": 1}}),
        ],
        _ => vec![
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": ANSWER}]}}),
            json!({"type": "result", "subtype": "success", "is_error": false}),
        ],
    };
    let echoed = lines
        .iter()
        .map(|line| format!("echo '{line}'\n"))
        .collect::<String>();
    let arguments = directory.join(ARGUMENTS);
    let partial = directory.join(format!("{ARGUMENTS}.partial"));
    let executable = directory.join(STAND_IN);
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '%s' \"$*\" > {partial}\nmv {partial} {arguments}\n{echoed}",
            partial = partial.display(),
            arguments = arguments.display(),
        ),
    )
    .expect("the stand-in agent");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in agent to be executable");
    executable
}

fn identifier(value: &Value) -> Uuid {
    value
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("an id: {value}"))
}

/// The names of the calls the turn stopped to ask about.
fn asked(frames: &[Value]) -> Vec<&str> {
    frames
        .iter()
        .filter(|frame| frame["type"] == APPROVAL_REQUIRED)
        .filter_map(|frame| frame["name"].as_str())
        .collect()
}

/// What a completion request tells the model it may call: the tool schemas, and the system
/// prompt that names every deferred tool.
fn advertised(request: &Value) -> String {
    let system = request["messages"]
        .as_array()
        .expect("a completion carries messages")
        .iter()
        .filter(|message| message["role"] == "system")
        .filter_map(|message| message["content"].as_str())
        .collect::<String>();
    format!("{}\n{system}", request["tools"])
}

/// Whether `text` names the tool `name` itself, rather than something that only begins with it.
fn names(text: &str, name: &str) -> bool {
    let word = |character: Option<char>| {
        character.is_some_and(|character| character.is_alphanumeric() || character == '_')
    };
    text.match_indices(name).any(|(start, _)| {
        !word(text[..start].chars().next_back()) && !word(text[start + name.len()..].chars().next())
    })
}

/// What the model was told the call `id` produced.
fn result(request: &Value, id: &str) -> String {
    request["messages"]
        .as_array()
        .expect("a completion carries messages")
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .and_then(|message| message["content"].as_str())
        .unwrap_or_else(|| panic!("no result for {id} in {request}"))
        .to_string()
}

/// The tool denials audited so far, waiting for at least one: each is written in a task of its
/// own after the call is refused.
async fn denials(pool: &PgPool) -> Vec<Audit> {
    let deadline = Instant::now() + AUDIT_TIMEOUT;
    loop {
        let audits: Vec<Audit> = sqlx::query_as(
            "SELECT actor_id, organization_id, resource_type, resource_id, old_values, new_values
             FROM audit_logs WHERE action = $1 ORDER BY created_at",
        )
        .bind(actions::AGENT_POLICY_TOOL_DENIED)
        .fetch_all(pool)
        .await
        .unwrap();
        if !audits.is_empty() {
            return audits;
        }
        assert!(Instant::now() < deadline, "no tool denial was audited");
        tokio::time::sleep(POLL).await;
    }
}

fn command(id: &str) -> Value {
    tool(id, RUN_COMMAND, json!({"command": "echo held"}))
}

#[tokio::test]
async fn auto_approve_asks_for_a_call_above_the_ceiling() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"ceiling": "write"})).await;

    let community = server.chat(MODEL, json!({})).await;
    server.script.push(calls("", vec![command("community")]));
    server.script.push(answer(ANSWER));
    let frames = server.turn(community, "Run it.", None).await;
    assert!(
        asked(&frames).is_empty(),
        "Community holds auto-approve to no ceiling: {frames:?}"
    );

    server.license(&active(Tier::Team)).await;
    let enforced = server.chat(MODEL, json!({})).await;
    server.script.push(calls("", vec![command("enforced")]));
    server.script.push(answer(ANSWER));
    let frames = server.turn(enforced, "Run it.", None).await;
    assert_eq!(
        asked(&frames),
        [RUN_COMMAND],
        "a command is above a write ceiling, so auto-approve asks: {frames:?}"
    );

    server.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_still_holds_the_ceiling() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"ceiling": "write"})).await;
    server.license(&lapsed(Tier::Enterprise)).await;

    let chat = server.chat(MODEL, json!({})).await;
    server.script.push(calls("", vec![command("lapsed")]));
    server.script.push(answer(ANSWER));
    let frames = server.turn(chat, "Run it.", None).await;
    assert_eq!(asked(&frames), [RUN_COMMAND], "{frames:?}");

    server.cleanup().await;
}

#[tokio::test]
async fn a_host_ceiling_releases_a_write_and_asks_for_an_outward_call() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"ceiling": "host"})).await;
    server.license(&active(Tier::Team)).await;
    let note = server.directory.path().join("note.txt");

    let chat = server.chat(MODEL, json!({})).await;
    server.script.push(calls(
        "",
        vec![tool(
            "write",
            WRITE_FILE,
            json!({"path": note, "content": "released\n"}),
        )],
    ));
    server.script.push(calls(
        "",
        vec![tool(
            "remind",
            CREATE_REMINDER,
            json!({"message": "Check the build", "minutes": 5}),
        )],
    ));
    server.script.push(answer(ANSWER));
    let frames = server.turn(chat, "Write it, then remind me.", None).await;

    assert_eq!(
        asked(&frames),
        [CREATE_REMINDER],
        "a write is within a host ceiling and an outward call is above it: {frames:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("the released write"),
        "released\n"
    );

    server.cleanup().await;
}

#[tokio::test]
async fn a_writerless_task_run_refuses_a_call_above_the_ceiling_and_audits_it() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"ceiling": "write"})).await;
    server.license(&active(Tier::Team)).await;

    let task = tasks::create_task_as(
        server.pool(),
        server.workspace,
        &[],
        "Held to the ceiling",
        "Run the command",
        None,
        None,
        true,
        None,
        None,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1")
        .bind(task.id)
        .bind(MODEL)
        .execute(server.pool())
        .await
        .unwrap();
    let run = tasks::create_task_run_as(server.pool(), task.id, None)
        .await
        .unwrap();
    server.script.push(calls("", vec![command("held")]));
    server.script.push(answer(ANSWER));

    zone_server::workers::task::execute_task_run(&server.state, run.id, task.id).await;

    let completions = server.completions().await;
    assert!(
        completions.len() >= 2,
        "the refusal goes back to the model: {completions:?}"
    );
    let refused = result(&completions[1], "held");
    assert!(
        refused.contains(POLICY),
        "nobody attends a run to approve a command above the ceiling: {refused}"
    );
    let audits = denials(server.pool()).await;
    let [audit] = audits.as_slice() else {
        panic!("one refusal, one audit: {audits:?}");
    };
    assert_eq!(audit.resource_type, resources::TASK);
    assert_eq!(audit.resource_id, Some(task.id));
    assert_eq!(audit.organization_id, Some(server.organization));
    assert_eq!(audit.actor_id, None, "a writerless run has no actor");
    let values = audit.new_values.as_ref().expect("the denial's values");
    assert_eq!(values["code"], Denial::Approval.code());
    assert_eq!(values["detail"], RUN_COMMAND);

    server.cleanup().await;
}

#[tokio::test]
async fn a_codex_chat_under_any_ceiling_runs_without_its_own_tools() {
    let server = Server::start(Backend::Cli(AgentKind::Codex)).await;
    server.restrict(json!({"ceiling": "host"})).await;
    let unsandboxed = json!({"agent_sandboxed": false});

    let community = server.chat(AGENT_MODEL, unsandboxed.clone()).await;
    server.turn(community, "Write it.", None).await;
    let arguments = server.arguments().await;
    assert!(
        !arguments.contains(CODEX_WITHHELD),
        "an unsandboxed Codex keeps its own tools with no policy: {arguments}"
    );

    server.license(&active(Tier::Team)).await;
    let enforced = server.chat(AGENT_MODEL, unsandboxed).await;
    server.turn(enforced, "Write it.", None).await;
    let arguments = server.arguments().await;
    assert!(
        arguments.contains(CODEX_WITHHELD),
        "Codex asks Zone about none of its own calls, so a ceiling withholds them: {arguments}"
    );

    server.cleanup().await;
}

#[tokio::test]
async fn a_cli_agent_under_domains_runs_without_its_own_tools() {
    let server = Server::start(Backend::Cli(AgentKind::Claude)).await;
    server.restrict(json!({"domains": [ADMITTED]})).await;
    let unsandboxed = json!({"agent_sandboxed": false});

    let community = server.chat(AGENT_MODEL, unsandboxed.clone()).await;
    server.turn(community, "Write it.", None).await;
    let arguments = server.arguments().await;
    assert!(!arguments.contains(CLAUDE_WITHHELD), "{arguments}");

    server.license(&active(Tier::Team)).await;
    let enforced = server.chat(AGENT_MODEL, unsandboxed).await;
    server.turn(enforced, "Write it.", None).await;
    let arguments = server.arguments().await;
    assert!(
        arguments.contains(CLAUDE_WITHHELD),
        "an agent's own tools reach any host, so a domain list withholds them: {arguments}"
    );

    server.cleanup().await;
}

#[tokio::test]
async fn a_chat_under_domains_loses_web_search_github_and_the_search_prefetch() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"domains": [ADMITTED]})).await;
    let searching = |frames: &[Value]| {
        frames
            .iter()
            .any(|frame| frame["type"] == STATUS && frame["message"] == SEARCHING)
    };
    let requested = Some(json!({"web_search": true}));

    let community = server.chat(MODEL, json!({})).await;
    server.script.push(answer(ANSWER));
    let frames = server.turn(community, NEWS, requested.clone()).await;
    assert!(searching(&frames), "{frames:?}");
    assert_eq!(server.searches().await, 1, "Community searches the web");
    let open = advertised(server.completions().await.last().expect("a completion"));
    for name in [WEB_SEARCH, CREATE_PULL_REQUEST, FETCH_URL] {
        assert!(names(&open, name), "{name} is offered with no policy");
    }

    server.license(&active(Tier::Team)).await;
    let enforced = server.chat(MODEL, json!({})).await;
    server.script.push(answer(ANSWER));
    let frames = server.turn(enforced, NEWS, requested).await;
    assert!(!searching(&frames), "{frames:?}");
    assert_eq!(server.searches().await, 1, "the prefetch is skipped");
    let restricted = advertised(&server.completions().await.pop().expect("a completion"));
    for name in [WEB_SEARCH, CREATE_PULL_REQUEST] {
        assert!(!names(&restricted, name), "{name} reaches the network");
    }
    assert!(
        names(&restricted, FETCH_URL),
        "fetch_url holds itself to the domains, so it is readmitted"
    );

    server.cleanup().await;
}

#[tokio::test]
async fn fetch_url_to_a_host_the_policy_does_not_admit_is_refused_and_audited() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"domains": [ADMITTED]})).await;
    server.license(&active(Tier::Team)).await;

    let chat = server.chat(MODEL, json!({})).await;
    server.script.push(calls(
        "",
        vec![tool(
            "foreign",
            FETCH_URL,
            json!({"url": format!("https://{FOREIGN}/")}),
        )],
    ));
    server.script.push(answer(ANSWER));
    server.turn(chat, "Fetch it.", None).await;

    let request = server.completions().await.pop().expect("a completion");
    let refused = result(&request, "foreign");
    assert!(
        refused.contains(&Denial::Domain.to_string()),
        "{FOREIGN} is not on the list: {refused}"
    );
    let audits = denials(server.pool()).await;
    let [audit] = audits.as_slice() else {
        panic!("one refusal, one audit: {audits:?}");
    };
    assert_eq!(audit.resource_type, resources::CHAT);
    assert_eq!(audit.resource_id, Some(chat));
    assert_eq!(audit.organization_id, Some(server.organization));
    assert!(audit.actor_id.is_some(), "the chat's member is the actor");
    let values = audit.new_values.as_ref().expect("the denial's values");
    assert_eq!(values["code"], Denial::Domain.code());
    assert_eq!(values["detail"], FOREIGN);

    server.cleanup().await;
}

#[tokio::test]
async fn a_chat_offline_on_its_own_is_never_readmitted() {
    let server = Server::start(Backend::Http).await;
    server.restrict(json!({"domains": [ADMITTED]})).await;
    server.license(&active(Tier::Team)).await;

    let chat = server.chat(MODEL, json!({"offline": true})).await;
    server.script.push(answer(ANSWER));
    server.turn(chat, "Fetch it.", None).await;

    let request = server.completions().await.pop().expect("a completion");
    assert!(
        !names(&advertised(&request), FETCH_URL),
        "a chat that opted out of the public web is readmitted nothing"
    );

    server.cleanup().await;
}
