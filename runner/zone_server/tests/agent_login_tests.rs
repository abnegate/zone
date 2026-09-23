//! Organizations sign in to Claude Code and Codex from AI settings.
//!
//! Claude's token endpoint is a local stub and codex is a stand-in script that replays what the
//! real one printed, so no test here signs in to anything real. The one test that asks the host's
//! own claude is ignored.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use axum::http::StatusCode;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, TimeDelta, Utc};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use reqwest::Url;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::SecretValue;
use zone_core::llm::AgentKind;
use zone_server::config::{AgentConfig, Config, ModelBackend};
use zone_server::services::login::claude::{AUTHORIZE_URL, CLIENT_ID, REDIRECT_URL, Tokens};
use zone_server::services::login::devices;

use common::{TestClient, test_email, test_password};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/login");
const TOKEN_PATH: &str = "/v1/oauth/token";
const ACCESS: &str = "fake-access-token-for-agent-login-tests";
const REFRESH: &str = "fake-refresh-token-for-agent-login-tests";
const CODE: &str = "fake-authorization-code";
const YEAR: i64 = 31_536_000;
const INFERENCE_SCOPE: &str = "user:inference";
const FULL_SCOPE: &str = "org:create_api_key user:profile user:inference \
                          user:sessions:claude_code user:mcp_servers user:file_upload user:plugins";
const AUTHORIZE_PARAMETERS: [&str; 8] = [
    "code",
    "client_id",
    "response_type",
    "redirect_uri",
    "scope",
    "code_challenge",
    "code_challenge_method",
    "state",
];
const ADMINS_ONLY: &str = "Only organization admins can sign in to coding agents";
const NOT_INSTALLED: &str = "The codex CLI is not installed on this server";
const UNREADABLE_CODE: &str =
    "Paste the code Claude showed, as code#state, or the whole callback URL";
const FOREIGN_CALLBACK: &str = "That URL is not Claude's sign-in callback";
const UNKNOWN_SIGN_IN: &str =
    "That code is not from a sign-in you started here, or the sign-in expired. Start again.";
const INVALID_CODE: &str = "invalid_code";
const START_AGAIN: &str = "start_again";
const NOT_FOUND: &str = "Organization not found";
const INTERNAL: &str = "Internal server error";
const REFUSAL: &str =
    "Error logging in with device code: device code request failed with status 403 Forbidden";
const POLL_FAILED: &str =
    "Error logging in with device code: device auth failed with status 500 Internal Server Error";
const VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
const USER_CODE: &str = "ABCD-EFGHI";
const MISSING_CODEX: &str = "/nonexistent/zone/codex";
const CREDENTIALS: &str = "auth.json";
const STAGING: &str = ".login";
const SAVED_LOGIN: &str = "{\"auth_mode\":\"chatgpt\",\"OPENAI_API_KEY\":null,\"tokens\":{\"id_token\":\"fixture\",\"access_token\":\"fixture\",\"refresh_token\":\"fixture\",\"account_id\":\"00000000-0000-4000-8000-000000000000\"},\"last_refresh\":\"2026-09-23T07:27:26.827371Z\"}\n";
const WAIT: Duration = Duration::from_secs(20);
const PAUSE: Duration = Duration::from_millis(50);
const ATTEMPTS: u128 = WAIT.as_millis() / PAUSE.as_millis();
/// How long the stand-in codex waits to be told how its sign-in ends.
const PATIENCE: Duration = Duration::from_secs(60);

/// A stand-in codex, steered through marker files in its own directory. `login --device-auth`
/// refuses when `refuse` exists, and otherwise prints the recorded prompt and waits: `approve`
/// saves a login and `fail` fails the way codex does when its poll is refused. It gives up once
/// its directory is gone, so a failed test leaves nothing running. `login status` and `logout`
/// read and clear the `auth.json` in the `CODEX_HOME` they are given, and `logout` fails without
/// touching it while `broken` exists.
struct Codex {
    directory: TempDir,
    executable: PathBuf,
}

impl Codex {
    const APPROVE: &'static str = "approve";
    const BROKEN: &'static str = "broken";
    const FAIL: &'static str = "fail";
    const REFUSE: &'static str = "refuse";
    const STARTED: &'static str = "started";
    const LOGOUTS: &'static str = "logouts";
    const PROCESS: &'static str = "device.pid";

