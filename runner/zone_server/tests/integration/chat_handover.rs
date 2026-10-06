//! A chat turn whose coding agent login runs out partway through carries on,
//! in the same reply, on another of the organization's logins.
//!
//! These tests drive real turns through the websocket against stand-ins for
//! `claude` and `codex`. The stand-in `claude` behaves per login, told apart
//! by the token it runs under: it answers, or streams the start of an answer
//! and then refuses the rest the way an account past its limit, out of usage
//! credits or signed out does. Like the real one it keeps each session in a
//! file in its login's home, and refuses to resume a session whose file is
//! not there. Every run of either stand-in records how it was invoked, where,
//! under which home and MCP token, and what it read on stdin.

use crate::common::context::{finish, next, send, successful};
use crate::common::transcript::{self, Speaker};
use crate::common::{
    TestClient, context_database_url, create_test_router, create_test_state, test_config,
    test_email, test_password,
};

use abnegate_secret::SecretValue;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::AgentKind;
use zone_core::llm::provider::UNFUNDED;
use zone_server::config::{AgentConfig, Config, ModelBackend};
use zone_server::db::agent_logins::{self, AgentLoginRow, Insert};
use zone_server::db::ai_settings;
use zone_server::db::chats::{self, ChatSession};
use zone_server::services::agent::sessions;
use zone_server::services::backend;
use zone_server::services::login::claude::Tokens;
use zone_server::services::login::router::unfunded;
use zone_server::state::AppState;

const QUESTION: &str = "Why are there two tides a day?";
const FOLLOW_UP: &str = "And why are spring tides higher?";

/// What a login streams before it is refused. Its trailing space is the
/// reader's evidence that the reply is joined as streamed, not re-spaced.
const PARTIAL: &str = "The moon pulls the near ocean ";
const ANSWER: &str = "and the earth is pulled away from the far one.";
const CODEX_ANSWER: &str = "and the far ocean is left behind.";
/// What a stand-in `claude` answers on a run nothing else was planned for.
const REPLY: &str = "Gravity, mostly.";

/// What a turn handed to a session that already holds the partial answer is
/// told, and what a replayed one is told after it.
const CONTINUE: &str =
    "Continue the answer from where it stopped, without repeating what was already written.";

const SESSION_LIMIT: &str = "You've hit your session limit · resets 5pm";
const FABLE_REFUSAL: &str =
    "Fable 5.1 requires usage credits. Switch to another model to continue.";
const INVALID_KEY: &str = "Invalid API key · Please run /login";
/// Claude's words for a session it does not hold, before it starts a turn.
const UNKNOWN_SESSION: &str = "No conversation found with session ID:";
/// The wording every limit the chat could not hand over ends the turn in.
const EXHAUSTED: &str = "has reached its usage limit";

const PARTIAL_INPUT_TOKENS: u32 = 3;
const PARTIAL_OUTPUT_TOKENS: u32 = 2;
const ANSWER_INPUT_TOKENS: u32 = 5;
const ANSWER_OUTPUT_TOKENS: u32 = 7;

const SESSION_ID: &str = "--session-id";
const RESUME: &str = "--resume";
const MODEL: &str = "--model";
const CODEX_RESUME: &str = "resume";

const MESSAGE_START: &str = "message_start";
const MESSAGE_END: &str = "message_end";
const HANDOVER: &str = "handover";
const ERROR: &str = "error";

const LIMIT: &str = "limit";
const CREDITS: &str = "credits";
const SIGNED_OUT: &str = "signed_out";
const CONFIGURED: &str = "configured";

const RESETS_AT: &str = "resets_at";

/// The fields a handover frame and its stored record share.
const HANDOVER_FIELDS: [&str; 8] = [
    "from",
    "to",
    "from_agent",
    "agent",
    "reason",
    RESETS_AT,
    "carried",
    "at",
];

/// The directory under each test's own that holds its organizations' agent
/// homes, by which a run looked up on `PATH` is passed to that test's stand-in.
const STATE: &str = "handover-agents";

const CODEX_CREDENTIALS_FILE: &str = "auth.json";

/// A codex login's credentials, enough for anything that reads them to find it signed in.
const CODEX_CREDENTIALS: &str = r#"{"tokens":{"access_token":"fake-codex-access-token","refresh_token":"fake-codex-refresh-token","account_id":"fake-codex-account"}}"#;

const CAROL: &str = "carol@example.com";

/// One of the organization's Claude sign-ins: who it is, the token its turns
/// run under, and how much of its five-hour window it has used.
#[derive(Clone, Copy)]
struct Account {
    label: &'static str,
    token: &'static str,
    used: f64,
}

/// The login with the most headroom, which a new chat starts on.
const ALICE: Account = Account {
    label: "alice@example.com",
    token: "claude-token-alice",
    used: 10.0,
};

/// The login with less headroom, which a chat moves to only when Alice is out.
const BOB: Account = Account {
    label: "bob@example.com",
    token: "claude-token-bob",
    used: 80.0,
};

/// Where a run of either agent looked up by name on `PATH` finds this binary's
/// stand-ins.
///
/// A turn handed over to the agent the server was not started with runs that
/// agent by name. Each script here passes the run on to the stand-in of the
/// test whose agent home it runs in, so no run ever reaches a real CLI. Set
/// once for the whole binary: an environment written while another test
/// thread reads one is why the call is unsafe at all.
static STAND_INS: LazyLock<PathBuf> = LazyLock::new(|| {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("chat-handover-stand-ins");
    std::fs::create_dir_all(&directory).expect("a directory for the stand-ins on PATH");
    for agent in AgentKind::ALL {
        let name = agent.executable();
        let staged = directory.join(format!("{name}.{}", std::process::id()));
        std::fs::write(
            &staged,
            format!(
                "#!/bin/sh\nhome=\"${{CLAUDE_CONFIG_DIR:-$CODEX_HOME}}\"\ncase \"$home\" in\n  \
                 */{STATE}/*) exec \"${{home%%/{STATE}/*}}/{name}\" \"$@\" ;;\nesac\necho \
                 \"{name}: no stand-in serves this run\" >&2\nexit 127\n"
            ),
        )
        .expect("the stand-in on PATH");
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in on PATH to be executable");
        std::fs::rename(&staged, directory.join(name)).expect("the stand-in in place on PATH");
    }
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let searched = std::env::join_paths(
        std::iter::once(directory.clone()).chain(std::env::split_paths(&inherited)),
    )
    .expect("a PATH that leads with the stand-ins");
    unsafe { std::env::set_var("PATH", searched) };
    directory
});

