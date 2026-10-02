//! A chat's turns on a coding agent resume the CLI's own session.
//!
//! The first turn on an agent pins a session and replays the whole
//! conversation; every later turn on it resumes that session and sends only
//! what the session has not seen. These tests drive real turns through the
//! websocket against stand-ins for `claude` and `codex` that record how each
//! turn invoked them and what it wrote to their stdin, and read back what the
//! chat stored about its session.

mod common;

use common::context::{finish, next, send, successful};
use common::transcript::{self, Speaker};
use common::{
    TestClient, context_database_url, create_test_router, create_test_state, test_config,
    test_email, test_password,
};

use abnegate_secret::SecretValue;
use chrono::{TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::AgentKind;
use zone_server::config::{AgentConfig, Config, ModelBackend};
use zone_server::db::agent_logins::{self, Insert};
use zone_server::db::ai_settings;
use zone_server::db::chats::{self, ChatSession};
use zone_server::services::login::claude::Tokens;
use zone_server::state::AppState;

const FIRST: &str = "Remember the word heliotrope.";
const SECOND: &str = "Which word did I ask you to remember?";

const CLAUDE_REPLY: &str = "Claude reply";
const CODEX_REPLY: &str = "Codex reply";

const SESSION_ID: &str = "--session-id";
const RESUME: &str = "--resume";
const CODEX_RESUME: &str = "resume";
const CODEX_PROMPT: &str = "-";

/// Claude's words for a session it does not hold, before it starts a turn.
const UNKNOWN_SESSION: &str = "No conversation found with session ID:";

const CODEX_CREDENTIALS_FILE: &str = "auth.json";

/// A codex login's credentials, enough for anything that reads them to find it signed in.
const CODEX_CREDENTIALS: &str = r#"{"tokens":{"access_token":"fake-codex-access-token","refresh_token":"fake-codex-refresh-token","account_id":"fake-codex-account"}}"#;

/// What the stand-in `claude` does with a turn that asks it to resume a session.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Resumption {
    Accepted,
    Refused,
}

/// One run of a stand-in agent: its argument list, and the prompt it read on stdin.
#[derive(Debug, Clone)]
struct Invocation {
    arguments: Vec<String>,
    prompt: String,
}

impl Invocation {
    /// The value that follows `flag` in the argument list.
    fn after(&self, flag: &str) -> Option<&str> {
        self.arguments
            .iter()
            .position(|argument| argument == flag)
            .and_then(|index| self.arguments.get(index + 1))
            .map(String::as_str)
    }

    fn has(&self, flag: &str) -> bool {
        self.arguments.iter().any(|argument| argument == flag)
    }

    /// Asserts the prompt is the whole conversation: the instructions, then
    /// exactly `exchanges` in order, beside this turn's search state.
    fn assert_replays(&self, exchanges: &[(Speaker, &str)]) {
        transcript::assert_replays(&self.prompt, exchanges);
    }

    /// Asserts the prompt is only what a resumed session has not yet seen: an
    /// optional note holding this turn's clock, then `message`. This turn's
    /// search state may ride in the note or follow the message, once.
    fn assert_resumes_with(&self, message: &str) {
        transcript::assert_resumes_with(&self.prompt, message);
    }
}

/// A stand-in for one of the host's coding agents, recording each run in `log`.
struct Agent {
    executable: PathBuf,
    log: PathBuf,
}

impl Agent {
    /// A stand-in `claude`: it announces the session it was pinned to or asked
    /// to resume, as the real one does in its `system/init` event, and with
    /// [`Resumption::Refused`] fails every resume the way Claude fails one for a
    /// session it does not hold.
    fn claude(directory: &Path, resumption: Resumption) -> Self {
        let refusal = match resumption {
            Resumption::Accepted => String::new(),
            Resumption::Refused => format!(
                "if [ -n \"$resumed\" ]; then\n  echo \"{UNKNOWN_SESSION} $id\" >&2\n  exit 1\nfi\n"
            ),
        };
        let body = format!(
            "id=\"unpinned-$n\"\nresumed=\"\"\nprevious=\"\"\nfor argument in \"$@\"; do\n  case \
             \"$previous\" in\n    {SESSION_ID}) id=\"$argument\" ;;\n    {RESUME}) \
             id=\"$argument\"; resumed=1 ;;\n  esac\n  previous=\"$argument\"\ndone\n{refusal}\
             printf '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"%s\"}}\\n' \
             \"$id\"\nprintf \
             '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{CLAUDE_REPLY} \
             %s\"}}]}},\"session_id\":\"%s\"}}\\n' \"$n\" \"$id\"\nprintf \
             '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{CLAUDE_REPLY} \
             %s\",\"session_id\":\"%s\"}}\\n' \"$n\" \"$id\"\n"
        );
        Self::write(directory, AgentKind::Claude, &body)
    }