    fn new() -> Self {
        let directory = TempDir::new().expect("a directory for the stand-in codex");
        let executable = directory.path().join("codex");
        fs::write(directory.path().join(CREDENTIALS), SAVED_LOGIN).expect("the login to save");
        fs::write(&executable, Self::script(directory.path())).expect("the stand-in codex");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("the stand-in codex to be executable");
        settle(&executable);
        Self {
            directory,
            executable,
        }
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn touch(&self, name: &str) {
        fs::write(self.marker(name), b"").expect("a marker for the stand-in codex");
    }

    fn approve(&self) {
        self.touch(Self::APPROVE);
    }

    fn fail(&self) {
        self.touch(Self::FAIL);
    }

    fn break_logout(&self) {
        self.touch(Self::BROKEN);
    }

    fn refuse(&self) {
        self.touch(Self::REFUSE);
    }

    fn accept(&self) {
        fs::remove_file(self.marker(Self::REFUSE)).expect("the refusal marker to go");
    }

    fn lines(&self, name: &str) -> Vec<String> {
        fs::read_to_string(self.marker(name))
            .map(|text| text.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn starts(&self) -> usize {
        self.lines(Self::STARTED).len()
    }

    fn logouts(&self) -> Vec<String> {
        self.lines(Self::LOGOUTS)
    }

    fn process(&self) -> Pid {
        let text = fs::read_to_string(self.marker(Self::PROCESS))
            .expect("the stand-in codex to have recorded its process id");
        Pid::from_raw(text.trim().parse().expect("a process id"))
    }

    fn script(control: &Path) -> String {
        let control = control.display();
        let (approve, broken, fail, refuse) =
            (Self::APPROVE, Self::BROKEN, Self::FAIL, Self::REFUSE);
        let (started, logouts, process) = (Self::STARTED, Self::LOGOUTS, Self::PROCESS);
        let polls = PATIENCE.as_millis() / PAUSE.as_millis();
        let pause = PAUSE.as_secs_f64();
        format!(
            r#"#!/bin/sh
control='{control}'
fixtures='{FIXTURES}'
case "$*" in
'login --device-auth')
    echo $$ > "$control/{process}"
    echo started >> "$control/{started}"
    if [ -e "$control/{refuse}" ]; then
        cat "$fixtures/fake-issuer/codex-login-device-auth-refused-403.stderr" >&2
        exit 1
    fi
    cat "$fixtures/codex-login-device-auth.stdout"
    waited=0
    while [ ! -e "$control/{approve}" ] && [ ! -e "$control/{fail}" ] && [ -d "$control" ] && [ "$waited" -lt {polls} ]; do
        sleep {pause}
        waited=$((waited + 1))
    done
    if [ -e "$control/{approve}" ]; then
        cp "$control/{CREDENTIALS}" "$CODEX_HOME/{CREDENTIALS}"
        cat "$fixtures/fake-issuer/codex-login-device-auth-success.stderr" >&2
        exit 0
    fi
    cat "$fixtures/fake-issuer/codex-login-device-auth-poll-failed-500.stderr" >&2
    exit 1
    ;;
'login status')
    if [ -f "$CODEX_HOME/{CREDENTIALS}" ]; then
        cat "$fixtures/fake-issuer/codex-login-status-chatgpt.stderr" >&2
        exit 0
    fi
    cat "$fixtures/codex-login-status-signed-out.stderr" >&2
    exit 1
    ;;
logout)
    printf '%s\n' "$CODEX_HOME" >> "$control/{logouts}"
    if [ -e "$control/{broken}" ]; then
        cat "$fixtures/fake-issuer/codex-login-status-corrupt-auth.stderr" >&2
        exit 1
    fi
    if [ -f "$CODEX_HOME/{CREDENTIALS}" ]; then
        rm -f "$CODEX_HOME/{CREDENTIALS}"
        cat "$fixtures/fake-issuer/codex-logout-chatgpt.stderr" >&2
    else
        cat "$fixtures/codex-logout-signed-out.stderr" >&2
    fi
    exit 0
    ;;
*)
    echo "unexpected arguments: $*" >&2
    exit 64
    ;;
esac
"#
        )
    }
}

/// Runs the stand-in once, so it is executable before any deadline starts: Linux refuses to exec
/// a file another process still holds open for writing, and macOS assesses a new executable on
/// its first run.
fn settle(executable: &Path) {
    for _ in 0..ATTEMPTS {
        match std::process::Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Err(error) if error.raw_os_error() == Some(Errno::ETXTBSY as i32) => {
                std::thread::sleep(PAUSE);
            }
            _ => return,
        }
    }
}

/// One test's server. Host login is always off, so no test asks the host's own CLIs.
struct Stage {
    client: TestClient,
    /// The agent state root, kept for as long as the test runs.
    _state: Option<TempDir>,
}

impl Stage {
    /// Claude's token endpoint is `claude`, and codex cannot be started at all.
    async fn claude(claude: &MockServer) -> Self {
        let config = Config {
            model_backend: codex_at(PathBuf::from(MISSING_CODEX)),
            agents: AgentConfig {
                host_login: false,
                claude_token_url: format!("{}{TOKEN_PATH}", claude.uri()),
                ..AgentConfig::default()
            },
            ..common::test_config()
        };
        Self {
            client: TestClient::with_config(config).await,
            _state: None,
        }
    }

    /// Codex runs from `executable`, with its homes under a private state root.
    async fn codex(executable: &Path) -> Self {
        let state = TempDir::new().expect("an agent state root");
        let config = Config {
            model_backend: codex_at(executable.to_path_buf()),
            agents: AgentConfig {
                state: state.path().join("agents"),
                host_login: false,
                ..AgentConfig::default()
            },
            ..common::test_config()
        };
        Self {
            client: TestClient::with_config(config).await,
            _state: Some(state),
        }
    }

    fn pool(&self) -> &PgPool {
        self.client.state().db()
    }

    fn home(&self, organization: Uuid) -> PathBuf {
        self.client
            .state()
            .config()
            .agents
            .home(organization, AgentKind::Codex)
    }

    /// `<state>/<organization>`, where every agent home of the organization lives.
    fn agent_state(&self, organization: Uuid) -> PathBuf {
        self.client
            .state()
            .config()
            .agents
            .state
            .join(organization.to_string())
    }

    async fn delete(&self, organization: Uuid, owner: &Person) {
        self.client
            .delete_auth(&format!("/api/organizations/{organization}"), &owner.token)
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }

    async fn status(&self, organization: Uuid, agent: &str, person: &Person) -> Value {
        let response = self
            .client
            .get_auth(&agent_path(organization, agent), &person.token)
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    async fn start(&self, organization: Uuid, agent: &str, body: Value, person: &Person) -> Value {
        let response = self
            .client
            .post_json_auth(&login_path(organization, agent), &body, &person.token)
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    async fn submit(
        &self,
        organization: Uuid,
        code: &str,
        person: &Person,
    ) -> common::TestResponse {
        self.client
            .post_json_auth(
                &code_path(organization),
                &json!({ "code": code }),
                &person.token,
            )
            .await
    }

    async fn sign_out(&self, organization: Uuid, agent: &str, person: &Person) {
        self.client
            .delete_auth(&login_path(organization, agent), &person.token)
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }

    /// The codex status once its sign-in is no longer pending.
    async fn settled(&self, organization: Uuid, person: &Person) -> Value {
        for _ in 0..ATTEMPTS {
            let status = self.status(organization, "codex", person).await;
            if status["state"] != "pending" {
                return status;
            }
            tokio::time::sleep(PAUSE).await;
        }
        panic!("the codex sign-in was still pending after {WAIT:?}");
    }

    async fn login_row(&self, organization: Uuid, agent: &str) -> Option<LoginRow> {
        sqlx::query_as(
            "SELECT credential, label, expires_at FROM agent_logins \
             WHERE organization_id = $1 AND agent = $2",
        )
        .bind(organization)
        .bind(agent)
        .fetch_optional(self.pool())
        .await
        .expect("the agent logins are readable")
    }

    async fn audited(&self, organization: Uuid) -> Vec<(String, Option<Uuid>, Option<Value>)> {
        sqlx::query_as(
            "SELECT action, actor_id, new_values FROM audit_logs \
             WHERE organization_id = $1 AND resource_type = 'agent_login' \
             ORDER BY created_at, id",
        )
        .bind(organization)
        .fetch_all(self.pool())
        .await
        .expect("the audit log is readable")
    }
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    credential: Option<String>,
    label: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

fn codex_at(executable: PathBuf) -> ModelBackend {
    ModelBackend::Cli {
        agent: AgentKind::Codex,
        executable: Some(executable),
    }
}

async fn token_endpoint(status: u16, body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

fn granted() -> Value {
    json!({
        "access_token": ACCESS,
        "refresh_token": REFRESH,
        "expires_in": YEAR,
        "scope": INFERENCE_SCOPE,
        "subscription_type": "max",
        "token_type": "Bearer",
    })
}

async fn exchanges(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .expect("the stub records requests")
        .iter()
        .map(|request| request.body_json().expect("a JSON exchange"))
        .collect()
}

struct Person {
    token: String,
    id: Uuid,
}

async fn person(client: &TestClient) -> Person {
    let body = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await
        .json_value();
    Person {
        token: body["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("registration returns an access token, got {body}"))
            .to_string(),
        id: body["user"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("registration returns the user, got {body}")),
    }
}

async fn organization(client: &TestClient, owner: &Person) -> Uuid {
    let body = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Agent sign-ins", "slug": Uuid::new_v4().to_string() }),
            &owner.token,
        )
        .await
        .json_value();
    body["organization"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("the organization is created, got {body}"))
}

async fn seat(
    client: &TestClient,
    organization: Uuid,
    owner: &Person,
    person: &Person,
    role: &str,
) {
    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": person.id, "role": role }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

fn agents_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/agents")
}

fn agent_path(organization: Uuid, agent: &str) -> String {
    format!("{}/{agent}", agents_path(organization))
}

fn login_path(organization: Uuid, agent: &str) -> String {
    format!("{}/login", agent_path(organization, agent))
}

fn code_path(organization: Uuid) -> String {
    format!("{}/code", login_path(organization, "claude"))
}

fn models(agent: AgentKind) -> Value {
    json!(agent.models())
}

fn signed_out(agent: AgentKind, provider: &str) -> Value {
    json!({
        "agent": agent.as_str(),
        "provider": provider,
        "state": "signed_out",
        "source": null,
        "label": null,
        "expires_at": null,
        "models": models(agent),
        "pending": null,
        "error": null,
    })
}

fn timestamp(value: &Value) -> DateTime<Utc> {
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&Utc))
        .unwrap_or_else(|| panic!("an RFC 3339 timestamp, got {value}"))
}

/// Whether `time` is `lifetime` after a moment between `before` and `after`, give or take the
/// second a timestamp is truncated to.
fn lasts(
    time: DateTime<Utc>,
    lifetime: TimeDelta,
    before: DateTime<Utc>,
    after: DateTime<Utc>,
) -> bool {
    time >= before + lifetime - TimeDelta::seconds(1) && time <= after + lifetime
}

fn without(value: &Value, key: &str) -> Value {
    let mut value = value.clone();
    value.as_object_mut().expect("a JSON object").remove(key);
    value
}

fn parameters(url: &str) -> Vec<(String, String)> {
    Url::parse(url)
        .unwrap_or_else(|error| panic!("{url} is not a URL: {error}"))
        .query_pairs()
        .into_owned()
        .collect()
}

fn parameter(url: &str, name: &str) -> String {
    parameters(url)
        .into_iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("{url} has no {name}"))
}

async fn ended(process: Pid) -> bool {
    for _ in 0..ATTEMPTS {
        if kill(process, None) == Err(Errno::ESRCH) {
            return true;
        }
        tokio::time::sleep(PAUSE).await;
    }
    false
}

#[tokio::test]
async fn a_member_reads_both_statuses_with_their_models_and_never_a_code() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let member = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    seat(&stage.client, organization, &owner, &member, "member").await;
    stage.start(organization, "codex", json!({}), &owner).await;

    let response = stage
        .client
        .get_auth(&agents_path(organization), &member.token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let agents = body["agents"].as_array().expect("a list of statuses");
    assert_eq!(agents.len(), 2, "{body}");
    assert_eq!(agents[0], signed_out(AgentKind::Claude, "claude_code"));
    let pending = &agents[1];
    assert_eq!(
        without(pending, "pending"),
        json!({
            "agent": "codex",
            "provider": "codex",
            "state": "pending",
            "source": null,
            "label": null,
            "expires_at": null,
            "models": models(AgentKind::Codex),
            "error": null,
        })
    );
    assert_eq!(pending["pending"]["verification_url"], VERIFICATION_URL);
    assert_eq!(
        pending["pending"]["user_code"],
        Value::Null,
        "a member was shown the code that finishes the organization's sign-in"
    );
    timestamp(&pending["pending"]["expires_at"]);
    assert!(!body.to_string().contains(USER_CODE), "{body}");

    let seen = stage.status(organization, "codex", &owner).await;
    assert_eq!(seen["pending"]["user_code"], USER_CODE);
    assert_eq!(
        stage.status(organization, "claude", &member).await,
        signed_out(AgentKind::Claude, "claude_code")
    );

    stage.sign_out(organization, "codex", &owner).await;
}

#[tokio::test]
async fn the_admin_who_started_a_codex_sign_in_still_sees_its_code_once_demoted() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let starter = person(&stage.client).await;
    let bystander = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    seat(&stage.client, organization, &owner, &starter, "admin").await;
    seat(&stage.client, organization, &owner, &bystander, "member").await;
    stage
        .start(organization, "codex", json!({}), &starter)
        .await;

