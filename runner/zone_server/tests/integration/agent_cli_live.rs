//! The real claude and codex, run through abnegate-agent-cli exactly as Zone wires a chat turn:
//! the backend from `services::backend`, zone's tools from a `mcp::Turn` lease, and the session
//! moved from one home to another the way a handover moves it.
//!
//! Each test prepares temporary homes and runs a copy of this binary in a cleared environment that
//! holds only the [`KEPT`] variables the parent has set, the run's own variables and the
//! temporary homes' paths, so no agent finds the host's `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, a
//! sign-in token or any other variable of the host's, and nothing calls `set_var`. `HOME` stays
//! the host's, since claude finds the login Keychain through it. Every agent runs through a
//! wrapper that records its process id, the names of the variables it was given and the home it
//! ran in.
//!
//! Ignored because each spends real turns of the host user's subscriptions, and because two of
//! them share the host's own sign-in:
//!
//! - The claude test runs with `CLAUDE_SECURESTORAGE_CONFIG_DIR` empty, so claude reads its
//!   sign-in from the host's own unsuffixed Keychain item and writes back to it: should claude
//!   refresh its OAuth token during the run, the rotated token replaces the host's, and a host
//!   claude refreshing at the same moment can lose that race and be signed out. It skips when
//!   `claude auth status` reports the item signed out.
//! - The codex test copies `~/.codex/auth.json` and skips when its last refresh is a week old,
//!   since a refresh inside the copy would rotate the token and sign the host's own codex out. It
//!   fails when the host's `auth.json` changed during the run.
//! - The claude organization login test runs only when `CLAUDE_CODE_OAUTH_TOKEN` holds a token
//!   from `claude setup-token`, and only that test's copy is handed the token.
//!
//! A test that skips passes, and says so with a `SKIPPED:` line on standard error that the test
//! harness does not capture. Run by hand, from `runner/`, against a migrated database:
//!
//! `TEST_DATABASE_URL=<url> DATABASE_URL=<url> REDIS_URL=<url> cargo test -p zone_server
//! --no-default-features --features zone_context/test-utils --test integration -- --ignored
//! --nocapture --test-threads=1 agent_cli_live::`

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use abnegate_agent_cli::login::codex::CREDENTIALS;
use abnegate_agent_cli::login::{Cli, Status};
use abnegate_agent_cli::{AgentKind, Occupancy, Session};
use abnegate_llm::{LlmConfig, Message, ReasoningEffort};
use abnegate_secret::SecretValue;
use chrono::{DateTime, TimeDelta, Utc};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use uuid::Uuid;
use zone_core::llm::{BuiltinTools, LlmBackend, LlmClient, SignIn, Toolset, cli};

use zone_server::agent::{AgentEvent, ApprovalPolicy, ChatTools, LoopBudget, WorkspaceScope};
use zone_server::config::{Config, ModelBackend};
use zone_server::db::agent_logins::{self, AgentLoginRow, Insert};
use zone_server::db::chats::ChatSession;
use zone_server::db::{ai_settings, workspaces};
use zone_server::mcp::{Lease, Scope, Turn, endpoint};
use zone_server::services::agent::home::{self, CLAUDE_CODE_OAUTH_TOKEN};
use zone_server::services::backend::{self, Routing};
use zone_server::services::chat::handover;
use zone_server::services::endpoint::Origin;
use zone_server::services::login::credential::Login;
use zone_server::services::login::router::Chosen;
use zone_server::state::AppState;

use crate::common::context::Harness;

/// Set on the copy of this binary that runs the lanes.
const COPY: &str = "ZONE_LIVE_COPY";
/// The run directory the parent prepared for the copy.
const RUN: &str = "ZONE_LIVE_RUN";
/// Where run directories are made, the system's temporary directory when unset.
const ROOT: &str = "ZONE_LIVE_ROOT";
/// Where zone's own tools run, so `list_files` lists the probe directory.
const TOOLS_DIRECTORY: &str = "ZONE_CHAT_AGENT_CWD";
/// Which Keychain item claude reads its sign-in from: empty is the unsuffixed one.
const SECURE_STORAGE: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";
/// What every agent's own home is named by, none of which a copy may inherit.
const HOST_HOMES: &[&str] = &["CLAUDE_CONFIG_DIR", SECURE_STORAGE, "CODEX_HOME"];
/// All a copy is given of the parent's environment, each when the parent has it set.
const KEPT: &[&str] = &[
    "PATH",
    "HOME",
    "TMPDIR",
    "USER",
    "LOGNAME",
    "SHELL",
    "TEST_DATABASE_URL",
    "ZONE_CONTEXT_TEST_DATABASE_URL",
    "DATABASE_URL",
    "REDIS_URL",
    "RUST_LOG",
    "RUST_BACKTRACE",
];