    /// A stand-in `codex`: a new session announces `thread` as codex announces
    /// the thread it started, and a resumed one the thread it was asked for.
    fn codex(directory: &Path, thread: &str) -> Self {
        let body = format!(
            "id=\"{thread}\"\nprevious=\"\"\nfor argument in \"$@\"; do\n  [ \"$previous\" = \
             \"{CODEX_RESUME}\" ] && id=\"$argument\"\n  previous=\"$argument\"\ndone\nprintf \
             '{{\"type\":\"thread.started\",\"thread_id\":\"%s\"}}\\n' \"$id\"\nprintf \
             '{{\"type\":\"turn.started\"}}\\n'\nprintf \
             '{{\"type\":\"item.completed\",\"item\":{{\"id\":\"item_0\",\"type\":\"agent_message\",\"text\":\"{CODEX_REPLY} \
             %s\"}}}}\\n' \"$n\"\nprintf \
             '{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}}}\\n'\n"
        );
        Self::write(directory, AgentKind::Codex, &body)
    }

    /// Every run is numbered from zero: its arguments, NUL-separated, go to
    /// `<n>.arguments` and its stdin to `<n>.prompt`, before it answers.
    fn write(directory: &Path, agent: AgentKind, body: &str) -> Self {
        let log = directory.join(format!("{}-runs", agent.as_str()));
        std::fs::create_dir(&log).expect("the stand-in's log directory");
        let executable = directory.join(agent.as_str());
        let script = format!(
            "#!/bin/sh\nlog=\"{log}\"\nn=0\nwhile [ -e \"$log/$n.arguments\" ]; do n=$((n + \
             1)); done\ncat > \"$log/$n.prompt\"\nprintf '%s\\0' \"$@\" > \
             \"$log/$n.arguments\"\n{body}",
            log = log.display(),
        );
        std::fs::write(&executable, script).expect("the stand-in agent");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in agent to be executable");
        Self { executable, log }
    }

    fn invocations(&self) -> Vec<Invocation> {
        (0..)
            .map_while(|run| {
                let arguments =
                    std::fs::read_to_string(self.log.join(format!("{run}.arguments"))).ok()?;
                let prompt = std::fs::read_to_string(self.log.join(format!("{run}.prompt")))
                    .expect("a run that recorded its arguments recorded its prompt");
                Some(Invocation {
                    arguments: arguments
                        .split('\0')
                        .filter(|argument| !argument.is_empty())
                        .map(str::to_string)
                        .collect(),
                    prompt,
                })
            })
            .collect()
    }
}

/// A chat in an organization whose turns run on its own coding agent logins.
struct Harness {
    pool: PgPool,
    config: Config,
    state: AppState,
    token: String,
    organization: Uuid,
    chat: Uuid,
    address: String,
    claude: Agent,
    codex: Agent,
    thread: String,
    server: tokio::task::JoinHandle<()>,
    _services: MockServer,
    _directory: TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Harness {
    /// The organization runs on `agent`, signed in once. Every service the
    /// server would otherwise reach, the model catalog and each agent's usage
    /// and token endpoints, is a local mock.
    async fn start(agent: AgentKind, resumption: Resumption) -> Self {
        let services = MockServer::start().await;
        for (verb, route, body) in [
            ("GET", "/v2/model/info", json!({"data": []})),
            ("GET", "/api/ps", json!({"models": []})),
            (
                "GET",
                "/api/oauth/usage",
                json!({"five_hour": {"utilization": 10.0}, "seven_day": {"utilization": 5.0}}),
            ),
            ("GET", "/api/oauth/profile", json!({})),
            (
                "GET",
                "/backend-api/wham/usage",
                json!({"rate_limit": {"primary_window": {"used_percent": 10.0}}}),
            ),
        ] {
            Mock::given(method(verb))
                .and(path(route))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&services)
                .await;
        }

        let directory = TempDir::new().expect("a temporary directory");
        let thread = Uuid::new_v4().to_string();
        let claude = Agent::claude(directory.path(), resumption);
        let codex = Agent::codex(directory.path(), &thread);

        let database = context_database_url();
        let pool = PgPool::connect(&database).await.expect("the test database");
        let mut config = test_config();
        config.database_url = database;
        config.litellm_host = services.uri();
        config.ollama_host = services.uri();
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        config.agents = AgentConfig {
            state: directory.path().join("agents"),
            host_login: false,
            claude_token_url: format!("{}/v1/oauth/token", services.uri()),
            claude_api_url: services.uri(),
            codex_api_url: services.uri(),
            ..AgentConfig::default()
        };

        let state = create_test_state(config.clone(), pool.clone());
        let router = create_test_router(state.clone());
        let client = TestClient::new(router);
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
                &json!({"name": "Chat sessions", "slug": format!("sessions-{suffix}")}),
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
                &json!({"name": "Chat sessions", "slug": format!("sessions-{suffix}")}),
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
                    "title": "Chat sessions",
                    "model_name": "sonnet",
                    "agent_enabled": false,
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

        let mut harness = Self {
            pool,
            config,
            state,
            token,
            organization,
            chat,
            address: String::new(),
            claude,
            codex,
            thread,
            server: tokio::spawn(async {}),
            _services: services,
            _directory: directory,
        };
        harness.run_on(agent).await;
        harness.sign_in(agent).await;
        harness
    }