/// Whether the stand-in `claude` keeps the session file the real one writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sessions {
    Kept,
    Lost,
}

/// Whether the chat's turns are served zone's tools over an MCP lease.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tools {
    Served,
    Withheld,
}

/// When the frame recording a handover says the login it left resets.
#[derive(Debug, Clone, Copy)]
enum Reset {
    At(DateTime<Utc>),
    Unreported,
    Unchecked,
}

/// What a handover frame and the stored record of it are expected to say.
struct Expected<'a> {
    from: &'a str,
    to: &'a str,
    from_agent: AgentKind,
    agent: AgentKind,
    reason: &'a str,
    carried: bool,
    at: usize,
    resets: Reset,
}

/// How one run of the stand-in `claude` under a login goes, after it has
/// announced its session.
#[derive(Clone, Copy)]
enum Run<'a> {
    /// It answers in full.
    Answers(&'a str),
    /// It streams `partial`, then refuses the rest as an account past its
    /// five-hour window, which resets at `resets_at`.
    Limited {
        partial: Option<&'a str>,
        resets_at: DateTime<Utc>,
    },
    /// It streams `partial`, then refuses the rest as an account that cannot
    /// spend usage credits on the model.
    Unfunded { partial: Option<&'a str> },
    /// It streams `partial`, then fails as a login whose sign-in was revoked.
    SignedOut { partial: Option<&'a str> },
}

impl Run<'_> {
    fn lines(self) -> Vec<Value> {
        match self {
            Self::Answers(text) => vec![
                json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}),
                json!({
                    "type": "result",
                    "subtype": "success",
                    "is_error": false,
                    "result": text,
                    "usage": {"input_tokens": ANSWER_INPUT_TOKENS, "output_tokens": ANSWER_OUTPUT_TOKENS},
                }),
            ],
            Self::Limited { partial, resets_at } => streamed(partial)
                .into_iter()
                .chain([
                    json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": resets_at.timestamp(), "rateLimitType": "five_hour", "isUsingOverage": false}}),
                    json!({"type": "assistant", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": SESSION_LIMIT}]}, "parent_tool_use_id": null, "error": "rate_limit", "is_api_error_message": true}),
                    json!({"type": "result", "subtype": "success", "is_error": true, "api_error_status": 429, "result": SESSION_LIMIT}),
                ])
                .collect(),
            Self::Unfunded { partial } => streamed(partial)
                .into_iter()
                .chain([
                    json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "overageStatus": "rejected", "overageDisabledReason": "overage_not_provisioned", "isUsingOverage": false, "errorCode": "credits_required"}}),
                    json!({"type": "assistant", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": FABLE_REFUSAL}]}, "parent_tool_use_id": null, "error": "rate_limit", "is_api_error_message": true, "api_error": "model_requires_usage_credits"}),
                    json!({"type": "result", "subtype": "success", "is_error": true, "api_error_status": 429, "result": FABLE_REFUSAL}),
                ])
                .collect(),
            Self::SignedOut { partial } => streamed(partial)
                .into_iter()
                .chain([json!({"type": "result", "subtype": "success", "is_error": true, "result": INVALID_KEY})])
                .collect(),
        }
    }
}

/// The line that streams `partial`, when there is one, with the tokens it took.
fn streamed(partial: Option<&str>) -> Option<Value> {
    partial.map(|text| {
        json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "text", "text": text}],
                "usage": {"input_tokens": PARTIAL_INPUT_TOKENS, "output_tokens": PARTIAL_OUTPUT_TOKENS},
            },
        })
    })
}

/// A moment `after` from now, to the second, as a refusal names its reset.
fn reset_in(after: TimeDelta) -> DateTime<Utc> {
    DateTime::from_timestamp((Utc::now() + after).timestamp(), 0).expect("a reset time")
}

/// One run of a stand-in agent.
#[derive(Debug, Clone)]
struct Invocation {
    arguments: Vec<String>,
    /// What it read on stdin.
    prompt: String,
    /// Its working directory, every link resolved.
    directory: PathBuf,
    /// The home its agent keeps its settings, sign-in and sessions in.
    home: PathBuf,
    /// The Claude token it ran under; empty for codex.
    token: String,
    /// The token it reaches zone's tools with; empty when it was served none.
    tools: String,
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

    /// The session it was pinned to as a new one.
    fn pinned(&self) -> &str {
        self.after(SESSION_ID)
            .unwrap_or_else(|| panic!("a new session is pinned: {:?}", self.arguments))
    }

    /// Asserts the prompt is only the user's instruction to carry on, which is
    /// all a session that already holds the partial answer needs.
    fn assert_continues(&self) {
        let blocks = transcript::blocks(&self.prompt);
        assert!(
            matches!(
                blocks.as_slice(),
                [only] if only.content == CONTINUE && only.speaker == Speaker::User
            ),
            "a carried session is told only to continue: {}",
            self.prompt
        );
    }

    /// Asserts the prompt is only what a resumed session has not yet seen:
    /// this turn's note, then `message`.
    fn assert_resumes_with(&self, message: &str) {
        transcript::assert_resumes_with(&self.prompt, message);
    }