const CLAUDE_MODEL: &str = "haiku";
const CODEX_MODEL: &str = "gpt-6-luna";
const CODEX_EFFORT: ReasoningEffort = ReasoningEffort::Low;

const CLAUDE_STATE: &str = ".claude.json";
const CLAUDE_PROJECTS: &str = "projects";
const CODEX_SESSIONS: &str = "sessions";
const HOST_CLAUDE_HOME: &str = ".claude";
const HOST_CODEX_HOME: &str = ".codex";
const STALE_AFTER: TimeDelta = TimeDelta::days(7);
const LAST_REFRESH: &str = "last_refresh";

const TURN_TIMEOUT: Duration = Duration::from_secs(240);
const STOP_WITHIN: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(100);
const EXECUTABLE: u32 = 0o755;

const WRAPPERS: &str = "bin";
const PIDFILE: &str = "pid";
const NAMES: &str = "names";
const HOMES: &str = "homes";
const HOME_A: &str = "a";
const HOME_B: &str = "b";
const AUTH: &str = "auth.json";
const AGENTS: &str = "agents";
const PROBE: &str = "probe";
const PROJECT: &str = "project";
const SESSIONS: &str = "sessions";
const SKIPPED: &str = "skipped";

const PROBED_TOOL: &str = "list_files";
const RECALL: &str = "What was the word I asked you to remember? Reply with only that word.";
const COUNT: &str = "Count from 1 to 300, one number per line, and nothing else.";
const UNREACHABLE: &str = "Reply with only the word OK.";

type Console = tokio::sync::mpsc::UnboundedReceiver<AgentEvent>;

/// The directory a parent prepares for one copy, and what the agent's wrapper wrote there.
struct Run {
    root: PathBuf,
    agent: AgentKind,
}

impl Run {
    fn from_environment(agent: AgentKind) -> Self {
        let root = env::var_os(RUN).expect("the parent names the run directory");
        Self {
            root: PathBuf::from(root),
            agent,
        }
    }

    fn wrapper(&self) -> PathBuf {
        self.root.join(WRAPPERS).join(self.agent.executable())
    }

    fn home(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn probe(&self) -> PathBuf {
        self.root.join(PROBE)
    }

    fn forget_pid(&self) {
        let _ = fs::remove_file(self.root.join(PIDFILE));
    }

    fn pid(&self) -> u32 {
        fs::read_to_string(self.root.join(PIDFILE))
            .expect("the wrapper wrote the agent's process id")
            .trim()
            .parse()
            .expect("a process id")
    }

    /// The names of the variables the agent last started with, never their values.
    fn names(&self) -> BTreeSet<String> {
        fs::read_to_string(self.root.join(NAMES))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The home the agent last started in.
    fn last_home(&self) -> PathBuf {
        let homes = fs::read_to_string(self.root.join(HOMES)).expect("the wrapper wrote homes");
        PathBuf::from(homes.lines().last().expect("the agent ran at least once"))
    }

    fn record(&self, name: &str, value: &str) {
        fs::write(self.root.join(name), value).expect("a record for the parent");
    }

    /// Tell the parent the lanes were skipped and why, for it to report past the capture.
    fn skip(&self, reason: &str) {
        println!("SKIPPED: {reason}");
        self.record(SKIPPED, reason);
    }
}

/// What one turn produced: its text, the session it announced, zone's tools it called, the
/// context it reported, whether it reported usage, and the error it ended in.
#[derive(Default)]
struct Answer {
    text: String,
    session: Option<String>,
    called: Vec<String>,
    occupancy: Option<Occupancy>,
    used: bool,
    error: Option<String>,
}

impl Answer {
    fn finished(self, lane: &str) -> Self {
        if let Some(error) = &self.error {
            panic!("{lane} failed: {error}; it wrote {:?}", self.text);
        }
        self
    }
}

/// The entries the host's own agent homes hold, by name only.
struct Listing {
    claude: BTreeSet<PathBuf>,
    codex: BTreeSet<PathBuf>,
}

impl Listing {
    fn take() -> Self {
        let home = host_home();
        let configured = home::variable(AgentKind::Claude)
            .and_then(env::var_os)
            .filter(|directory| !directory.is_empty())
            .map(PathBuf::from);
        let claude = [Some(home.join(HOST_CLAUDE_HOME)), configured]
            .into_iter()
            .flatten()
            .collect::<BTreeSet<PathBuf>>()
            .into_iter()
            .flat_map(|directory| entries(&directory.join(CLAUDE_PROJECTS)))
            .collect();
        let codex = files(&home.join(HOST_CODEX_HOME).join(CODEX_SESSIONS));
        Self { claude, codex }
    }

    /// Assert the run left nothing of its own in the host's homes, and print how much they grew
    /// meanwhile, which other sessions on the host account for.
    fn assert_untouched_by(&self, before: &Self, run: &Path) {
        let project = fs::read_to_string(run.join(PROJECT)).unwrap_or_default();
        let sessions = fs::read_to_string(run.join(SESSIONS)).unwrap_or_default();
        println!(
            "host listings: claude projects {} before, {} after; codex sessions {} before, {} after",
            before.claude.len(),
            self.claude.len(),
            before.codex.len(),
            self.codex.len()
        );
        let project = project.trim();
        if !project.is_empty() {
            let leaked: Vec<&PathBuf> = self
                .claude
                .iter()
                .filter(|entry| entry.file_name().is_some_and(|name| name == project))
                .collect();
            assert!(
                leaked.is_empty(),
                "the run wrote its project into the host's claude: {leaked:?}"
            );
            println!("no host claude projects entry is named {project}");
        }
        for session in sessions
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let leaked: Vec<&PathBuf> = self
                .claude
                .iter()
                .chain(&self.codex)
                .filter(|entry| entry.to_string_lossy().contains(session))
                .collect();
            assert!(
                leaked.is_empty(),
                "session {session} reached the host's own homes: {leaked:?}"
            );
            println!("no host claude project or codex session names {session}");
        }
    }
}

fn host_home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").expect("HOME is set"))
}