    /// Points the organization's AI settings at `agent`, and serves the chat
    /// from a server whose `agent` binary is the stand-in.
    async fn run_on(&mut self, agent: AgentKind) {
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2) \
             ON CONFLICT (organization_id) DO UPDATE SET provider = EXCLUDED.provider",
        )
        .bind(self.organization)
        .bind(ai_settings::provider(agent))
        .execute(&self.pool)
        .await
        .expect("the organization's AI settings");

        self.config.model_backend = ModelBackend::Cli {
            agent,
            executable: Some(self.agent(agent).executable.clone()),
        };
        self.server.abort();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        self.address = listener
            .local_addr()
            .expect("the bound address")
            .to_string();
        self.state = create_test_state(self.config.clone(), self.pool.clone());
        let router = create_test_router(self.state.clone());
        self.server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
    }

    fn agent(&self, agent: AgentKind) -> &Agent {
        match agent {
            AgentKind::Claude => &self.claude,
            AgentKind::Codex => &self.codex,
        }
    }

    /// Signs the organization in to `agent` once more: a Claude login holds a
    /// token far from expiry, and a codex login its own `auth.json`.
    async fn sign_in(&self, agent: AgentKind) -> Uuid {
        let sealed = match agent {
            AgentKind::Claude => Some(
                Tokens {
                    access: SecretValue::new("fake-claude-access-token"),
                    refresh: None,
                    expires_at: Utc::now() + TimeDelta::days(30),
                    issued_at: None,
                    scope: "user:inference".to_string(),
                    subscription: None,
                }
                .seal(self.state.encryption_key())
                .expect("the tokens to seal"),
            ),
            AgentKind::Codex => None,
        };
        let account = Uuid::new_v4().to_string();
        let login = agent_logins::insert(
            &self.pool,
            &Insert {
                organization_id: self.organization,
                agent: agent.as_str(),
                account: Some(&account),
                credential: sealed.as_deref(),
                label: Some(&account),
                expires_at: None,
            },
        )
        .await
        .expect("a stored login")
        .id;
        if agent == AgentKind::Codex {
            let home = self
                .config
                .agents
                .create_login_home(self.organization, agent, login)
                .expect("the login's home");
            std::fs::write(home.join(CODEX_CREDENTIALS_FILE), CODEX_CREDENTIALS)
                .expect("the codex login's credentials");
        }
        login
    }

    async fn turn(&self, content: &str) -> Vec<Value> {
        let (mut socket, _) =
            connect_async(format!("ws://{}/ws/chats/{}", self.address, self.chat))
                .await
                .expect("the chat socket");
        send(&mut socket, json!({"type": "auth", "token": self.token})).await;
        let initial = next(&mut socket).await;
        assert_eq!(initial["type"], "init", "initial frame: {initial}");
        send(&mut socket, json!({"type": "send", "content": content})).await;
        let frames = finish(&mut socket).await;
        let _ = socket.close(None).await;
        frames
    }

    async fn session(&self) -> Option<ChatSession> {
        chats::session(&self.pool, self.chat)
            .await
            .expect("the chat's session to be readable")
    }

    /// The position of the chat's latest entry.
    async fn latest(&self) -> i64 {
        sqlx::query_scalar("SELECT MAX(position) FROM chat_entries WHERE chat_id = $1")
            .bind(self.chat)
            .fetch_one(&self.pool)
            .await
            .expect("the chat's entries to be readable")
    }

    async fn login(&self, agent: AgentKind) -> Uuid {
        let logins = agent_logins::list_for(&self.pool, self.organization, agent.as_str())
            .await
            .expect("the organization's logins");
        match logins.as_slice() {
            [only] => only.id,
            logins => panic!("expected one {agent} login, found {}", logins.len()),
        }
    }

    /// Asserts the chat's session is `id` on `agent`'s login, has seen every
    /// entry the chat holds, and remembers the prompt it saw by its hash.
    async fn assert_session(&self, agent: AgentKind, id: &str) -> ChatSession {
        let session = self
            .session()
            .await
            .unwrap_or_else(|| panic!("a turn on {agent} must keep its session"));
        assert_eq!(session.id, id, "the session the CLI announced: {session:?}");
        assert_eq!(session.agent, agent, "{session:?}");
        assert_eq!(
            session.login,
            Some(self.login(agent).await),
            "the session runs on the login the turn ran on: {session:?}"
        );
        assert_eq!(
            session.entry,
            self.latest().await,
            "a finished turn's session has seen every entry: {session:?}"
        );
        assert!(
            session
                .prompt
                .as_deref()
                .is_some_and(|hash| hash.len() == 64
                    && hash.chars().all(|character| character.is_ascii_hexdigit())),
            "the session remembers the prompt it saw as a SHA-256: {session:?}"
        );
        session
    }
}