    /// Asserts the prompt replays the conversation so far, the partial answer
    /// as the assistant's, followed by the user's instruction to carry on. It is
    /// the user's because system entries are hoisted above the conversation,
    /// where it would precede the answer it asks to continue.
    fn assert_replays_with_the_partial(&self) {
        transcript::assert_replays(
            &self.prompt,
            &[
                (Speaker::User, QUESTION),
                (Speaker::Assistant, PARTIAL.trim()),
                (Speaker::User, CONTINUE),
            ],
        );
    }
}

/// A stand-in for one of the host's coding agents. Run `n` records itself in
/// `runs/n`; the stand-in `claude` plays the `k`th run under a token from
/// `plans/<token>.k.jsonl`, and answers [`REPLY`] when there is none.
struct Agent {
    executable: PathBuf,
    runs: PathBuf,
    plans: PathBuf,
}

impl Agent {
    fn claude(directory: &Path, sessions: Sessions) -> Self {
        let agent = Self::new(directory, AgentKind::Claude);
        let keeping = match sessions {
            Sessions::Kept => {
                "mkdir -p \"$(dirname \"$session\")\"\nprintf '{\"type\":\"user\",\"sessionId\":\"%s\"}\\n' \"$id\" >> \"$session\"\n"
            }
            Sessions::Lost => "",
        };
        std::fs::write(
            agent.plans.join("default.jsonl"),
            jsonl(&Run::Answers(REPLY).lines()),
        )
        .expect("the stand-in's default answer");
        let body = format!(
            "id=\"\"\nresumed=\"\"\nprevious=\"\"\nfor argument in \"$@\"; do\n  case \"$previous\" \
             in\n    {SESSION_ID}) id=\"$argument\" ;;\n    {RESUME}) id=\"$argument\"; resumed=1 \
             ;;\n  esac\n  previous=\"$argument\"\ndone\nsession=\"$CLAUDE_CONFIG_DIR/projects/$(pwd \
             -P | sed 's/[^A-Za-z0-9]/-/g')/$id.jsonl\"\nif [ -n \"$resumed\" ] && [ ! -f \
             \"$session\" ]; then\n  echo \"{UNKNOWN_SESSION} $id\" >&2\n  exit 1\nfi\n{keeping}printf \
             '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"%s\"}}\\n' \"$id\"\nk=0\nwhile \
             ! mkdir \"$plans/$CLAUDE_CODE_OAUTH_TOKEN.$k\" 2>/dev/null; do k=$((k + 1)); \
             done\nplan=\"$plans/$CLAUDE_CODE_OAUTH_TOKEN.$k.jsonl\"\n[ -f \"$plan\" ] || \
             plan=\"$plans/default.jsonl\"\ncat \"$plan\"\n"
        );
        agent.install("CLAUDE_CONFIG_DIR", &body)
    }

    /// A stand-in `codex` that answers [`CODEX_ANSWER`], announcing `thread`
    /// as codex announces a thread it started, or the one it was asked to resume.
    fn codex(directory: &Path, thread: &str) -> Self {
        let agent = Self::new(directory, AgentKind::Codex);
        std::fs::write(
            agent.plans.join("default.jsonl"),
            jsonl(&[
                json!({"type": "turn.started"}),
                json!({"type": "item.completed", "item": {"id": "item_0", "type": "agent_message", "text": CODEX_ANSWER}}),
                json!({"type": "turn.completed", "usage": {"input_tokens": 1, "cached_input_tokens": 0, "output_tokens": 1}}),
            ]),
        )
        .expect("the stand-in's answer");
        let body = format!(
            "id=\"{thread}\"\nprevious=\"\"\nfor argument in \"$@\"; do\n  [ \"$previous\" = \
             \"{CODEX_RESUME}\" ] && id=\"$argument\"\n  previous=\"$argument\"\ndone\nprintf \
             '{{\"type\":\"thread.started\",\"thread_id\":\"%s\"}}\\n' \"$id\"\ncat \
             \"$plans/default.jsonl\"\n"
        );
        agent.install("CODEX_HOME", &body)
    }

    fn new(directory: &Path, agent: AgentKind) -> Self {
        let name = agent.executable();
        let runs = directory.join(format!("{name}-runs"));
        let plans = directory.join(format!("{name}-plans"));
        std::fs::create_dir(&runs).expect("the stand-in's run log");
        std::fs::create_dir(&plans).expect("the stand-in's plans");
        Self {
            executable: directory.join(name),
            runs,
            plans,
        }
    }