    stage
        .client
        .patch_json_auth(
            &format!("/api/organizations/{organization}/members/{}", starter.id),
            &json!({ "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let started = stage.status(organization, "codex", &starter).await;
    assert_eq!(started["pending"]["user_code"], USER_CODE);
    let other = stage.status(organization, "codex", &bystander).await;
    assert_eq!(other["pending"]["user_code"], Value::Null);

    stage.sign_out(organization, "codex", &owner).await;
}

#[tokio::test]
async fn only_an_admin_may_start_finish_or_end_a_sign_in() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let member = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    seat(&stage.client, organization, &owner, &member, "member").await;

    for agent in ["claude", "codex"] {
        let refusals = [
            stage
                .client
                .post_json_auth(&login_path(organization, agent), &json!({}), &member.token)
                .await,
            stage
                .client
                .delete_auth(&login_path(organization, agent), &member.token)
                .await,
        ];
        for refused in refusals {
            refused.assert_status(StatusCode::FORBIDDEN);
            assert_eq!(refused.json_value(), json!({ "error": ADMINS_ONLY }));
        }
    }
    let submitted = stage
        .submit(organization, &format!("{CODE}#fake-state"), &member)
        .await;
    submitted.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(submitted.json_value(), json!({ "error": ADMINS_ONLY }));
    assert_eq!(codex.starts(), 0, "a member started codex");
}

#[tokio::test]
async fn a_stranger_or_an_unknown_agent_finds_nothing() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let stranger = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;

    for uri in [
        agents_path(organization),
        agent_path(organization, "claude"),
    ] {
        let response = stage.client.get_auth(&uri, &stranger.token).await;
        response.assert_status(StatusCode::NOT_FOUND);
        assert_eq!(response.json_value(), json!({ "error": NOT_FOUND }));
    }
    let mut refused = vec![
        stage
            .submit(organization, &format!("{CODE}#fake-state"), &stranger)
            .await,
    ];
    for agent in ["claude", "codex"] {
        refused.push(
            stage
                .client
                .post_json_auth(
                    &login_path(organization, agent),
                    &json!({}),
                    &stranger.token,
                )
                .await,
        );
        refused.push(
            stage
                .client
                .delete_auth(&login_path(organization, agent), &stranger.token)
                .await,
        );
    }
    for response in refused {
        response.assert_status(StatusCode::NOT_FOUND);
        assert_eq!(response.json_value(), json!({ "error": NOT_FOUND }));
    }

    let unknown = [
        stage
            .client
            .get_auth(&agent_path(organization, "gemini"), &owner.token)
            .await,
        stage
            .client
            .post_json_auth(
                &login_path(organization, "gemini"),
                &json!({}),
                &owner.token,
            )
            .await,
        stage
            .client
            .delete_auth(&login_path(organization, "gemini"), &owner.token)
            .await,
        stage
            .client
            .post_json_auth(
                &format!("{}/code", login_path(organization, "gemini")),
                &json!({ "code": format!("{CODE}#fake-state") }),
                &owner.token,
            )
            .await,
    ];
    for response in unknown {
        response.assert_status(StatusCode::NOT_FOUND);
        assert!(
            response.json_value()["error"].is_string(),
            "{}",
            response.text()
        );
    }
    assert_eq!(codex.starts(), 0);
}

#[tokio::test]
async fn the_authorize_url_carries_the_clis_parameters_in_order_for_each_scope() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;

    for (body, scope) in [
        (json!({}), INFERENCE_SCOPE),
        (json!({ "scope": "inference" }), INFERENCE_SCOPE),
        (json!({ "scope": "full" }), FULL_SCOPE),
    ] {
        let before = Utc::now();
        let started = stage.start(organization, "claude", body, &owner).await;
        let after = Utc::now();

        let url = started["authorize_url"].as_str().expect("an authorize URL");
        let pairs = parameters(url);
        let names: Vec<&str> = pairs.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, AUTHORIZE_PARAMETERS, "{url}");
        let value = |name: &str| parameter(url, name);
        assert_eq!(value("code"), "true");
        assert_eq!(value("client_id"), CLIENT_ID);
        assert_eq!(value("response_type"), "code");
        assert_eq!(value("redirect_uri"), REDIRECT_URL);
        assert_eq!(value("scope"), scope);
        assert_eq!(value("code_challenge_method"), "S256");
        for random in [value("code_challenge"), value("state")] {
            assert_eq!(random.len(), 43, "{random}");
            assert!(
                random
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
                "{random}"
            );
        }
        assert!(
            url.starts_with(&format!(
                "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code\
                 &redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope="
            )),
            "{url}"
        );
        assert_eq!(started["agent"], "claude");
        assert!(
            lasts(
                timestamp(&started["expires_at"]),
                TimeDelta::minutes(10),
                before,
                after
            ),
            "{started}"
        );
        assert_eq!(
            started.as_object().map(|fields| fields.len()),
            Some(3),
            "{started}"
        );
    }
    let refused = stage
        .client
        .post_json_auth(
            &login_path(organization, "claude"),
            &json!({ "scope": "everything" }),
            &owner.token,
        )
        .await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        refused.json_value()["error"]
            .as_str()
            .is_some_and(|error| error.contains("unknown variant `everything`")),
        "{}",
        refused.text()
    );
    assert!(exchanges(&claude).await.is_empty());
}