fn reply(agent: AgentKind, run: usize) -> String {
    match agent {
        AgentKind::Claude => format!("{CLAUDE_REPLY} {run}"),
        AgentKind::Codex => format!("{CODEX_REPLY} {run}"),
    }
}

#[tokio::test]
async fn a_first_turn_pins_a_session_and_a_later_turn_resumes_it_with_only_the_new_message() {
    let harness = Harness::start(AgentKind::Claude, Resumption::Accepted).await;

    successful(&harness.turn(FIRST).await);
    let runs = harness.claude.invocations();
    let [first] = runs.as_slice() else {
        panic!("one turn runs the agent once: {runs:?}");
    };
    let id = first
        .after(SESSION_ID)
        .unwrap_or_else(|| panic!("a first turn pins its session: {:?}", first.arguments))
        .to_string();
    assert!(!first.has(RESUME), "{:?}", first.arguments);
    first.assert_replays(&[(Speaker::User, FIRST)]);
    let pinned = harness.assert_session(AgentKind::Claude, &id).await;

    successful(&harness.turn(SECOND).await);
    let runs = harness.claude.invocations();
    let [_, second] = runs.as_slice() else {
        panic!("each turn runs the agent once: {runs:?}");
    };
    assert_eq!(
        second.after(RESUME),
        Some(id.as_str()),
        "a later turn resumes the session it pinned: {:?}",
        second.arguments
    );
    assert!(!second.has(SESSION_ID), "{:?}", second.arguments);
    second.assert_resumes_with(SECOND);

    let resumed = harness.assert_session(AgentKind::Claude, &id).await;
    assert!(resumed.entry > pinned.entry, "{pinned:?} then {resumed:?}");
    assert_eq!(
        resumed.prompt, pinned.prompt,
        "a prompt that was not sent again keeps the hash the session saw"
    );
}

#[tokio::test]
async fn a_codex_turn_learns_its_thread_id_from_the_stream_and_resumes_it() {
    let harness = Harness::start(AgentKind::Codex, Resumption::Accepted).await;

    successful(&harness.turn(FIRST).await);
    let runs = harness.codex.invocations();
    let [first] = runs.as_slice() else {
        panic!("one turn runs the agent once: {runs:?}");
    };
    assert!(
        !first.has(CODEX_RESUME),
        "codex picks a new session's id itself: {:?}",
        first.arguments
    );
    first.assert_replays(&[(Speaker::User, FIRST)]);
    harness
        .assert_session(AgentKind::Codex, &harness.thread)
        .await;

    successful(&harness.turn(SECOND).await);
    let runs = harness.codex.invocations();
    let [_, second] = runs.as_slice() else {
        panic!("each turn runs the agent once: {runs:?}");
    };
    assert!(
        second.arguments.ends_with(&[
            CODEX_RESUME.to_string(),
            harness.thread.clone(),
            CODEX_PROMPT.to_string(),
        ]),
        "a later turn resumes the thread codex announced, reading its prompt from stdin: {:?}",
        second.arguments
    );
    second.assert_resumes_with(SECOND);
    harness
        .assert_session(AgentKind::Codex, &harness.thread)
        .await;
}