    /// Writes the stand-in: every run records itself, before it reads the
    /// home its agent names in `home` for anything else, then runs `body`.
    fn install(self, home: &str, body: &str) -> Self {
        let script = format!(
            "#!/bin/sh\nruns='{runs}'\nplans='{plans}'\nn=0\nwhile ! mkdir \"$runs/$n\" 2>/dev/null; \
             do n=$((n + 1)); done\nrun=\"$runs/$n\"\nprintf '%s\\0' \"$@\" > \"$run/arguments\"\npwd \
             -P > \"$run/directory\"\nprintf '%s' \"${home}\" > \"$run/home\"\nprintf '%s' \
             \"$CLAUDE_CODE_OAUTH_TOKEN\" > \"$run/token\"\nprintf '%s' \"$ZONE_MCP_TOKEN\" > \
             \"$run/tools\"\ncat > \"$run/prompt\"\n{body}",
            runs = self.runs.display(),
            plans = self.plans.display(),
        );
        std::fs::write(&self.executable, script).expect("the stand-in agent");
        std::fs::set_permissions(&self.executable, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in agent to be executable");
        self
    }

    /// The `run`th run under `account`'s token, counted from zero, goes as `plan` says.
    fn plan(&self, account: Account, run: usize, plan: Run<'_>) {
        std::fs::write(
            self.plans.join(format!("{}.{run}.jsonl", account.token)),
            jsonl(&plan.lines()),
        )
        .expect("the stand-in's plan");
    }

    fn invocations(&self) -> Vec<Invocation> {
        (0..)
            .map_while(|run| {
                let directory = self.runs.join(run.to_string());
                let read = |name: &str| {
                    std::fs::read_to_string(directory.join(name)).unwrap_or_else(|error| {
                        panic!("run {run} recorded no {name}: {error}");
                    })
                };
                directory.is_dir().then(|| Invocation {
                    arguments: read("arguments")
                        .split('\0')
                        .filter(|argument| !argument.is_empty())
                        .map(str::to_string)
                        .collect(),
                    prompt: read("prompt"),
                    directory: PathBuf::from(read("directory").trim_end()),
                    home: PathBuf::from(read("home")),
                    token: read("token"),
                    tools: read("tools"),
                })
            })
            .collect()
    }
}

fn jsonl(lines: &[Value]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// A chat in an organization whose turns run on Claude, signed in more than
/// once, served from a server whose own `claude` is the stand-in.
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
    services: MockServer,
    server: tokio::task::JoinHandle<()>,
    _directory: TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Harness {
    /// Every service the server would otherwise reach, the model catalog and
    /// each agent's usage and token endpoints, is a local mock.
    async fn start(sessions: Sessions, tools: Tools) -> Self {
        LazyLock::force(&STAND_INS);
        let services = MockServer::start().await;
        for (route, body) in [
            ("/v2/model/info", json!({"data": []})),
            ("/api/ps", json!({"models": []})),
            ("/api/oauth/profile", json!({})),
            (
                "/backend-api/wham/usage",
                json!({"rate_limit": {"primary_window": {"used_percent": 10.0}}}),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path(route))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&services)
                .await;
        }

        let directory = TempDir::new().expect("a temporary directory");
        let thread = Uuid::new_v4().to_string();
        let claude = Agent::claude(directory.path(), sessions);
        let codex = Agent::codex(directory.path(), &thread);

        let database = context_database_url();
        let pool = PgPool::connect(&database).await.expect("the test database");
        let mut config = test_config();
        config.database_url = database;
        config.litellm_host = services.uri();
        config.ollama_host = services.uri();
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        config.model_backend = ModelBackend::Cli {
            agent: AgentKind::Claude,
            executable: Some(claude.executable.clone()),
        };
        config.agents = AgentConfig {
            state: directory.path().join(STATE),
            host_login: false,
            claude_token_url: format!("{}/v1/oauth/token", services.uri()),
            claude_api_url: services.uri(),
            codex_api_url: services.uri(),
            ..AgentConfig::default()
        };

        let state = create_test_state(config.clone(), pool.clone());
        let client = TestClient::new(create_test_router(state.clone()));
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
                &json!({"name": "Chat handover", "slug": format!("handover-{suffix}")}),
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
                &json!({"name": "Chat handover", "slug": format!("handover-{suffix}")}),
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
                    "title": "Chat handover",
                    "model_name": "sonnet",
                    "agent_enabled": tools == Tools::Served,
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

        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2) \
             ON CONFLICT (organization_id) DO UPDATE SET provider = EXCLUDED.provider",
        )
        .bind(organization)
        .bind(ai_settings::provider(AgentKind::Claude))
        .execute(&pool)
        .await
        .expect("the organization's AI settings");

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener
            .local_addr()
            .expect("the bound address")
            .to_string();
        let router = create_test_router(state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        Self {
            pool,
            config,
            state,
            token,
            organization,
            chat,
            address,
            claude,
            codex,
            thread,
            services,
            server,
            _directory: directory,
        }
    }

    /// Signs the organization in to Claude as `account`, with a token far from
    /// expiry, whose usage reads as `account.used` percent of its window.
    async fn sign_in(&self, account: Account) -> Uuid {
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .and(header(
                "authorization",
                format!("Bearer {}", account.token).as_str(),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "five_hour": {"utilization": account.used},
                "seven_day": {"utilization": account.used / 2.0},
            })))
            .mount(&self.services)
            .await;
        let sealed = Tokens {
            access: SecretValue::new(account.token),
            refresh: None,
            expires_at: Utc::now() + TimeDelta::days(30),
            issued_at: None,
            scope: "user:inference".to_string(),
            subscription: None,
        }
        .seal(self.state.encryption_key())
        .expect("the tokens to seal");
        self.insert(AgentKind::Claude, account.label, Some(&sealed))
            .await
    }

    /// Signs the organization in to codex as `label`, holding its own `auth.json`.
    async fn sign_in_codex(&self, label: &str) -> Uuid {
        let login = self.insert(AgentKind::Codex, label, None).await;
        let home = self
            .config
            .agents
            .create_login_home(self.organization, AgentKind::Codex, login)
            .expect("the login's home");
        std::fs::write(home.join(CODEX_CREDENTIALS_FILE), CODEX_CREDENTIALS)
            .expect("the codex login's credentials");
        login
    }

    async fn insert(&self, agent: AgentKind, label: &str, credential: Option<&str>) -> Uuid {
        agent_logins::insert(
            &self.pool,
            &Insert {
                organization_id: self.organization,
                agent: agent.as_str(),
                account: Some(label),
                credential,
                label: Some(label),
                expires_at: None,
            },
        )
        .await
        .expect("a stored login")
        .id
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

    async fn session(&self) -> ChatSession {
        chats::session(&self.pool, self.chat)
            .await
            .expect("the chat's session to be readable")
            .expect("a turn on a coding agent keeps its session")
    }

    async fn login(&self, login: Uuid) -> AgentLoginRow {
        agent_logins::get(&self.pool, login)
            .await
            .expect("the login to be readable")
            .expect("the login to be stored")
    }

    /// Clears what Alice's limit recorded, as though her window had since
    /// reset, leaving her the login a fresh pick would rank first.
    async fn refresh(&self, login: Uuid) {
        sqlx::query(
            "UPDATE agent_logins SET exhausted_until = NULL, windows = NULL, headroom = NULL, \
             usage_fetched_at = NULL WHERE id = $1",
        )
        .bind(login)
        .execute(&self.pool)
        .await
        .expect("the login's usage to be cleared");
    }

    /// The metadata stored with the assistant message `message_id`, as a reload reads it.
    async fn stored(&self, message_id: &Value) -> Value {
        let reloaded = TestClient::new(create_test_router(self.state.clone()))
            .get_auth(&format!("/api/chats/{}", self.chat), &self.token)
            .await
            .json_value();
        reloaded["chat"]["messages"]
            .as_array()
            .expect("the chat's messages")
            .iter()
            .find(|message| &message["id"] == message_id)
            .unwrap_or_else(|| panic!("the reply {message_id} is stored: {reloaded}"))["metadata"]
            .clone()
    }

    fn home(&self, agent: AgentKind, login: Uuid) -> PathBuf {
        self.config
            .agents
            .login_home(self.organization, agent, login)
    }

    /// The organization's working directory for `agent`, as a CLI run in it sees it.
    fn work(&self, agent: AgentKind) -> PathBuf {
        std::fs::canonicalize(self.config.agents.work(self.organization, agent))
            .expect("the agent's working directory")
    }

    /// Whether session `id`'s file is in `login`'s home, where Claude resumes it from.
    fn holds(&self, login: Uuid, id: &str) -> bool {
        sessions::locate(
            &self.home(AgentKind::Claude, login),
            AgentKind::Claude,
            &self
                .config
                .agents
                .work(self.organization, AgentKind::Claude),
            id,
        )
        .is_some()
    }
}