#[tokio::test]
async fn a_pasted_code_signs_the_organization_in_and_only_a_sealed_login_is_kept() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let started = stage.start(organization, "claude", json!({}), &owner).await;
    let url = started["authorize_url"].as_str().expect("an authorize URL");
    let state = parameter(url, "state");
    let challenge = parameter(url, "code_challenge");

    let before = Utc::now();
    let response = stage
        .submit(organization, &format!("{CODE}#{state}"), &owner)
        .await;
    let after = Utc::now();

    response.assert_status(StatusCode::OK);
    let status = response.json_value();
    assert_eq!(
        status,
        json!({
            "agent": "claude",
            "provider": "claude_code",
            "state": "signed_in",
            "source": "zone",
            "label": "Claude Max",
            "expires_at": null,
            "models": models(AgentKind::Claude),
            "pending": null,
            "error": null,
        }),
        "a sign-in that renews itself showed when its access token runs out"
    );
    assert_eq!(stage.status(organization, "claude", &owner).await, status);

    let sent = exchanges(&claude).await;
    assert_eq!(sent.len(), 1);
    let verifier = sent[0]["code_verifier"].as_str().expect("a verifier");
    assert_eq!(
        without(&sent[0], "code_verifier"),
        json!({
            "grant_type": "authorization_code",
            "code": CODE,
            "redirect_uri": REDIRECT_URL,
            "client_id": CLIENT_ID,
            "state": state,
            "expires_in": YEAR,
        })
    );
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier)),
        challenge,
        "the verifier sent is not the one the authorize link was challenged with"
    );

    let login = stage
        .login_row(organization, "claude")
        .await
        .expect("a stored login");
    let credential = login.credential.expect("a sealed credential");
    for secret in [ACCESS, REFRESH, CODE, verifier] {
        assert!(
            !credential.contains(secret),
            "the stored login shows {secret}"
        );
    }
    let tokens = Tokens::open(stage.client.state().encryption_key(), &credential)
        .expect("the stored login opens with the server's key");
    assert_eq!(tokens.access.expose(), ACCESS);
    assert_eq!(
        tokens.refresh.as_ref().map(|refresh| refresh.expose()),
        Some(REFRESH)
    );
    assert_eq!(login.label.as_deref(), Some("Claude Max"));
    let expires_at = login.expires_at.expect("the access token's expiry");
    assert_eq!(expires_at.timestamp(), tokens.expires_at.timestamp());
    assert!(
        lasts(expires_at, TimeDelta::seconds(YEAR), before, after),
        "{expires_at}"
    );

    assert_eq!(
        stage.audited(organization).await,
        [(
            "agent.signed_in".to_string(),
            Some(owner.id),
            Some(json!({ "agent": "claude", "source": "zone" }))
        )]
    );
}

#[tokio::test]
async fn a_pasted_callback_url_signs_in_as_well() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let started = stage.start(organization, "claude", json!({}), &owner).await;
    let state = parameter(started["authorize_url"].as_str().expect("a URL"), "state");

    let response = stage
        .submit(
            organization,
            &format!("{REDIRECT_URL}?code={CODE}&state={state}"),
            &owner,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["state"], "signed_in");
    assert_eq!(exchanges(&claude).await[0]["code"], CODE);
}

#[tokio::test]
async fn a_sign_in_without_a_refresh_token_shows_when_its_token_runs_out() {
    let claude = token_endpoint(200, without(&granted(), "refresh_token")).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let started = stage.start(organization, "claude", json!({}), &owner).await;
    let state = parameter(started["authorize_url"].as_str().expect("a URL"), "state");

    let before = Utc::now();
    let response = stage
        .submit(organization, &format!("{CODE}#{state}"), &owner)
        .await;
    let after = Utc::now();

    response.assert_status(StatusCode::OK);
    let status = response.json_value();
    assert_eq!(status["state"], "signed_in", "{status}");
    assert!(
        lasts(
            timestamp(&status["expires_at"]),
            TimeDelta::seconds(YEAR),
            before,
            after
        ),
        "{status}"
    );
    assert_eq!(stage.status(organization, "claude", &owner).await, status);
}

#[tokio::test]
async fn a_claude_login_zone_cannot_open_has_expired() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let mut another_key = [0_u8; 32];
    rand::fill(&mut another_key);
    let tokens = Tokens {
        access: SecretValue::new(ACCESS),
        refresh: Some(SecretValue::new(REFRESH)),
        expires_at: Utc::now() + TimeDelta::seconds(YEAR),
        scope: INFERENCE_SCOPE.to_string(),
        subscription: Some("max".to_string()),
    };
    sqlx::query(
        "INSERT INTO agent_logins (organization_id, agent, credential, label, expires_at) \
         VALUES ($1, 'claude', $2, 'Claude Max', $3)",
    )
    .bind(organization)
    .bind(
        tokens
            .seal(&another_key)
            .expect("tokens sealed with another key"),
    )
    .bind(tokens.expires_at)
    .execute(stage.pool())
    .await
    .expect("a login sealed with another key");

    let status = stage.status(organization, "claude", &owner).await;

    assert_eq!(
        status,
        json!({
            "agent": "claude",
            "provider": "claude_code",
            "state": "expired",
            "source": "zone",
            "label": "Claude Max",
            "expires_at": null,
            "models": models(AgentKind::Claude),
            "pending": null,
            "error": null,
        }),
        "a login no turn can open was shown as signed in"
    );
}