#[tokio::test]
async fn a_resume_the_cli_refuses_falls_back_to_a_full_replay_under_a_fresh_id() {
    let harness = Harness::start(AgentKind::Claude, Resumption::Refused).await;

    successful(&harness.turn(FIRST).await);
    let runs = harness.claude.invocations();
    let [first] = runs.as_slice() else {
        panic!("one turn runs the agent once: {runs:?}");
    };
    let refused = first
        .after(SESSION_ID)
        .unwrap_or_else(|| panic!("a first turn pins its session: {:?}", first.arguments))
        .to_string();

    successful(&harness.turn(SECOND).await);
    let runs = harness.claude.invocations();
    let [_, resume, replay] = runs.as_slice() else {
        panic!("a refused resume runs the agent once more, and only once: {runs:?}");
    };
    assert_eq!(
        resume.after(RESUME),
        Some(refused.as_str()),
        "{:?}",
        resume.arguments
    );
    assert!(
        !replay.has(RESUME),
        "the rerun starts over: {:?}",
        replay.arguments
    );
    let fresh = replay
        .after(SESSION_ID)
        .unwrap_or_else(|| panic!("the rerun pins a session: {:?}", replay.arguments))
        .to_string();
    assert_ne!(
        fresh, refused,
        "the rerun's session is a fresh one, not the one refused"
    );
    replay.assert_replays(&[
        (Speaker::User, FIRST),
        (Speaker::Assistant, &reply(AgentKind::Claude, 0)),
        (Speaker::User, SECOND),
    ]);
    harness.assert_session(AgentKind::Claude, &fresh).await;
}

#[tokio::test]
async fn an_http_backend_keeps_no_session() {
    let harness = common::context::Harness::new(
        None,
        false,
        vec![
            common::context::answer("The word is heliotrope."),
            common::context::answer("You asked me to remember heliotrope."),
        ],
    )
    .await;

    successful(&harness.turn(FIRST).await);
    assert_eq!(
        chats::session(&harness.pool, harness.chat)
            .await
            .expect("the chat's session to be readable"),
        None,
        "a turn over HTTP has no CLI session to keep"
    );

    successful(&harness.turn(SECOND).await);
    assert_eq!(
        chats::session(&harness.pool, harness.chat)
            .await
            .expect("the chat's session to be readable"),
        None,
        "a turn over HTTP has no CLI session to keep"
    );
    let requests = harness.requests().await;
    let ordinary = common::context::ordinary(&requests);
    let last = ordinary.last().expect("the second turn's request");
    let users: Vec<&str> = last["messages"]
        .as_array()
        .expect("the request's messages")
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .collect();
    assert!(
        users.iter().any(|content| content.contains(FIRST)),
        "an HTTP turn still carries the whole conversation: {users:?}"
    );
}

#[tokio::test]
async fn a_chat_on_a_new_agent_starts_a_new_session_with_the_whole_transcript() {
    let mut harness = Harness::start(AgentKind::Claude, Resumption::Accepted).await;

    successful(&harness.turn(FIRST).await);
    let runs = harness.claude.invocations();
    let [first] = runs.as_slice() else {
        panic!("one turn runs the agent once: {runs:?}");
    };
    let pinned = first
        .after(SESSION_ID)
        .unwrap_or_else(|| panic!("a first turn pins its session: {:?}", first.arguments))
        .to_string();
    harness.assert_session(AgentKind::Claude, &pinned).await;

    let claude = harness.login(AgentKind::Claude).await;
    agent_logins::exhaust(&harness.pool, claude, Utc::now() + TimeDelta::hours(5))
        .await
        .expect("the Claude login to be exhausted");
    harness.run_on(AgentKind::Codex).await;
    harness.sign_in(AgentKind::Codex).await;

    successful(&harness.turn(SECOND).await);
    assert_eq!(
        harness.claude.invocations().len(),
        1,
        "the exhausted Claude login runs nothing more"
    );
    let runs = harness.codex.invocations();
    let [moved] = runs.as_slice() else {
        panic!("the turn runs codex once: {runs:?}");
    };
    assert!(
        !moved.has(CODEX_RESUME),
        "a session on another agent is not one codex can resume: {:?}",
        moved.arguments
    );
    moved.assert_replays(&[
        (Speaker::User, FIRST),
        (Speaker::Assistant, &reply(AgentKind::Claude, 0)),
        (Speaker::User, SECOND),
    ]);
    harness
        .assert_session(AgentKind::Codex, &harness.thread)
        .await;
}