fn codex_credentials() -> PathBuf {
    host_home().join(HOST_CODEX_HOME).join(CREDENTIALS)
}

/// The SHA-256 of the host's own codex sign-in, or `None` when it cannot be read.
fn codex_fingerprint() -> Option<Vec<u8>> {
    fs::read(codex_credentials())
        .ok()
        .map(|credentials| Sha256::digest(credentials).to_vec())
}

/// Say a test skipped on the process's real standard error, which the test harness does not
/// capture, so a skip that passes is never mistaken for a run.
fn skipped(reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {reason}");
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime for the copy's lanes")
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

fn files(directory: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    for path in entries(directory) {
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.insert(path);
        }
    }
    found
}

/// Where `executable` is first found on this process's `PATH`.
fn located(executable: &str) -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{executable} is not on PATH"))
}

/// A wrapper that records the agent's process id, the names of its variables and its home in
/// `run`, then becomes the agent.
fn wrap(run: &Path, agent: AgentKind) {
    let directory = run.join(WRAPPERS);
    fs::create_dir_all(&directory).expect("a directory for the wrapper");
    let wrapper = directory.join(agent.executable());
    let home = home::variable(agent).expect("the agent's home variable");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n\
             echo $$ > '{pid}'\n\
             /usr/bin/perl -e 'print map {{ \"$_\\n\" }} sort keys %ENV' > '{names}'\n\
             printf '%s\\n' \"${{{home}-}}\" >> '{homes}'\n\
             exec '{real}' \"$@\"\n",
            pid = run.join(PIDFILE).display(),
            names = run.join(NAMES).display(),
            homes = run.join(HOMES).display(),
            real = located(agent.executable()).display(),
        ),
    )
    .expect("the wrapper");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(EXECUTABLE))
        .expect("an executable wrapper");
}

/// Why codex's own sign-in cannot be copied safely, when it cannot: it was refreshed too long
/// ago. Reads nothing of `auth.json` but its last refresh.
fn codex_refresh_is_stale() -> Option<String> {
    let path = codex_credentials();
    let Ok(text) = fs::read_to_string(&path) else {
        return Some(format!("{} cannot be read", path.display()));
    };
    let refreshed = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|credentials| credentials.get(LAST_REFRESH)?.as_str().map(str::to_string))
        .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| at.with_timezone(&Utc));
    let Some(refreshed) = refreshed else {
        return Some(format!(
            "{} holds no readable {LAST_REFRESH}",
            path.display()
        ));
    };
    let age = Utc::now() - refreshed;
    println!(
        "codex last refreshed its sign-in at {refreshed}, {} hours ago",
        age.num_hours()
    );
    (age >= STALE_AFTER).then(|| {
        format!(
            "codex last refreshed its sign-in at {refreshed}, {} days ago; a refresh inside the copy \
             would rotate the token and sign the host's codex out",
            age.num_days()
        )
    })
}