fn kinds<'a>(frames: &'a [Value], kind: &str) -> Vec<&'a Value> {
    frames
        .iter()
        .filter(|frame| frame["type"] == kind)
        .collect()
}

/// The reply's one `message_start`, which every handover frame is about.
fn started(frames: &[Value]) -> &Value {
    match kinds(frames, MESSAGE_START).as_slice() {
        [start] => start,
        starts => panic!(
            "a turn is one reply, however many logins it ran on, not {}: {frames:#?}",
            starts.len()
        ),
    }
}

/// The `message_end` of a turn that answered in full.
fn answered(frames: &[Value]) -> &Value {
    successful(frames);
    kinds(frames, MESSAGE_END)
        .into_iter()
        .next()
        .expect("an answered turn ends its reply")
}

/// The handover frames of a turn that handed over `count` times.
fn handed(frames: &[Value], count: usize) -> Vec<&Value> {
    let handovers = kinds(frames, HANDOVER);
    assert_eq!(
        handovers.len(),
        count,
        "the turn hands over {count} time(s): {frames:#?}"
    );
    handovers
}

/// What the turn failed with: its `error` frame, or the error its reply ended on.
fn failure(frames: &[Value]) -> String {
    frames
        .iter()
        .find_map(|frame| match frame["type"].as_str() {
            Some(ERROR) => frame["message"].as_str(),
            Some(MESSAGE_END) => frame["error"].as_str(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the turn fails: {frames:#?}"))
        .to_string()
}

fn assert_handover(frame: &Value, start: &Value, expected: &Expected<'_>) {
    assert_eq!(frame["message_id"], start["message_id"], "{frame}");
    assert_eq!(frame["from"], expected.from, "{frame}");
    assert_eq!(frame["to"], expected.to, "{frame}");
    assert_eq!(frame["from_agent"], expected.from_agent.as_str(), "{frame}");
    assert_eq!(frame["agent"], expected.agent.as_str(), "{frame}");
    assert_eq!(frame["reason"], expected.reason, "{frame}");
    assert_eq!(frame["carried"], expected.carried, "{frame}");
    assert_eq!(frame["at"], expected.at, "{frame}");
    match expected.resets {
        Reset::At(at) => assert_eq!(resets(frame), Some(at), "{frame}"),
        Reset::Unreported => assert_eq!(resets(frame), None, "{frame}"),
        Reset::Unchecked => {}
    }
}

fn resets(handover: &Value) -> Option<DateTime<Utc>> {
    handover[RESETS_AT].as_str().map(|at| {
        DateTime::parse_from_rfc3339(at)
            .unwrap_or_else(|error| panic!("resets_at {at} is a timestamp: {error}"))
            .with_timezone(&Utc)
    })
}

/// Asserts `metadata` records the turn's handover frames, in order.
fn assert_recorded(metadata: &Value, handovers: &[&Value]) {
    let recorded = metadata["handovers"]
        .as_array()
        .unwrap_or_else(|| panic!("the reply records its handovers: {metadata}"));
    assert_eq!(recorded.len(), handovers.len(), "{metadata}");
    for (record, frame) in recorded.iter().zip(handovers) {
        for field in HANDOVER_FIELDS {
            let same = match field {
                RESETS_AT => resets(record) == resets(frame),
                field => record[field] == frame[field],
            };
            assert!(same, "the record of {frame} differs in {field}: {record}");
        }
    }
}

#[tokio::test]
async fn a_turn_that_hits_its_limit_continues_on_another_login_in_the_same_message() {
    let harness = Harness::start(Sessions::Kept, Tools::Served).await;
    let alice = harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    let resets_at = reset_in(TimeDelta::hours(2));
    harness.claude.plan(
        ALICE,
        0,
        Run::Limited {
            partial: Some(PARTIAL),
            resets_at,
        },
    );
    harness.claude.plan(BOB, 0, Run::Answers(ANSWER));

    let frames = harness.turn(QUESTION).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: BOB.label,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Claude,
            reason: LIMIT,
            carried: true,
            at: PARTIAL.chars().count(),
            resets: Reset::At(resets_at),
        },
    );
    let end = answered(&frames);
    assert_eq!(end["message_id"], start["message_id"], "{end}");
    assert_eq!(
        end["content"],
        format!("{PARTIAL}{ANSWER}"),
        "the reply is what both logins streamed, under one message"
    );
    assert_recorded(&end["metadata"], &handovers);
    assert_recorded(&harness.stored(&start["message_id"]).await, &handovers);
    assert_eq!(
        end["metadata"]["usage"],
        json!({
            "prompt_tokens": PARTIAL_INPUT_TOKENS + ANSWER_INPUT_TOKENS,
            "completion_tokens": PARTIAL_OUTPUT_TOKENS + ANSWER_OUTPUT_TOKENS,
            "total_tokens": PARTIAL_INPUT_TOKENS + ANSWER_INPUT_TOKENS + PARTIAL_OUTPUT_TOKENS + ANSWER_OUTPUT_TOKENS,
        }),
        "the reply's usage sums what each login spent on it"
    );

    let runs = harness.claude.invocations();
    let [limited, carried] = runs.as_slice() else {
        panic!("the turn runs Alice, then Bob: {runs:#?}");
    };
    assert_eq!(limited.token, ALICE.token);
    assert_eq!(carried.token, BOB.token);
    let id = limited.pinned();
    assert_eq!(
        carried.after(RESUME),
        Some(id),
        "Bob resumes the session Alice started: {:?}",
        carried.arguments
    );
    assert!(!carried.has(SESSION_ID), "{:?}", carried.arguments);
    carried.assert_continues();
    assert_eq!(carried.home, harness.home(AgentKind::Claude, bob));
    assert!(
        harness.holds(bob, id),
        "the session file was carried into Bob's home"
    );
    assert!(
        !limited.tools.is_empty(),
        "the chat's turn is served zone's tools"
    );
    assert_eq!(
        carried.tools, limited.tools,
        "the handover keeps the turn's MCP lease"
    );
    assert_eq!(limited.directory, harness.work(AgentKind::Claude));
    assert_eq!(carried.directory, limited.directory);

    let session = harness.session().await;
    assert_eq!(session.login, Some(bob), "{session:?}");
    assert_eq!(session.agent, AgentKind::Claude, "{session:?}");
    assert_eq!(session.id, id, "{session:?}");
    assert_eq!(
        harness.login(alice).await.exhausted_until,
        Some(resets_at),
        "Alice rests until her window resets"
    );
    assert_eq!(harness.login(bob).await.exhausted_until, None);
}

#[tokio::test]
async fn a_turn_limited_before_it_writes_sends_its_prompt_again_on_the_carried_session() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    let resets_at = reset_in(TimeDelta::hours(2));
    harness.claude.plan(
        ALICE,
        0,
        Run::Limited {
            partial: None,
            resets_at,
        },
    );
    harness.claude.plan(BOB, 0, Run::Answers(ANSWER));

    let frames = harness.turn(QUESTION).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: BOB.label,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Claude,
            reason: LIMIT,
            carried: true,
            at: 0,
            resets: Reset::At(resets_at),
        },
    );
    let end = answered(&frames);
    assert_eq!(end["content"], ANSWER, "{end}");
    let runs = harness.claude.invocations();
    let [limited, rerun] = runs.as_slice() else {
        panic!("the turn runs Alice, then Bob: {runs:#?}");
    };
    assert_eq!(rerun.token, BOB.token);
    assert_eq!(
        rerun.after(RESUME),
        Some(limited.pinned()),
        "Bob resumes the session Alice started: {:?}",
        rerun.arguments
    );
    assert_eq!(
        rerun.prompt, limited.prompt,
        "a round that wrote nothing is sent again as it was"
    );
    assert_eq!(rerun.home, harness.home(AgentKind::Claude, bob));
}