#[tokio::test]
async fn a_malformed_paste_or_a_state_zone_never_issued_is_refused_before_claude_is_asked() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let started = stage.start(organization, "claude", json!({}), &owner).await;
    let state = parameter(started["authorize_url"].as_str().expect("a URL"), "state");

    for (pasted, reason, kind) in [
        (CODE.to_string(), UNREADABLE_CODE, INVALID_CODE),
        (format!("{CODE}#"), UNREADABLE_CODE, INVALID_CODE),
        ("   ".to_string(), UNREADABLE_CODE, INVALID_CODE),
        (
            format!("https://attacker.example/oauth/code/callback?code={CODE}&state={state}"),
            FOREIGN_CALLBACK,
            INVALID_CODE,
        ),
        (format!("{CODE}#never-issued"), UNKNOWN_SIGN_IN, START_AGAIN),
    ] {
        let response = stage.submit(organization, &pasted, &owner).await;
        response.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json_value(),
            json!({ "error": reason, "kind": kind }),
            "{pasted:?}"
        );
    }
    assert!(exchanges(&claude).await.is_empty());

    stage
        .submit(organization, &format!("{CODE}#{state}"), &owner)
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn a_state_finishes_once_and_only_for_the_admin_and_organization_that_started_it() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let colleague = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let elsewhere = self::organization(&stage.client, &owner).await;
    seat(&stage.client, organization, &owner, &colleague, "admin").await;
    let state_of =
        |started: &Value| parameter(started["authorize_url"].as_str().expect("a URL"), "state");
    let refused = |response: common::TestResponse| {
        response.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json_value(),
            json!({ "error": UNKNOWN_SIGN_IN, "kind": START_AGAIN })
        );
    };

    let taken = state_of(&stage.start(organization, "claude", json!({}), &owner).await);
    refused(
        stage
            .submit(organization, &format!("{CODE}#{taken}"), &colleague)
            .await,
    );
    refused(
        stage
            .submit(organization, &format!("{CODE}#{taken}"), &owner)
            .await,
    );

    let moved = state_of(&stage.start(organization, "claude", json!({}), &owner).await);
    refused(
        stage
            .submit(elsewhere, &format!("{CODE}#{moved}"), &owner)
            .await,
    );
    refused(
        stage
            .submit(organization, &format!("{CODE}#{moved}"), &owner)
            .await,
    );

    let abandoned = state_of(&stage.start(organization, "claude", json!({}), &owner).await);
    let current = state_of(&stage.start(organization, "claude", json!({}), &owner).await);
    refused(
        stage
            .submit(organization, &format!("{CODE}#{abandoned}"), &owner)
            .await,
    );
    assert!(exchanges(&claude).await.is_empty());

    stage
        .submit(organization, &format!("{CODE}#{current}"), &owner)
        .await
        .assert_status(StatusCode::OK);
    refused(
        stage
            .submit(organization, &format!("{CODE}#{current}"), &owner)
            .await,
    );
    assert_eq!(exchanges(&claude).await.len(), 1);
    assert!(stage.login_row(elsewhere, "claude").await.is_none());
}

#[tokio::test]
async fn claude_refusing_the_code_is_a_bad_gateway_that_carries_its_reason() {
    let claude = token_endpoint(
        400,
        json!({ "error": "invalid_grant", "error_description": "Invalid authorization code" }),
    )
    .await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let started = stage.start(organization, "claude", json!({}), &owner).await;
    let state = parameter(started["authorize_url"].as_str().expect("a URL"), "state");

    let response = stage
        .submit(organization, &format!("{CODE}#{state}"), &owner)
        .await;

    response.assert_status(StatusCode::BAD_GATEWAY);
    assert_eq!(
        response.json_value(),
        json!({
            "error": "Claude refused the sign-in (HTTP 400): Invalid authorization code",
            "kind": START_AGAIN,
        })
    );
    assert!(stage.login_row(organization, "claude").await.is_none());
    assert_eq!(
        stage.status(organization, "claude", &owner).await,
        signed_out(AgentKind::Claude, "claude_code")
    );
}

#[tokio::test]
async fn signing_out_of_claude_forgets_the_login() {
    let claude = token_endpoint(200, granted()).await;
    let stage = Stage::claude(&claude).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let started = stage.start(organization, "claude", json!({}), &owner).await;
    let state = parameter(started["authorize_url"].as_str().expect("a URL"), "state");
    stage
        .submit(organization, &format!("{CODE}#{state}"), &owner)
        .await
        .assert_status(StatusCode::OK);

    stage.sign_out(organization, "claude", &owner).await;
    stage.sign_out(organization, "claude", &owner).await;

    assert_eq!(
        stage.status(organization, "claude", &owner).await,
        signed_out(AgentKind::Claude, "claude_code")
    );
    assert!(stage.login_row(organization, "claude").await.is_none());
    let claude_login = json!({ "agent": "claude", "source": "zone" });
    assert_eq!(
        stage.audited(organization).await,
        [
            (
                "agent.signed_in".to_string(),
                Some(owner.id),
                Some(claude_login.clone())
            ),
            (
                "agent.signed_out".to_string(),
                Some(owner.id),
                Some(claude_login)
            ),
        ],
        "signing out twice recorded the second one too"
    );
}

#[tokio::test]
async fn codex_has_no_code_to_paste() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;

    let response = stage
        .client
        .post_json_auth(
            &format!("{}/code", login_path(organization, "codex")),
            &json!({ "code": format!("{CODE}#fake-state") }),
            &owner.token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(body["error"].is_string(), "{body}");
    assert!(
        body.get("kind").is_none(),
        "a codex refusal said what to do with a Claude code: {body}"
    );
}