/// Prepare a run for `agent` and run `test` in a copy of this binary given only the [`KEPT`]
/// variables and its temporary homes, then assert the host's own homes hold nothing of it and,
/// for codex, that the host's own sign-in is unchanged.
fn parent(agent: AgentKind, sign_in: SignIn, test: &str) {
    let before = Listing::take();
    let unready = match (agent, sign_in) {
        (AgentKind::Codex, _) => codex_refresh_is_stale(),
        (AgentKind::Claude, SignIn::Organization) => env::var(CLAUDE_CODE_OAUTH_TOKEN)
            .ok()
            .filter(|token| !token.trim().is_empty())
            .is_none()
            .then(|| format!("{CLAUDE_CODE_OAUTH_TOKEN} is not set")),
        _ => None,
    };
    if let Some(reason) = unready {
        skipped(&reason);
        return;
    }
    let codex_before = (agent == AgentKind::Codex).then(codex_fingerprint);
    let root = env::var_os(ROOT).map_or_else(env::temp_dir, PathBuf::from);
    fs::create_dir_all(&root).expect("the run root");
    let run = TempDir::new_in(&root).expect("a run directory");
    wrap(run.path(), agent);
    for directory in [HOME_A, HOME_B, AGENTS, PROBE] {
        fs::create_dir_all(run.path().join(directory)).expect("a run subdirectory");
    }

    let mut command = Command::new(env::current_exe().expect("this test binary"));
    command.env_clear().args([
        "--exact",
        test,
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    for name in KEPT {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env(COPY, "1")
        .env(RUN, run.path())
        .env(TOOLS_DIRECTORY, run.path().join(PROBE));
    match (agent, sign_in) {
        (AgentKind::Claude, SignIn::Host) => {
            let home = host_home();
            let state = [
                home.join(CLAUDE_STATE),
                home.join(HOST_CLAUDE_HOME).join(CLAUDE_STATE),
            ]
            .into_iter()
            .find(|candidate| candidate.is_file())
            .expect("the host's claude state file");
            for directory in [HOME_A, HOME_B] {
                fs::copy(&state, run.path().join(directory).join(CLAUDE_STATE))
                    .expect("a copy of claude's state file");
            }
            let configuration = home::variable(AgentKind::Claude).expect("claude's home variable");
            command
                .env(configuration, run.path().join(HOME_A))
                .env(SECURE_STORAGE, "")
                .env(
                    cli::PASSTHROUGH,
                    format!("{configuration},{SECURE_STORAGE}"),
                );
        }
        (AgentKind::Claude, SignIn::Organization) => {
            command.env(
                CLAUDE_CODE_OAUTH_TOKEN,
                env::var_os(CLAUDE_CODE_OAUTH_TOKEN).expect("the token was checked above"),
            );
        }
        (AgentKind::Codex, _) => {
            fs::copy(codex_credentials(), run.path().join(AUTH))
                .expect("a copy of codex's sign-in");
        }
        _ => {}
    }

    let output = command.output().expect("a copy of this test binary");
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{printed}");
    if let Some(before) = codex_before {
        assert!(
            codex_fingerprint() == before,
            "{} changed while the codex lanes ran: the host user's codex login may have been \
             rotated and signed out; run `codex login` if the host's codex no longer works",
            codex_credentials().display()
        );
    }
    Listing::take().assert_untouched_by(&before, run.path());

    assert!(output.status.success(), "the {agent} lanes failed");
    assert!(
        printed.contains("1 passed"),
        "the copy ran no test: {printed}"
    );
    if let Ok(reason) = fs::read_to_string(run.path().join(SKIPPED)) {
        skipped(reason.trim());
    }
}

/// The harness's state with the agent run through the wrapper, the host's sign-in allowed, and
/// every organization's homes under the run.
fn configured(run: &Run, harness: &Harness) -> AppState {
    let mut config: Config = harness.config.clone();
    config.agents.state = run.root.join(AGENTS);
    config.agents.host_login = true;
    config.model_backend = ModelBackend::Cli {
        agent: run.agent,
        executable: Some(run.wrapper()),
    };
    let state = AppState::new(config, harness.pool.clone(), None);
    state.disable_mcp();
    state
}

async fn organization(harness: &Harness) -> Uuid {
    workspaces::get_workspace(&harness.pool, harness.workspace)
        .await
        .expect("the workspace to be readable")
        .expect("the harness's workspace")
        .organization_id
}

/// A turn of the harness's chat whose tools are served at `address`, approving every call.
async fn leased(harness: &Harness, state: &AppState, address: &str) -> (Lease, Console) {
    let tools = Arc::new(
        ChatTools::build(WorkspaceScope {
            state: state.clone(),
            workspace_id: harness.workspace,
            chat_id: Some(harness.chat),
            user_id: Uuid::new_v4(),
            offline: true,
            origin: zone_server::db::tasks::Origin::Member,
        })
        .await,
    );
    let (events, console) = tokio::sync::mpsc::unbounded_channel();
    let turn = Turn::new(
        Scope {
            workspace: harness.workspace,
            chat: harness.chat,
            user: Some(Uuid::new_v4()),
            approval: ApprovalPolicy::auto(),
            calls: LoopBudget::chat().max_tool_calls,
            builtin: BuiltinTools::Withheld,
        },
        tools,
        events,
    );
    (turn.open(endpoint(&format!("http://{address}"))), console)
}

fn client(model: &str, backend: LlmBackend, lease: &Lease) -> LlmClient {
    let mut config = LlmConfig::default();
    config.default_model = model.to_string();
    LlmClient::new(config)
        .with_backend(backend)
        .with_toolset(lease.toolset(), BuiltinTools::Withheld)
}

/// One whole turn of `prompt` on `client`, and what zone's console saw of it.
async fn converse(client: &LlmClient, mut console: Console, prompt: &str) -> Answer {
    let mut answer = Answer::default();
    let deadline = Instant::now() + TURN_TIMEOUT;
    match tokio::time::timeout(
        TURN_TIMEOUT,
        client.chat_stream(&[Message::user(prompt)], None),
    )
    .await
    {
        Err(_) => answer.error = Some("the agent did not start in time".to_string()),
        Ok(Err(error)) => answer.error = Some(error.to_string()),
        Ok(Ok(mut stream)) => loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left, stream.next()).await {
                Err(_) => {
                    answer.error = Some("the agent did not finish in time".to_string());
                    break;
                }
                Ok(None) => break,
                Ok(Some(Err(error))) => {
                    answer.error = Some(error.to_string());
                    break;
                }
                Ok(Some(Ok(chunk))) => {
                    answer.used |= chunk.usage.is_some();
                    if let Some(session) = chunk.session {
                        answer.session = Some(session);
                    }
                    for choice in chunk.choices {
                        if let Some(content) = choice.delta.content {
                            answer.text.push_str(&content);
                        }
                    }
                }
            }
        },
    }
    answer.occupancy = client.occupancy();
    while let Ok(event) = console.try_recv() {
        if let AgentEvent::ToolCallStarted { name, .. } = event {
            answer.called.push(name);
        }
    }
    answer
}

/// Turn 3: the turn dropped after its first event, and the agent's whole process group gone
/// within [`STOP_WITHIN`].
async fn stop(run: &Run, client: &LlmClient) {
    run.forget_pid();
    let mut stream = tokio::time::timeout(
        TURN_TIMEOUT,
        client.chat_stream(&[Message::user(COUNT)], None),
    )
    .await
    .expect("the agent starts in time")
    .expect("the agent starts");
    tokio::time::timeout(TURN_TIMEOUT, stream.next())
        .await
        .expect("a first event in time")
        .expect("a first event")
        .expect("the first event is not a failure");
    let pid = run.pid();
    drop(stream);

    let started = Instant::now();
    while started.elapsed() < STOP_WITHIN {
        if !alive(pid) && !grouped(pid) {
            println!(
                "turn 3: {} {pid} and its process group were gone {:?} after the stream was dropped",
                run.agent,
                started.elapsed()
            );
            return;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!(
        "{} {pid} outlived its dropped stream by {STOP_WITHIN:?}: alive {}, group {}",
        run.agent,
        alive(pid),
        grouped(pid)
    );
}

fn alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn grouped(pid: u32) -> bool {
    Command::new("/usr/bin/pgrep")
        .args(["-g", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn nonce() -> String {
    format!("zephyr{}", &Uuid::new_v4().simple().to_string()[..8])
}

/// Turn 1's prompt: call zone's tool once, report what it listed, and remember `word`.
fn probing(agent: AgentKind, word: &str) -> String {
    format!(
        "Remember the word {word}. Call the {} tool exactly once, with path \".\", then reply \
         with one line: the name of the one file it listed, then the word {word}. Do nothing else.",
        agent.tool(Toolset::SERVER, PROBED_TOOL)
    )
}

/// Turn 1, in the first home: zone's tool called once, the answer carrying `word` and what the
/// tool listed, a session announced and the context recorded. Returns the session.
async fn first(
    run: &Run,
    harness: &Harness,
    state: &AppState,
    model: &str,
    backend: LlmBackend,
    word: &str,
) -> String {
    let marker = nonce();
    fs::write(run.probe().join(format!("{marker}.txt")), "").expect("the probe file");
    let (lease, console) = leased(harness, state, &harness.address).await;
    assert!(
        lease.toolset().tools.iter().any(|tool| tool == PROBED_TOOL),
        "zone does not serve {PROBED_TOOL}: {:?}",
        lease.toolset().tools
    );
    let answer = converse(
        &client(model, backend, &lease),
        console,
        &probing(run.agent, word),
    )
    .await
    .finished("turn 1");
    println!(
        "turn 1: {:?}, called {:?}, session {:?}, occupancy {:?}",
        answer.text, answer.called, answer.session, answer.occupancy
    );
    if let Some(session) = &answer.session {
        run.record(SESSIONS, session);
    }

    assert!(
        answer.called.iter().any(|name| name == PROBED_TOOL),
        "zone's console saw no {PROBED_TOOL} call: {:?}",
        answer.called
    );
    assert!(
        answer.text.contains(word),
        "turn 1 lost the nonce: {:?}",
        answer.text
    );
    assert!(
        answer.text.contains(&marker),
        "turn 1 did not report what zone's tool listed: {:?}",
        answer.text
    );
    assert!(answer.occupancy.is_some(), "turn 1 recorded no occupancy");
    answer.session.expect("turn 1 announced its session")
}

/// Turns 2 to 4, resumed in `second`: the nonce recalled, the turn stopped after its first
/// event, and the nonce recalled again after the stop.
async fn resumed(
    run: &Run,
    harness: &Harness,
    state: &AppState,
    model: &str,
    backend: &LlmBackend,
    word: &str,
    second: &Path,
) {
    let (recall, console) = leased(harness, state, &harness.address).await;
    let answer = converse(&client(model, backend.clone(), &recall), console, RECALL)
        .await
        .finished("turn 2");
    println!("turn 2: {:?}", answer.text);
    assert_eq!(
        run.last_home(),
        second,
        "turn 2 ran outside the second home"
    );
    assert!(
        answer.text.contains(word),
        "turn 2 resumed in the second home without turn 1: {:?}",
        answer.text
    );
    drop(recall);

    let (stopping, _console) = leased(harness, state, &harness.address).await;
    stop(run, &client(model, backend.clone(), &stopping)).await;
    drop(stopping);

    let (again, console) = leased(harness, state, &harness.address).await;
    let answer = converse(&client(model, backend.clone(), &again), console, RECALL)
        .await
        .finished("turn 4");
    println!("turn 4: {:?}", answer.text);
    assert_eq!(
        run.last_home(),
        second,
        "turn 4 ran outside the second home"
    );
    assert!(
        answer.text.contains(word),
        "turn 4 lost the session after the stop: {:?}",
        answer.text
    );
}

/// Whether claude is signed in, asked through the wrapper as Zone asks it: the environment every
/// child that is not an agent inherits, which carries the passthrough. The names claude was
/// given are printed so a dropped variable is told apart from a real sign-out.
async fn claude_preflight(run: &Run) -> bool {
    let inherited = cli::inherited();
    println!(
        "claude auth status is handed {:?}",
        inherited.keys().collect::<Vec<_>>()
    );
    let status = Status::check(
        AgentKind::Claude,
        &Cli::new(run.wrapper()).with_environment(inherited),
    )
    .await;
    let names = run.names();
    println!("claude auth status ran with {names:?}");
    let configuration = home::variable(AgentKind::Claude).expect("claude's home variable");
    for variable in [configuration, SECURE_STORAGE] {
        assert!(
            names.contains(variable),
            "{variable} never reached claude, so its status says nothing of the temporary home"
        );
    }
    assert_eq!(
        run.last_home(),
        run.home(HOME_A),
        "claude's status read another home than the temporary one"
    );
    match status {
        Ok(status) if status.signed_in => {
            println!("claude is signed in: {:?}", status.label);
            true
        }
        Ok(_) => {
            run.skip(
                "claude auth status reports the host's Keychain item signed out under a \
                 temporary CLAUDE_CONFIG_DIR with CLAUDE_SECURESTORAGE_CONFIG_DIR empty",
            );
            false
        }
        Err(error) => panic!("claude's sign-in could not be checked: {error}"),
    }
}

async fn claude_lanes() {
    let run = Run::from_environment(AgentKind::Claude);
    if !claude_preflight(&run).await {
        return;
    }
    let harness = Harness::new(None, true, vec![]).await;
    let state = configured(&run, &harness);
    let organization = organization(&harness).await;
    sqlx::query(
        "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2) \
         ON CONFLICT (organization_id) DO UPDATE SET provider = EXCLUDED.provider",
    )
    .bind(organization)
    .bind(ai_settings::provider(AgentKind::Claude))
    .execute(&harness.pool)
    .await
    .expect("the organization's AI settings");
    let settings = ai_settings::for_workspace(&harness.pool, harness.workspace)
        .await
        .expect("the workspace's effective AI settings");

    let resolved = backend::for_settings(
        &state,
        organization,
        &settings,
        Origin::Instance,
        Routing::default(),
    )
    .await
    .expect("the host's sign-in serves an organization without one of its own");
    assert!(resolved.login.is_none(), "the host's sign-in is no login");
    let backend = backend::bounded(resolved.backend, TURN_TIMEOUT);
    let LlmBackend::Cli {
        agent,
        settings,
        sign_in,
    } = &backend
    else {
        panic!("expected the claude CLI, got {backend:?}");
    };
    assert_eq!(*agent, AgentKind::Claude);
    assert_eq!(*sign_in, SignIn::Host);
    let work = settings
        .working_directory
        .clone()
        .expect("the organization's working directory");
    run.record(PROJECT, &Session::project(&work));

    let first_home = run.home(HOME_A);
    let second_home = run.home(HOME_B);
    let word = nonce();
    let session = first(&run, &harness, &state, CLAUDE_MODEL, backend.clone(), &word).await;
    assert_eq!(
        run.last_home(),
        first_home,
        "turn 1 ran outside the first home"
    );

    let carried = Session::resumed(session.as_str())
        .carry(AgentKind::Claude, &first_home, &second_home, &work)
        .expect("claude's session is carried to the second home");
    assert!(carried.bytes > 0, "claude's session file is empty");

    let configuration = home::variable(AgentKind::Claude).expect("claude's home variable");
    let second_directory = second_home.to_str().expect("a UTF-8 home").to_string();
    let in_second = backend.configured(|settings| {
        settings
            .with_variable(configuration, second_directory)
            .with_session(Session::resumed(session.as_str()))
    });
    resumed(
        &run,
        &harness,
        &state,
        CLAUDE_MODEL,
        &in_second,
        &word,
        &second_home,
    )
    .await;
}

/// One of `organization`'s logins of `run`'s agent, as the router would choose it: codex signed
/// in with the copy of the host's `auth.json`, claude with the OAuth token in the environment.
async fn login(state: &AppState, run: &Run, organization: Uuid, label: &str) -> Chosen {
    let row: AgentLoginRow = agent_logins::insert(
        state.db(),
        &Insert {
            organization_id: organization,
            agent: run.agent.as_str(),
            account: Some(label),
            credential: None,
            label: Some(label),
            expires_at: None,
        },
    )
    .await
    .expect("a stored login");
    let resolved = match run.agent {
        AgentKind::Codex => {
            let home = state
                .config()
                .agents
                .create_login_home(organization, AgentKind::Codex, row.id)
                .expect("the login's home");
            fs::copy(run.root.join(AUTH), home.join(CREDENTIALS)).expect("the login's sign-in");
            Login::Codex { home }
        }
        _ => Login::Claude {
            token: SecretValue::new(
                env::var(CLAUDE_CODE_OAUTH_TOKEN).expect("the parent checked the token is set"),
            ),
        },
    };
    Chosen {
        login: row,
        agent: run.agent,
        resolved,
        snapshot: None,
    }
}

/// The model each agent's turns run on: the cheapest each accepts.
fn model(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Codex => CODEX_MODEL,
        _ => CLAUDE_MODEL,
    }
}

/// `backend` bounded to a live turn, at codex's lowest effort for codex; claude's haiku takes
/// none.
fn limited(agent: AgentKind, backend: LlmBackend) -> LlmBackend {
    let bounded = backend::bounded(backend, TURN_TIMEOUT);
    match agent {
        AgentKind::Codex => backend::effort(bounded, CODEX_EFFORT),
        _ => bounded,
    }
}

/// Lane 6: with zone's endpoint on a closed port, the run fails before the agent takes a turn,
/// since zone's server is required.
async fn unreachable(harness: &Harness, state: &AppState, backend: LlmBackend) {
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .to_string();
    let (lease, console) = leased(harness, state, &closed).await;
    let answer = converse(&client(CODEX_MODEL, backend, &lease), console, UNREACHABLE).await;
    println!(
        "unreachable: {:?}, error {:?}, usage reported {}",
        answer.text, answer.error, answer.used
    );
    assert!(
        answer.error.is_some(),
        "codex ran without zone's required server: {:?}",
        answer.text
    );
    assert!(
        answer.text.is_empty() && !answer.used,
        "codex took a turn before failing: {:?}",
        answer.text
    );
}

/// The lanes of `agent` on two of an organization's own logins, the session moved between them
/// by the handover a chat's turn makes when it changes login.
async fn organization_lanes(agent: AgentKind) {
    let run = Run::from_environment(agent);
    let model = model(agent);
    let harness = Harness::new(None, true, vec![]).await;
    let state = configured(&run, &harness);
    let organization = organization(&harness).await;
    let suffix = Uuid::new_v4().simple().to_string();
    let first_login = login(&state, &run, organization, &format!("live-a-{suffix}")).await;
    let second_login = login(&state, &run, organization, &format!("live-b-{suffix}")).await;
    let agents = &state.config().agents;
    let first_home = agents.login_home(organization, agent, first_login.login.id);
    let second_home = agents.login_home(organization, agent, second_login.login.id);
    run.record(
        PROJECT,
        &Session::project(&agents.work(organization, agent)),
    );

    let on_first = backend::on_login(state.config(), organization, &first_login, None)
        .expect("the first login's backend");
    let backend = limited(agent, on_first.backend);
    let word = nonce();
    let session = first(&run, &harness, &state, model, backend.clone(), &word).await;
    assert_eq!(
        run.last_home(),
        first_home,
        "turn 1 ran outside the first home"
    );
    let names = run.names();
    println!("{agent} ran with {names:?}");
    let own = home::variable(agent).expect("the agent's home variable");
    for variable in HOST_HOMES.iter().filter(|variable| **variable != own) {
        assert!(!names.contains(*variable), "{agent} was handed {variable}");
    }

    let mut on_second = backend::on_login(state.config(), organization, &second_login, None)
        .expect("the second login's backend");
    let previous = ChatSession {
        login: Some(first_login.login.id),
        id: session.clone(),
        agent,
        entry: 0,
        prompt: None,
    };
    let notice = handover::opening(
        &state,
        organization,
        agent,
        Some(&previous),
        &mut on_second,
        Some(model),
        None,
    )
    .await
    .expect("the move onto the second login is told");
    assert!(
        notice.carried,
        "{agent}'s session was not carried to the second login"
    );
    resumed(
        &run,
        &harness,
        &state,
        model,
        &limited(agent, on_second.backend),
        &word,
        &second_home,
    )
    .await;

    if agent == AgentKind::Codex {
        unreachable(&harness, &state, backend).await;
    }
}

#[test]
#[ignore = "spends four turns of the host user's real Claude subscription"]
fn claude_runs_moves_stops_and_resumes_through_zone() {
    if env::var_os(COPY).is_some() {
        runtime().block_on(claude_lanes());
        return;
    }
    parent(
        AgentKind::Claude,
        SignIn::Host,
        "agent_cli_live::claude_runs_moves_stops_and_resumes_through_zone",
    );
}

#[test]
#[ignore = "spends four turns of the host user's real ChatGPT subscription"]
fn codex_runs_moves_stops_resumes_and_requires_zone() {
    if env::var_os(COPY).is_some() {
        runtime().block_on(organization_lanes(AgentKind::Codex));
        return;
    }
    parent(
        AgentKind::Codex,
        SignIn::Organization,
        "agent_cli_live::codex_runs_moves_stops_resumes_and_requires_zone",
    );
}

#[test]
#[ignore = "spends four turns of the Claude subscription CLAUDE_CODE_OAUTH_TOKEN signs in to"]
fn a_claude_organization_login_runs_moves_stops_and_resumes_through_zone() {
    if env::var_os(COPY).is_some() {
        runtime().block_on(organization_lanes(AgentKind::Claude));
        return;
    }
    parent(
        AgentKind::Claude,
        SignIn::Organization,
        "agent_cli_live::a_claude_organization_login_runs_moves_stops_and_resumes_through_zone",
    );
}