#[tokio::test]
async fn a_handover_without_a_session_file_replays_the_transcript() {
    let harness = Harness::start(Sessions::Lost, Tools::Withheld).await;
    harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    let resets_at = reset_in(TimeDelta::hours(2));
    harness.claude.plan(
        ALICE,
        0,
        Run::Limited {
            partial: Some(PARTIAL),
            resets_at,
        },
    );
    harness.claude.plan(BOB, 0, Run::Answers(ANSWER));

    let frames = harness.turn(QUESTION).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: BOB.label,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Claude,
            reason: LIMIT,
            carried: false,
            at: PARTIAL.chars().count(),
            resets: Reset::At(resets_at),
        },
    );
    let end = answered(&frames);
    assert_eq!(end["content"], format!("{PARTIAL}{ANSWER}"), "{end}");
    assert_recorded(&end["metadata"], &handovers);

    let runs = harness.claude.invocations();
    let [limited, replayed] = runs.as_slice() else {
        panic!("the turn runs Alice, then Bob once: {runs:#?}");
    };
    assert_eq!(replayed.token, BOB.token);
    assert!(
        !replayed.has(RESUME),
        "a session with no file to carry is not resumed: {:?}",
        replayed.arguments
    );
    let fresh = replayed.pinned();
    assert_ne!(fresh, limited.pinned(), "the replay runs in a new session");
    replayed.assert_replays_with_the_partial();

    let session = harness.session().await;
    assert_eq!(session.login, Some(bob), "{session:?}");
    assert_eq!(session.id, fresh, "{session:?}");
    assert!(
        session
            .prompt
            .as_deref()
            .is_some_and(|hash| hash.len() == 64
                && hash.chars().all(|character| character.is_ascii_hexdigit())),
        "the replayed session remembers the prompt it saw: {session:?}"
    );
}

#[tokio::test]
async fn a_chat_whose_agent_is_spent_continues_on_the_other_agent() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    harness.sign_in(ALICE).await;
    let carol = harness.sign_in_codex(CAROL).await;
    let resets_at = reset_in(TimeDelta::hours(2));
    harness.claude.plan(
        ALICE,
        0,
        Run::Limited {
            partial: Some(PARTIAL),
            resets_at,
        },
    );

    let frames = harness.turn(QUESTION).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: CAROL,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Codex,
            reason: LIMIT,
            carried: false,
            at: PARTIAL.chars().count(),
            resets: Reset::At(resets_at),
        },
    );
    let end = answered(&frames);
    assert_eq!(end["content"], format!("{PARTIAL}{CODEX_ANSWER}"), "{end}");

    assert_eq!(
        harness.claude.invocations().len(),
        1,
        "Claude has no other login to run on"
    );
    let runs = harness.codex.invocations();
    let [moved] = runs.as_slice() else {
        panic!("the turn runs codex once: {runs:#?}");
    };
    assert!(
        !moved.has(CODEX_RESUME),
        "a Claude session is not one codex can resume: {:?}",
        moved.arguments
    );
    moved.assert_replays_with_the_partial();
    assert_eq!(
        moved.directory,
        harness.work(AgentKind::Codex),
        "codex runs in its own working directory, not Claude's"
    );
    assert_eq!(moved.home, harness.home(AgentKind::Codex, carol));
    assert!(
        moved
            .after(MODEL)
            .is_none_or(|model| AgentKind::Codex.knows(model)),
        "the model is picked again for codex: {:?}",
        moved.arguments
    );

    let session = harness.session().await;
    assert_eq!(session.agent, AgentKind::Codex, "{session:?}");
    assert_eq!(session.login, Some(carol), "{session:?}");
    assert_eq!(session.id, harness.thread, "{session:?}");
}