#[tokio::test]
async fn a_codex_sign_in_stays_pending_until_codex_saves_its_login() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;

    let before = Utc::now();
    let started = stage.start(organization, "codex", json!({}), &owner).await;
    let after = Utc::now();

    assert_eq!(
        without(&started, "expires_at"),
        json!({ "agent": "codex", "verification_url": VERIFICATION_URL, "user_code": USER_CODE })
    );
    let expires_at = timestamp(&started["expires_at"]);
    assert!(
        lasts(expires_at, TimeDelta::minutes(15), before, after),
        "{started}"
    );
    assert_eq!(
        stage.start(organization, "codex", json!({}), &owner).await,
        started,
        "a second start did not hand back the sign-in already waiting"
    );
    assert_eq!(codex.starts(), 1, "a second start ran codex again");
    let pending = stage.status(organization, "codex", &owner).await;
    assert_eq!(pending["state"], "pending");
    assert_eq!(
        pending["pending"],
        json!({
            "verification_url": VERIFICATION_URL,
            "user_code": USER_CODE,
            "expires_at": started["expires_at"],
        })
    );

    codex.approve();
    let status = stage.settled(organization, &owner).await;

    assert_eq!(
        status,
        json!({
            "agent": "codex",
            "provider": "codex",
            "state": "signed_in",
            "source": "zone",
            "label": "ChatGPT",
            "expires_at": null,
            "models": models(AgentKind::Codex),
            "pending": null,
            "error": null,
        })
    );
    let home = stage.home(organization);
    assert_eq!(
        fs::read_to_string(home.join(CREDENTIALS)).ok().as_deref(),
        Some(SAVED_LOGIN)
    );
    assert!(
        !home.join(STAGING).exists(),
        "the staging directory was left"
    );
    let login = stage
        .login_row(organization, "codex")
        .await
        .expect("a stored login");
    assert_eq!(login.credential, None, "codex keeps its own login");
    assert_eq!(login.label.as_deref(), Some("ChatGPT"));
    assert_eq!(login.expires_at, None);
    assert_eq!(
        stage.audited(organization).await,
        [(
            "agent.signed_in".to_string(),
            Some(owner.id),
            Some(json!({ "agent": "codex", "source": "zone" }))
        )]
    );
}

#[tokio::test]
async fn cancelling_a_codex_sign_in_stops_codex_and_leaves_nothing_behind() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    let process = codex.process();

    stage.sign_out(organization, "codex", &owner).await;

    assert!(ended(process).await, "codex was left running");
    assert_eq!(
        stage.status(organization, "codex", &owner).await,
        signed_out(AgentKind::Codex, "codex")
    );
    let home = stage.home(organization);
    assert!(
        !home.join(STAGING).exists(),
        "the staging directory was left"
    );
    assert!(!home.join(CREDENTIALS).exists());
    assert_eq!(codex.logouts(), [home.display().to_string()]);
    assert!(stage.login_row(organization, "codex").await.is_none());
    assert!(
        stage.audited(organization).await.is_empty(),
        "a sign-in that never finished was recorded as a sign-out"
    );
}

#[tokio::test]
async fn a_refused_codex_sign_in_is_a_bad_gateway_and_the_status_says_why_until_the_next_start() {
    let codex = Codex::new();
    codex.refuse();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;

    let response = stage
        .client
        .post_json_auth(&login_path(organization, "codex"), &json!({}), &owner.token)
        .await;

    response.assert_status(StatusCode::BAD_GATEWAY);
    assert_eq!(response.json_value(), json!({ "error": REFUSAL }));
    let status = stage.status(organization, "codex", &owner).await;
    assert_eq!(
        status,
        json!({
            "agent": "codex",
            "provider": "codex",
            "state": "signed_out",
            "source": null,
            "label": null,
            "expires_at": null,
            "models": models(AgentKind::Codex),
            "pending": null,
            "error": REFUSAL,
        })
    );
    assert!(!stage.home(organization).join(STAGING).exists());

    codex.accept();
    stage.start(organization, "codex", json!({}), &owner).await;
    let pending = stage.status(organization, "codex", &owner).await;
    assert_eq!(pending["state"], "pending");
    assert_eq!(
        pending["error"],
        Value::Null,
        "a new start kept the old refusal"
    );

    stage.sign_out(organization, "codex", &owner).await;
}

#[tokio::test]
async fn a_sign_in_that_fails_after_its_prompt_says_why_in_the_status() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;

    codex.fail();
    let status = stage.settled(organization, &owner).await;

    assert_eq!(status["state"], "signed_out");
    assert_eq!(status["error"], POLL_FAILED);
    assert!(stage.login_row(organization, "codex").await.is_none());
    assert!(!stage.home(organization).join(STAGING).exists());
    assert!(stage.audited(organization).await.is_empty());
}

#[tokio::test]
async fn codex_missing_from_the_server_is_unavailable() {
    let stage = Stage::codex(Path::new(MISSING_CODEX)).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;

    let response = stage
        .client
        .post_json_auth(&login_path(organization, "codex"), &json!({}), &owner.token)
        .await;

    response.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.json_value(), json!({ "error": NOT_INSTALLED }));
    assert_eq!(
        stage.status(organization, "codex", &owner).await["error"],
        NOT_INSTALLED
    );
}

#[tokio::test]
async fn signing_out_of_codex_logs_codex_out_and_forgets_the_login() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    codex.approve();
    assert_eq!(
        stage.settled(organization, &owner).await["state"],
        "signed_in"
    );
    let home = stage.home(organization);

    stage.sign_out(organization, "codex", &owner).await;

    assert_eq!(codex.logouts(), [home.display().to_string()]);
    assert!(
        !home.join(CREDENTIALS).exists(),
        "codex's login outlived the sign-out"
    );
    assert!(stage.login_row(organization, "codex").await.is_none());
    assert_eq!(
        stage.status(organization, "codex", &owner).await,
        signed_out(AgentKind::Codex, "codex")
    );
    let codex_login = json!({ "agent": "codex", "source": "zone" });
    assert_eq!(
        stage.audited(organization).await,
        [
            (
                "agent.signed_in".to_string(),
                Some(owner.id),
                Some(codex_login.clone())
            ),
            (
                "agent.signed_out".to_string(),
                Some(owner.id),
                Some(codex_login)
            ),
        ]
    );
}