#[tokio::test]
async fn a_turn_with_no_login_left_fails_naming_the_earliest_reset() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    let alice = harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    let earliest = reset_in(TimeDelta::hours(2));
    let later = reset_in(TimeDelta::hours(3));
    harness.claude.plan(
        ALICE,
        0,
        Run::Limited {
            partial: Some(PARTIAL),
            resets_at: earliest,
        },
    );
    harness.claude.plan(
        BOB,
        0,
        Run::Limited {
            partial: None,
            resets_at: later,
        },
    );

    let frames = harness.turn(QUESTION).await;

    started(&frames);
    handed(&frames, 1);
    let failure = failure(&frames);
    let expected = backend::Error::Limited {
        agent: AgentKind::Claude,
        resets_at: Some(earliest),
    }
    .to_string();
    assert!(
        failure.contains(EXHAUSTED) && failure.contains(&expected),
        "the turn names the earliest reset among the logins it tried, {expected:?}: {failure}"
    );
    let runs = harness.claude.invocations();
    let tokens: Vec<&str> = runs.iter().map(|run| run.token.as_str()).collect();
    assert_eq!(tokens, [ALICE.token, BOB.token], "{runs:#?}");
    assert_eq!(harness.login(alice).await.exhausted_until, Some(earliest));
    assert_eq!(harness.login(bob).await.exhausted_until, Some(later));
}

/// Alice has the more headroom of the two once her window resets, so a turn
/// that picked afresh would start on her; the chat stays on Bob instead.
#[tokio::test]
async fn the_next_turn_stays_on_the_login_the_last_one_ended_on() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    let alice = harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    harness.claude.plan(
        ALICE,
        0,
        Run::Limited {
            partial: Some(PARTIAL),
            resets_at: reset_in(TimeDelta::hours(2)),
        },
    );
    harness.claude.plan(BOB, 0, Run::Answers(ANSWER));
    let frames = harness.turn(QUESTION).await;
    handed(&frames, 1);
    answered(&frames);
    let carried = harness.session().await;
    assert_eq!(carried.login, Some(bob), "{carried:?}");
    harness.refresh(alice).await;

    let frames = harness.turn(FOLLOW_UP).await;

    handed(&frames, 0);
    let end = answered(&frames);
    assert_eq!(end["content"], REPLY, "{end}");
    let runs = harness.claude.invocations();
    let [_, _, stayed] = runs.as_slice() else {
        panic!("the second turn runs once: {runs:#?}");
    };
    assert_eq!(stayed.token, BOB.token, "the chat stays on Bob");
    assert_eq!(
        stayed.after(RESUME),
        Some(carried.id.as_str()),
        "{:?}",
        stayed.arguments
    );
    stayed.assert_resumes_with(FOLLOW_UP);
    let session = harness.session().await;
    assert_eq!(session.login, Some(bob), "{session:?}");
    assert_eq!(session.id, carried.id, "{session:?}");
}

#[tokio::test]
async fn a_sticky_login_exhausted_elsewhere_is_left_before_the_turn_spawns() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    let alice = harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    answered(&harness.turn(QUESTION).await);
    let pinned = harness.session().await;
    assert_eq!(pinned.login, Some(alice), "{pinned:?}");
    agent_logins::exhaust(&harness.pool, alice, reset_in(TimeDelta::hours(5)))
        .await
        .expect("Alice to be exhausted by another chat");

    let frames = harness.turn(FOLLOW_UP).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: BOB.label,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Claude,
            reason: LIMIT,
            carried: true,
            at: 0,
            resets: Reset::Unchecked,
        },
    );
    let end = answered(&frames);
    assert_eq!(end["content"], REPLY, "{end}");
    let runs = harness.claude.invocations();
    let [first, moved] = runs.as_slice() else {
        panic!("the exhausted login is never spawned for the second turn: {runs:#?}");
    };
    assert_eq!(first.token, ALICE.token);
    assert_eq!(moved.token, BOB.token);
    assert_eq!(
        moved.after(RESUME),
        Some(pinned.id.as_str()),
        "{:?}",
        moved.arguments
    );
    moved.assert_resumes_with(FOLLOW_UP);
    assert!(harness.holds(bob, &pinned.id));
    let session = harness.session().await;
    assert_eq!(session.login, Some(bob), "{session:?}");
    assert_eq!(session.id, pinned.id, "{session:?}");
}

/// A credits limit leaves Alice a candidate for other chats, so only the
/// turn's own record of the logins it tried keeps it from going back to her.
#[tokio::test]
async fn a_handover_never_runs_the_same_login_twice_in_one_turn() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    let alice = harness.sign_in(ALICE).await;
    harness.sign_in(BOB).await;
    harness.claude.plan(
        ALICE,
        0,
        Run::Unfunded {
            partial: Some(PARTIAL),
        },
    );
    harness.claude.plan(
        BOB,
        0,
        Run::Limited {
            partial: None,
            resets_at: reset_in(TimeDelta::hours(2)),
        },
    );

    let frames = harness.turn(QUESTION).await;

    started(&frames);
    handed(&frames, 1);
    failure(&frames);
    let runs = harness.claude.invocations();
    let tokens: Vec<&str> = runs.iter().map(|run| run.token.as_str()).collect();
    assert_eq!(
        tokens,
        [ALICE.token, BOB.token],
        "each login runs once: {runs:#?}"
    );
    assert_eq!(
        harness.login(alice).await.exhausted_until,
        None,
        "a credits limit never exhausts the login"
    );
}

#[tokio::test]
async fn a_credits_limit_hands_over_and_ends_with_the_credits_wording_when_none_remains() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    let alice = harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    harness.claude.plan(
        ALICE,
        0,
        Run::Unfunded {
            partial: Some(PARTIAL),
        },
    );
    harness.claude.plan(BOB, 0, Run::Unfunded { partial: None });

    let frames = harness.turn(QUESTION).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: BOB.label,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Claude,
            reason: CREDITS,
            carried: true,
            at: PARTIAL.chars().count(),
            resets: Reset::Unreported,
        },
    );
    let failure = failure(&frames);
    assert!(
        failure.contains(UNFUNDED),
        "the turn ends in the credits wording: {failure}"
    );
    let runs = harness.claude.invocations();
    let tokens: Vec<&str> = runs.iter().map(|run| run.token.as_str()).collect();
    assert_eq!(tokens, [ALICE.token, BOB.token], "{runs:#?}");
    assert_eq!(harness.login(alice).await.exhausted_until, None);
    assert_eq!(harness.login(bob).await.exhausted_until, None);
}

#[tokio::test]
async fn a_signed_out_login_hands_over_and_says_so() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    harness.sign_in(ALICE).await;
    let bob = harness.sign_in(BOB).await;
    harness.claude.plan(
        ALICE,
        0,
        Run::SignedOut {
            partial: Some(PARTIAL),
        },
    );
    harness.claude.plan(BOB, 0, Run::Answers(ANSWER));

    let frames = harness.turn(QUESTION).await;

    let start = started(&frames);
    let handovers = handed(&frames, 1);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: ALICE.label,
            to: BOB.label,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Claude,
            reason: SIGNED_OUT,
            carried: true,
            at: PARTIAL.chars().count(),
            resets: Reset::Unreported,
        },
    );
    let end = answered(&frames);
    assert_eq!(end["content"], format!("{PARTIAL}{ANSWER}"), "{end}");
    assert_recorded(&end["metadata"], &handovers);
    let runs = harness.claude.invocations();
    let [_, carried] = runs.as_slice() else {
        panic!("the turn runs Alice, then Bob: {runs:#?}");
    };
    assert_eq!(carried.token, BOB.token);
    carried.assert_continues();
    let session = harness.session().await;
    assert_eq!(session.login, Some(bob), "{session:?}");
}

/// Alice refused the chat's model for want of usage credits, so its turns run
/// on Carol until that refusal cools down. Then the chat goes back to Alice,
/// and a limit she hits partway through hands the turn back to Carol.
#[tokio::test]
async fn a_chat_stays_off_a_login_out_of_credits_for_its_model_until_the_cool_down_passes() {
    let harness = Harness::start(Sessions::Kept, Tools::Withheld).await;
    let alice = harness.sign_in(ALICE).await;
    let carol = harness.sign_in_codex(CAROL).await;
    harness.claude.plan(
        ALICE,
        0,
        Run::Unfunded {
            partial: Some(PARTIAL),
        },
    );
    let frames = harness.turn(QUESTION).await;
    let handovers = handed(&frames, 1);
    assert_eq!(handovers[0]["reason"], CREDITS, "{}", handovers[0]);
    answered(&frames);
    assert_eq!(harness.session().await.login, Some(carol));

    let frames = harness.turn(FOLLOW_UP).await;

    handed(&frames, 0);
    let end = answered(&frames);
    assert_eq!(end["content"], CODEX_ANSWER, "{end}");
    let runs = harness.claude.invocations();
    let [refused] = runs.as_slice() else {
        panic!("Alice is not run again while her refusal cools down: {runs:#?}");
    };
    assert_eq!(harness.codex.invocations().len(), 2);
    assert_eq!(harness.session().await.login, Some(carol));
    assert_eq!(harness.login(alice).await.exhausted_until, None);

    let model = refused
        .after(MODEL)
        .unwrap_or_else(|| panic!("Alice ran a model: {:?}", refused.arguments));
    unfunded::record(
        alice,
        model,
        Utc::now() - unfunded::COOL_DOWN - TimeDelta::seconds(1),
    );
    let resets_at = reset_in(TimeDelta::hours(2));
    harness.claude.plan(
        ALICE,
        1,
        Run::Limited {
            partial: Some(PARTIAL),
            resets_at,
        },
    );

    let frames = harness.turn("And neap tides?").await;

    let start = started(&frames);
    let handovers = handed(&frames, 2);
    assert_handover(
        handovers[0],
        start,
        &Expected {
            from: CAROL,
            to: ALICE.label,
            from_agent: AgentKind::Codex,
            agent: AgentKind::Claude,
            reason: CONFIGURED,
            carried: false,
            at: 0,
            resets: Reset::Unreported,
        },
    );
    assert_handover(
        handovers[1],
        start,
        &Expected {
            from: ALICE.label,
            to: CAROL,
            from_agent: AgentKind::Claude,
            agent: AgentKind::Codex,
            reason: LIMIT,
            carried: false,
            at: PARTIAL.chars().count(),
            resets: Reset::At(resets_at),
        },
    );
    let end = answered(&frames);
    assert_eq!(end["content"], format!("{PARTIAL}{CODEX_ANSWER}"), "{end}");
    let runs = harness.claude.invocations();
    let tokens: Vec<&str> = runs.iter().map(|run| run.token.as_str()).collect();
    assert_eq!(tokens, [ALICE.token, ALICE.token], "{runs:#?}");
    assert_eq!(harness.codex.invocations().len(), 3);
    assert_eq!(harness.session().await.login, Some(carol));
}