#[tokio::test]
async fn a_sign_out_codex_cannot_finish_still_deletes_codexs_login() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    codex.approve();
    assert_eq!(
        stage.settled(organization, &owner).await["state"],
        "signed_in"
    );
    let home = stage.home(organization);
    codex.break_logout();

    stage.sign_out(organization, "codex", &owner).await;

    assert_eq!(codex.logouts(), [home.display().to_string()]);
    assert!(
        !home.join(CREDENTIALS).exists(),
        "codex's login outlived the sign-out"
    );
    assert!(stage.login_row(organization, "codex").await.is_none());
    assert_eq!(
        stage.status(organization, "codex", &owner).await,
        signed_out(AgentKind::Codex, "codex")
    );
}

#[tokio::test]
async fn a_sign_out_that_cannot_delete_codexs_login_leaves_the_organization_signed_in() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    codex.approve();
    assert_eq!(
        stage.settled(organization, &owner).await["state"],
        "signed_in"
    );
    let home = stage.home(organization);
    codex.break_logout();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o500)).expect("a locked home");

    let response = stage
        .client
        .delete_auth(&login_path(organization, "codex"), &owner.token)
        .await;
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).expect("the home unlocked");

    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.json_value(),
        json!({ "error": "Internal server error" })
    );
    assert!(home.join(CREDENTIALS).exists());
    assert!(stage.login_row(organization, "codex").await.is_some());
    assert_eq!(
        stage.status(organization, "codex", &owner).await["state"],
        "signed_in"
    );

    fs::remove_file(codex.marker(Codex::BROKEN)).expect("codex's logout mended");
    stage.sign_out(organization, "codex", &owner).await;
    assert!(stage.login_row(organization, "codex").await.is_none());
}

#[tokio::test]
async fn a_codex_login_whose_file_is_gone_has_expired() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    codex.approve();
    assert_eq!(
        stage.settled(organization, &owner).await["state"],
        "signed_in"
    );

    fs::remove_file(stage.home(organization).join(CREDENTIALS)).expect("codex's login");

    let status = stage.status(organization, "codex", &owner).await;
    assert_eq!(status["state"], "expired");
    assert_eq!(status["source"], "zone");
    assert_eq!(status["label"], "ChatGPT");
}

#[tokio::test]
async fn a_codex_sign_in_zone_cannot_prepare_names_no_server_path() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    let leftover = stage.home(organization).join(STAGING).join("leftover");
    fs::create_dir_all(&leftover).expect("a directory an earlier attempt left");
    fs::write(leftover.join("file"), b"").expect("a file an earlier attempt left");
    fs::set_permissions(&leftover, fs::Permissions::from_mode(0o500)).expect("the leftover locked");

    let response = stage
        .client
        .post_json_auth(&login_path(organization, "codex"), &json!({}), &owner.token)
        .await;
    fs::set_permissions(&leftover, fs::Permissions::from_mode(0o700))
        .expect("the leftover unlocked");

    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.json_value(), json!({ "error": INTERNAL }));
    let status = stage.status(organization, "codex", &owner).await;
    assert_eq!(
        status["error"],
        Value::Null,
        "the server's own failure was kept to show to members"
    );
    assert_eq!(codex.starts(), 0);
}

#[tokio::test]
async fn deleting_an_organization_logs_codex_out_and_removes_its_agent_state() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    codex.approve();
    assert_eq!(
        stage.settled(organization, &owner).await["state"],
        "signed_in"
    );
    codex.refuse();
    stage
        .client
        .post_json_auth(&login_path(organization, "codex"), &json!({}), &owner.token)
        .await
        .assert_status(StatusCode::BAD_GATEWAY);
    assert_eq!(devices::failure(organization).as_deref(), Some(REFUSAL));
    let home = stage.home(organization);
    let agent_state = stage.agent_state(organization);
    let claude = agent_state.join("claude");
    fs::create_dir_all(&claude).expect("claude's home for the organization");
    fs::write(claude.join(".claude.json"), "{}").expect("a file claude keeps");

    stage.delete(organization, &owner).await;

    assert_eq!(codex.logouts(), [home.display().to_string()]);
    assert!(
        !agent_state.exists(),
        "the deleted organization's agent state was left on the server"
    );
    assert_eq!(
        devices::failure(organization),
        None,
        "the deleted organization's last failure was kept"
    );
}

#[tokio::test]
async fn deleting_an_organization_mid_sign_in_stops_codex_and_removes_its_agent_state() {
    let codex = Codex::new();
    let stage = Stage::codex(&codex.executable).await;
    let owner = person(&stage.client).await;
    let organization = organization(&stage.client, &owner).await;
    stage.start(organization, "codex", json!({}), &owner).await;
    let process = codex.process();
    let home = stage.home(organization);
    let agent_state = stage.agent_state(organization);

    stage.delete(organization, &owner).await;

    assert!(
        ended(process).await,
        "codex was left running for a deleted organization"
    );
    assert_eq!(codex.logouts(), [home.display().to_string()]);
    assert!(
        !agent_state.exists(),
        "the deleted organization's agent state was left on the server"
    );
    assert!(devices::pending(organization).is_none());
}

#[tokio::test]
#[ignore = "asks the host's own claude, which must be signed in"]
async fn host_login_reports_the_hosts_claude() {
    let config = Config {
        agents: AgentConfig {
            host_login: true,
            ..AgentConfig::default()
        },
        ..common::test_config()
    };
    let client = TestClient::with_config(config).await;
    let owner = person(&client).await;
    let organization = organization(&client, &owner).await;

    let response = client
        .get_auth(&agent_path(organization, "claude"), &owner.token)
        .await;

    response.assert_status(StatusCode::OK);
    let status = response.json_value();
    assert_eq!(status["state"], "signed_in", "{status}");
    assert_eq!(status["source"], "host", "{status}");
    assert_eq!(status["expires_at"], Value::Null);
}
