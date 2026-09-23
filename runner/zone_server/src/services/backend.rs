//! Where a workspace's completions go: the endpoint, or a coding agent CLI.

use std::path::{Path, PathBuf};
use std::time::Duration;

use uuid::Uuid;
use zone_core::llm::provider::{STDERR_HEADING, SignIn};
use zone_core::llm::{AgentKind, CliSettings, Credential, LlmBackend};

use crate::config::Config;
use crate::db::ai_settings::{self, EffectiveAiSettings};
use crate::db::workspaces;
use crate::services::login::credential::{self, Login};
use crate::state::AppState;

pub const REMEDY: &str = "Sign in again under Organization Settings > AI Settings.";

// Lowercased words each CLI prints when its sign-in failed. None follows the
// word token, bearer, basic, password, passwd or secret: redaction blanks
// what comes after those, so a failure could never match there.
const CLAUDE_SIGN_IN_FAILURES: &[&str] = &[
    "not logged in",
    "run /login",
    "login expired",
    "log in again",
    "oauth session expired",
    "invalid api key",
    "invalid auth token",
    "authentication_failed",
];

const CODEX_SIGN_IN_FAILURES: &[&str] = &[
    "not logged in",
    "sign in again",
    "logging out.",
    "codex login`",
    "401 unauthorized",
    "unauthorized (401)",
];

// Either CLI's words for a sign-in it could not renew. Codex prints them only
// on stderr, which makes them the one failure looked for past the agent's own
// words.
const RENEWAL_FAILURES: &[&str] = &["could not be refreshed"];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "The {agent} CLI is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings."
    )]
    SignedOut { agent: AgentKind },
    #[error("The {agent} CLI's sign-in could not be renewed: {message}. {REMEDY}")]
    Renewal { agent: AgentKind, message: String },
    #[error("Could not prepare the {agent} CLI's state directory: {message}")]
    Home { agent: AgentKind, message: String },
    #[error("Could not read the {agent} CLI's sign-in: {source}")]
    Database {
        agent: AgentKind,
        #[source]
        source: sqlx::Error,
    },
}

impl Error {
    fn login(agent: AgentKind, error: credential::Error) -> Self {
        match error {
            credential::Error::Database(source) => Self::Database { agent, source },
            error => Self::Renewal {
                agent,
                message: error.to_string(),
            },
        }
    }
}

/// The instance-wide default the environment selects, with every variable its
/// agent runs with.
pub fn instance(config: &Config) -> LlmBackend {
    match crate::state::llm_backend(config) {
        LlmBackend::Cli { agent, settings } => {
            LlmBackend::cli(agent, prepared(config, agent, settings))
        }
        LlmBackend::Http => LlmBackend::Http,
    }
}

/// The backend a workspace's completions run on. A workspace or AI settings that
/// cannot be read leave it on the instance default.
pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Result<LlmBackend, Error> {
    let organization = match workspaces::get_workspace(state.db(), workspace).await {
        Ok(Some(row)) => row.organization_id,
        Ok(None) => {
            tracing::warn!(%workspace, "No such workspace; using the instance's default backend");
            return Ok(instance(state.config()));
        }
        Err(error) => {
            tracing::warn!(
                %workspace,
                %error,
                "Could not read the workspace; using the instance's default backend"
            );
            return Ok(instance(state.config()));
        }
    };
    match ai_settings::get_effective_ai_settings(state.db(), organization, workspace).await {
        Ok(settings) => for_settings(state, organization, &settings).await,
        Err(error) => {
            tracing::warn!(
                %workspace,
                %error,
                "Could not read the AI settings; using the instance's default backend"
            );
            Ok(instance(state.config()))
        }
    }
}

/// The backend `settings` choose for one of `organization`'s workspaces.
///
/// A coding agent provider runs that agent in the organization's own working
/// directory, under the organization's sign-in, else under the host's when the
/// instance allows that. Every other provider runs on the instance default.
pub async fn for_settings(
    state: &AppState,
    organization: Uuid,
    settings: &EffectiveAiSettings,
) -> Result<LlmBackend, Error> {
    let config = state.config();
    let Some(agent) = settings.agent() else {
        return Ok(instance(config));
    };
    let login = credential::resolve(state, organization, agent)
        .await
        .map_err(|error| Error::login(agent, error))?;
    if login.is_none() && !config.agents.host_login {
        return Err(Error::SignedOut { agent });
    }

    let work = config.agents.work(organization, agent);
    let home = config
        .agents
        .create_home(organization, agent)
        .map_err(|error| Error::Home {
            agent,
            message: format!("{}: {error}", work.display()),
        })?;
    let mut cli = CliSettings::default()
        .with_working_directory(work)
        .with_sign_in(SignIn::Organization);
    if let Some(executable) = overridden(config, agent) {
        cli = cli.with_executable(executable);
    }
    let cli = match login {
        Some(Login::Claude { token }) => {
            let variable = agent.token().ok_or(Error::SignedOut { agent })?;
            homed(cli, agent, &home)?.with_credential(Credential::key(variable, token))
        }
        Some(Login::Codex { home }) => homed(cli, agent, &home)?,
        None => cli,
    };
    Ok(LlmBackend::cli(agent, prepared(config, agent, cli)))
}

/// A failure a client reported, and whether it was a coding agent's sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remedied {
    /// The failure, followed by how to fix a sign-in that failed.
    pub message: String,
    /// Whether the agent's sign-in failed, which no retry fixes.
    pub signed_out: bool,
}

/// What to do about `message`, a failure `agent` reported under `sign_in`,
/// when it reads as the agent's sign-in failing.
pub fn remedy(agent: AgentKind, sign_in: SignIn, message: &str) -> Option<String> {
    let failures = match agent {
        AgentKind::Claude => CLAUDE_SIGN_IN_FAILURES,
        AgentKind::Codex => CODEX_SIGN_IN_FAILURES,
    };
    let words = own_words(message).to_ascii_lowercase();
    let whole = message.to_ascii_lowercase();
    let signed_out = failures.iter().any(|failure| words.contains(failure))
        || RENEWAL_FAILURES
            .iter()
            .any(|failure| whole.contains(failure));
    signed_out.then(|| match sign_in {
        SignIn::Organization => REMEDY.to_string(),
        SignIn::Instance => format!(
            "The server's own {agent} sign-in failed; the server operator must sign in again \
             on the host."
        ),
    })
}

/// A coding agent's own words about a failure it reported. zone_core follows
/// them with [`STDERR_HEADING`] and the tail of the agent's stderr.
pub fn own_words(message: &str) -> &str {
    message
        .split_once(STDERR_HEADING)
        .map_or(message, |(words, _)| words)
}

/// `message`, a failure a client on `backend` reported, followed by its
/// remedy when a coding agent's sign-in failed.
pub fn remedied(backend: &LlmBackend, message: String) -> Remedied {
    let LlmBackend::Cli { agent, settings } = backend else {
        return Remedied {
            message,
            signed_out: false,
        };
    };
    match remedy(*agent, settings.sign_in, &message) {
        Some(fix) if message.contains(&fix) => Remedied {
            message,
            signed_out: true,
        },
        Some(fix) => Remedied {
            message: format!("{message}\n{fix}"),
            signed_out: true,
        },
        None => Remedied {
            message,
            signed_out: false,
        },
    }
}

/// `backend` with a coding agent's turn given `timeout`: the budget of what
/// it runs for, a chat's or a task attempt's, rather than its own default.
pub fn bounded(backend: LlmBackend, timeout: Duration) -> LlmBackend {
    match backend {
        LlmBackend::Cli { agent, settings } => {
            LlmBackend::cli(agent, settings.with_timeout(timeout))
        }
        LlmBackend::Http => LlmBackend::Http,
    }
}

/// `settings` with the variables `agent` always runs with and, for codex, the
/// sandbox the instance allows its tools.
fn prepared(config: &Config, agent: AgentKind, settings: CliSettings) -> CliSettings {
    let settings = agent
        .defaults()
        .iter()
        .fold(settings, |settings, (name, value)| {
            settings.with_variable(*name, *value)
        });
    match agent {
        AgentKind::Codex => settings.with_sandbox(config.agents.codex_sandbox),
        AgentKind::Claude => settings,
    }
}

/// The binary the environment names for `agent`, when it names one.
fn overridden(config: &Config, agent: AgentKind) -> Option<PathBuf> {
    let executable = config.agent_executable(agent);
    (executable.as_path() != Path::new(agent.executable())).then_some(executable)
}

/// `settings` with `agent` pointed at `home` for its settings and sign-in.
fn homed(settings: CliSettings, agent: AgentKind, home: &Path) -> Result<CliSettings, Error> {
    let directory = home.to_str().ok_or_else(|| Error::Home {
        agent,
        message: format!("{} is not valid UTF-8", home.display()),
    })?;
    Ok(settings.with_variable(agent.home(), directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, Utc};
    use sqlx::PgPool;
    use tempfile::TempDir;
    use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
    use zone_core::llm::CodexSandbox;
    use zone_core::secret::{SecretValue, redact};

    use crate::config::{AgentConfig, ModelBackend};
    use crate::db::agent_logins::{self, Upsert};
    use crate::db::ai_settings::{PROVIDER_CLAUDE_CODE, PROVIDER_CODEX};
    use crate::db::organizations;
    use crate::services::login::claude::Tokens;

    const ACCESS: &str = "fake-claude-access-token";
    const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
    const CODEX_HOME: &str = "CODEX_HOME";
    const CLAUDE_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

    struct Fixture {
        pool: PgPool,
        organization: Uuid,
        workspace: Uuid,
        agents: TempDir,
    }

    impl Fixture {
        async fn new(provider: &str) -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
            )
            .await
            .expect("the test database");
            let organization = organizations::create_organization(
                &pool,
                "Backend test",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .expect("an organization");
            let workspace = workspaces::create_workspace(
                &pool,
                organization.id,
                "Backend",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .expect("a workspace");
            sqlx::query(
                "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2)",
            )
            .bind(organization.id)
            .bind(provider)
            .execute(&pool)
            .await
            .expect("the organization's AI settings");
            Self {
                pool,
                organization: organization.id,
                workspace: workspace.id,
                agents: TempDir::new().expect("an agent state root"),
            }
        }

        async fn self_hosted() -> Self {
            Self::new(PROVIDER_SELF_HOSTED).await
        }

        async fn override_workspace(&self, provider: &str) {
            sqlx::query(
                "INSERT INTO workspace_ai_settings (workspace_id, provider) VALUES ($1, $2)",
            )
            .bind(self.workspace)
            .bind(provider)
            .execute(&self.pool)
            .await
            .expect("the workspace's own AI settings");
        }

        fn config(&self) -> Config {
            Config {
                agents: AgentConfig {
                    state: self.agents.path().to_path_buf(),
                    ..AgentConfig::default()
                },
                ..crate::state::test_config()
            }
        }

        fn state(&self, config: Config) -> AppState {
            AppState::new(config, self.pool.clone(), None)
        }

        async fn sign_in_claude(&self, state: &AppState, expires_in: TimeDelta) {
            let tokens = Tokens {
                access: SecretValue::new(ACCESS),
                refresh: None,
                expires_at: Utc::now() + expires_in,
                scope: "user:inference".to_string(),
                subscription: None,
            };
            let sealed = tokens.seal(state.encryption_key()).expect("tokens to seal");
            self.store(AgentKind::Claude, Some(&sealed)).await;
        }

        async fn sign_in_codex(&self) {
            self.store(AgentKind::Codex, None).await;
        }

        async fn store(&self, agent: AgentKind, credential: Option<&str>) {
            agent_logins::upsert(
                &self.pool,
                &Upsert {
                    organization_id: self.organization,
                    agent: agent.as_str(),
                    credential,
                    label: None,
                    expires_at: None,
                },
            )
            .await
            .expect("a stored login");
        }

        fn home(&self, agent: AgentKind) -> PathBuf {
            self.agents
                .path()
                .join(self.organization.to_string())
                .join(agent.as_str())
        }

        async fn resolve(&self, state: &AppState) -> Result<LlmBackend, Error> {
            for_workspace(state, self.workspace).await
        }

        async fn remove(&self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
        }
    }

    fn cli(resolved: Result<LlmBackend, Error>) -> (AgentKind, CliSettings) {
        match resolved {
            Ok(LlmBackend::Cli { agent, settings }) => (agent, settings),
            other => panic!("expected a coding agent CLI, got {other:?}"),
        }
    }

    fn variable<'a>(settings: &'a CliSettings, name: &str) -> Option<&'a str> {
        settings.variables.get(name).map(String::as_str)
    }

    fn assert_defaults(agent: AgentKind, settings: &CliSettings) {
        for (name, value) in agent.defaults() {
            assert_eq!(variable(settings, name), Some(*value), "{name}");
        }
    }

    fn assert_private(directory: &Path) {
        assert!(
            directory.is_dir(),
            "{} was not created",
            directory.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory)
                .expect("the directory's metadata")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o700,
                "{} is open to other users",
                directory.display()
            );
        }
    }

    #[tokio::test]
    async fn a_self_hosted_workspace_keeps_the_endpoint_the_instance_defaults_to() {
        let fixture = Fixture::self_hosted().await;
        let state = fixture.state(fixture.config());

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        assert!(matches!(resolved, Ok(LlmBackend::Http)), "{resolved:?}");
    }

    #[tokio::test]
    async fn a_self_hosted_workspace_keeps_the_agent_the_instance_defaults_to() {
        let fixture = Fixture::self_hosted().await;
        let executable = PathBuf::from("/opt/homebrew/bin/claude");
        let state = fixture.state(Config {
            model_backend: ModelBackend::Cli {
                agent: AgentKind::Claude,
                executable: Some(executable.clone()),
            },
            ..fixture.config()
        });
        fixture.sign_in_claude(&state, TimeDelta::hours(1)).await;

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        let (agent, settings) = cli(resolved);
        assert_eq!(agent, AgentKind::Claude);
        assert_eq!(settings.executable, Some(executable));
        assert_eq!(
            settings.working_directory, None,
            "the instance's own agent runs where it always has"
        );
        assert!(
            matches!(settings.credential, Credential::Inherited),
            "the instance's own agent runs under the host's sign-in, not an organization's"
        );
        assert_eq!(
            settings.sign_in,
            SignIn::Instance,
            "only the operator can renew the instance's own sign-in"
        );
        assert_eq!(variable(&settings, CLAUDE_CONFIG_DIR), None);
        assert_defaults(AgentKind::Claude, &settings);
    }

    #[tokio::test]
    async fn a_self_hosted_workspace_runs_the_instances_codex_in_the_configured_sandbox() {
        let fixture = Fixture::self_hosted().await;
        let mut config = Config {
            model_backend: ModelBackend::Cli {
                agent: AgentKind::Codex,
                executable: None,
            },
            ..fixture.config()
        };
        config.agents.codex_sandbox = CodexSandbox::DangerFullAccess;
        let state = fixture.state(config);

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        let (agent, settings) = cli(resolved);
        assert_eq!(agent, AgentKind::Codex);
        assert_eq!(settings.sandbox, CodexSandbox::DangerFullAccess);
        assert_eq!(settings.working_directory, None);
    }

    #[tokio::test]
    async fn an_organization_on_claude_code_runs_claude_under_its_own_sign_in() {
        let fixture = Fixture::new(PROVIDER_CLAUDE_CODE).await;
        let state = fixture.state(fixture.config());
        fixture.sign_in_claude(&state, TimeDelta::hours(1)).await;

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        let (agent, settings) = cli(resolved);
        assert_eq!(agent, AgentKind::Claude);
        assert_eq!(settings.credential.variable(), Some(CLAUDE_TOKEN));
        assert_eq!(settings.credential.expose(), Some(ACCESS));
        assert_eq!(settings.sign_in, SignIn::Organization);
        let home = fixture.home(AgentKind::Claude);
        assert_eq!(
            variable(&settings, CLAUDE_CONFIG_DIR).map(PathBuf::from),
            Some(home.clone())
        );
        assert_defaults(AgentKind::Claude, &settings);
        let work = home.join("work");
        assert_eq!(settings.working_directory.as_deref(), Some(work.as_path()));
        assert_private(&home);
        assert_private(&work);
        assert_eq!(settings.executable, None);
    }

    #[tokio::test]
    async fn a_workspace_on_codex_runs_its_organizations_codex_sign_in() {
        let fixture = Fixture::self_hosted().await;
        fixture.override_workspace(PROVIDER_CODEX).await;
        fixture.sign_in_codex().await;
        let mut config = fixture.config();
        config.agents.codex_sandbox = CodexSandbox::DangerFullAccess;
        let state = fixture.state(config);

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        let (agent, settings) = cli(resolved);
        assert_eq!(agent, AgentKind::Codex);
        assert!(matches!(settings.credential, Credential::Inherited));
        assert_eq!(settings.sign_in, SignIn::Organization);
        let home = fixture.home(AgentKind::Codex);
        assert_eq!(
            variable(&settings, CODEX_HOME).map(PathBuf::from),
            Some(home.clone())
        );
        assert_eq!(variable(&settings, CLAUDE_CONFIG_DIR), None);
        let work = home.join("work");
        assert_eq!(settings.working_directory.as_deref(), Some(work.as_path()));
        assert_private(&work);
        assert_eq!(settings.sandbox, CodexSandbox::DangerFullAccess);
    }

    #[tokio::test]
    async fn an_organization_with_no_sign_in_runs_the_hosts_when_the_instance_allows_it() {
        let fixture = Fixture::new(PROVIDER_CLAUDE_CODE).await;
        let mut config = fixture.config();
        config.agents.host_login = true;
        let state = fixture.state(config);

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        let (agent, settings) = cli(resolved);
        assert_eq!(agent, AgentKind::Claude);
        assert!(matches!(settings.credential, Credential::Inherited));
        assert_eq!(
            settings.sign_in,
            SignIn::Organization,
            "an organization's own sign-in replaces the host's, so its admins can fix it"
        );
        for home in [CLAUDE_CONFIG_DIR, CODEX_HOME] {
            assert_eq!(
                variable(&settings, home),
                None,
                "a home would hide the host's own sign-in"
            );
        }
        let work = fixture.home(AgentKind::Claude).join("work");
        assert_eq!(settings.working_directory.as_deref(), Some(work.as_path()));
        assert_private(&work);
        assert_defaults(AgentKind::Claude, &settings);
    }

    #[tokio::test]
    async fn an_organization_with_no_sign_in_is_signed_out_when_the_host_login_is_off() {
        let fixture = Fixture::new(PROVIDER_CLAUDE_CODE).await;
        let mut config = fixture.config();
        config.agents.host_login = false;
        let state = fixture.state(config);

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        assert!(
            matches!(
                resolved,
                Err(Error::SignedOut {
                    agent: AgentKind::Claude
                })
            ),
            "{resolved:?}"
        );
        assert!(
            !fixture
                .agents
                .path()
                .join(fixture.organization.to_string())
                .exists(),
            "a signed-out organization was given a state directory"
        );
    }

    #[tokio::test]
    async fn an_expired_sign_in_asks_the_organization_to_sign_in_again() {
        let fixture = Fixture::new(PROVIDER_CLAUDE_CODE).await;
        let state = fixture.state(fixture.config());
        fixture.sign_in_claude(&state, TimeDelta::minutes(-1)).await;

        let resolved = fixture.resolve(&state).await;
        fixture.remove().await;

        match resolved {
            Err(
                error @ Error::Renewal {
                    agent: AgentKind::Claude,
                    ..
                },
            ) => assert!(error.to_string().ends_with(REMEDY), "{error}"),
            other => panic!("expected the sign-in to need renewing, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_instances_claude_binary_serves_only_an_organization_that_chose_claude() {
        let fixture = Fixture::new(PROVIDER_CLAUDE_CODE).await;
        let claude = PathBuf::from("/opt/homebrew/bin/claude");
        let chose_claude = fixture.state(Config {
            model_backend: ModelBackend::Cli {
                agent: AgentKind::Claude,
                executable: Some(claude.clone()),
            },
            ..fixture.config()
        });
        let chose_codex = fixture.state(Config {
            model_backend: ModelBackend::Cli {
                agent: AgentKind::Codex,
                executable: Some(PathBuf::from("/opt/homebrew/bin/codex")),
            },
            ..fixture.config()
        });

        let same = fixture.resolve(&chose_claude).await;
        let other = fixture.resolve(&chose_codex).await;
        fixture.remove().await;

        assert_eq!(cli(same).1.executable, Some(claude));
        assert_eq!(cli(other).1.executable, None);
    }

    #[tokio::test]
    async fn a_workspace_that_does_not_exist_runs_on_the_instance_default() {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let state = AppState::new(crate::state::test_config(), pool, None);

        let resolved = for_workspace(&state, Uuid::new_v4()).await;

        assert!(matches!(resolved, Ok(LlmBackend::Http)), "{resolved:?}");
    }

    /// A CLI's failure as the chat loop reports it: zone_core redacts it, and
    /// the loop and `LlmClient` wrap it.
    fn reported(agent: AgentKind, failure: &str) -> String {
        format!(
            "Failed to generate response: {}",
            redact(&format!("{agent}: {failure}"))
        )
    }

    const CLAUDE_SIGN_IN_FAILURES: &[&str] = &[
        "Not logged in · Please run /login",
        "Not logged in. Run claude auth login to authenticate.",
        "Not logged in · Run /login",
        "Login expired · Please run /login",
        "OAuth token revoked · Please run /login",
        "Invalid API key · Fix external API key",
        "Invalid auth token · Fix external auth token",
        "Failed to authenticate: OAuth session expired and could not be refreshed",
        "Your organization has disabled API key authentication · Run /login to sign in with your claude.ai account",
        "Login: Expired — log in again",
        "authentication_failed",
    ];

    const CODEX_SIGN_IN_FAILURES: &[&str] = &[
        "Not logged in",
        "unexpected status 401 Unauthorized: Missing bearer or basic authentication in header, url: https://api.openai.com/v1/responses, cf-ray: a3f7cff71a08d9b6-AKL, request id: req_2046135c3c3749399f7bb5af5ca52c6e",
        "workspace routing discovery unauthorized (401)",
        "Your access token could not be refreshed because your refresh token has expired. Please log out and sign in again.",
        "Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.",
        "Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again.",
        "Your access token could not be refreshed. Please log out and sign in again.",
        "Your access token could not be refreshed because you have since logged out or signed in to another account. Please sign in again.",
        "Your authentication session could not be refreshed automatically. Please log out and sign in again.",
        "ChatGPT login is required, but an API key is currently being used. Logging out.",
        "API key login is required, but ChatGPT is currently being used. Logging out.",
        "ChatGPT account ID not available, please re-run `codex login`",
        "remote exec-server registration requires ChatGPT authentication or API key authentication; run `codex login` or set CODEX_API_KEY",
    ];

    const OTHER_FAILURES: &[&str] = &[
        "Reconnecting... 1/5 (stream disconnected before completion: stream closed before response.completed)",
        "tool call error: tool call failed for `zone/echo`\n\nCaused by:\n    timed out awaiting tools/call after 2s",
        "MCP tool call requires approval, but approval policy is never",
        "Error: thread/start: thread/start failed: error creating thread: Fatal error: Failed to initialize session: required MCP servers failed to initialize: zone: Environment variable ZONE_MCP_TOKEN for MCP server 'zone' is not set (code -32603)",
        "Model metadata for `qwen2.5:7b-instruct` not found. Defaulting to fallback metadata; this can degrade performance and cause issues.",
        "Error logging in with device code: device code request failed with status 403 Forbidden",
        "Could not refresh your login because another Claude Code process is refreshing it (or exited mid-refresh) · Try again in a minute",
        "Authentication error · Try again",
        "rate limit reached (five_hour, rejected)",
        "You have hit your weekly limit",
        "agent command timed out after 1800 seconds",
        "agent command exited with status 1: the agent produced no diagnostics",
        "agent command claude could not be started: No such file or directory (os error 2)",
        "the agent exited without completing its event stream",
        "exceeded retry limit, last status: 500 Internal Server Error",
        "The model returned an empty response. Try again or choose another model.",
    ];

    /// Codex's own words when a turn fails for a token it could not renew,
    /// and what it wrote to stderr on the way.
    const CODEX_TURN_FAILED: &str =
        "workspace backend must use an HTTPS origin without credentials";
    const CODEX_STDERR: &str = "2026-09-23T07:43:43.341111Z ERROR codex_login::auth::manager: Failed to refresh token: Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again.";

    /// Something other than a sign-in, on stderr beside a failure that is not
    /// one either: a warning about another server the agent was talking to.
    const UNRELATED_STDERR: &str =
        "2026-09-23T07:43:43.341111Z WARN codex_mcp: docs: Not logged in (401 Unauthorized)";

    /// Stderr that names no sign-in at all.
    const RETRYING_STDERR: &str =
        "2026-09-23T07:43:43.341111Z WARN codex_api: stream disconnected; retrying";

    #[test]
    fn remedy_names_the_fix_for_every_recorded_sign_in_failure() {
        for (agent, failures) in [
            (AgentKind::Claude, CLAUDE_SIGN_IN_FAILURES),
            (AgentKind::Codex, CODEX_SIGN_IN_FAILURES),
        ] {
            for failure in failures {
                assert_eq!(
                    remedy(agent, SignIn::Organization, &reported(agent, failure)).as_deref(),
                    Some(REMEDY),
                    "{agent}: {failure}"
                );
            }
        }
    }

    #[test]
    fn remedy_leaves_every_other_failure_alone() {
        for agent in AgentKind::ALL {
            for sign_in in [SignIn::Instance, SignIn::Organization] {
                for failure in OTHER_FAILURES {
                    assert_eq!(
                        remedy(agent, sign_in, &reported(agent, failure)),
                        None,
                        "{agent}: {failure}"
                    );
                }
            }
        }
    }

    #[test]
    fn remedy_finds_a_codex_renewal_failure_only_in_its_stderr() {
        assert_eq!(
            remedy(
                AgentKind::Codex,
                SignIn::Organization,
                &reported(AgentKind::Codex, CODEX_TURN_FAILED)
            ),
            None,
            "the turn.failed line alone names no sign-in failure"
        );
        assert_eq!(
            remedy(
                AgentKind::Codex,
                SignIn::Organization,
                &reported(
                    AgentKind::Codex,
                    &format!("{CODEX_TURN_FAILED}{STDERR_HEADING}{CODEX_STDERR}")
                )
            )
            .as_deref(),
            Some(REMEDY)
        );
    }

    /// The stderr tail zone_core appends to an agent's failure is read for a
    /// renewal the agent reports nowhere else, and for nothing more: a line
    /// about some other server is not this agent's sign-in failing.
    #[test]
    fn a_sign_in_phrase_only_on_stderr_is_not_the_agents_sign_in() {
        for agent in AgentKind::ALL {
            let failure = reported(
                agent,
                &format!("stream disconnected before completion{STDERR_HEADING}{UNRELATED_STDERR}"),
            );

            assert_eq!(
                remedy(agent, SignIn::Organization, &failure),
                None,
                "{agent}: {failure}"
            );
        }
    }

    /// An agent's own report can run to several lines before its stderr
    /// begins, and a sign-in failure it names on any of them is its own.
    #[test]
    fn a_sign_in_failure_on_a_later_line_of_the_agents_words_is_its_own() {
        let failure = reported(
            AgentKind::Claude,
            &format!(
                "API Error: the request was rejected.\nNot logged in · Please run /login\
                 {STDERR_HEADING}{RETRYING_STDERR}"
            ),
        );

        assert_eq!(
            remedy(AgentKind::Claude, SignIn::Organization, &failure).as_deref(),
            Some(REMEDY),
            "{failure}"
        );
    }

    #[test]
    fn an_organizations_sign_in_failure_is_followed_by_its_remedy_once() {
        let backend = LlmBackend::cli(
            AgentKind::Claude,
            CliSettings::default().with_sign_in(SignIn::Organization),
        );
        let failure = reported(AgentKind::Claude, CLAUDE_SIGN_IN_FAILURES[0]);

        let explained = remedied(&backend, failure.clone());

        assert_eq!(
            explained,
            Remedied {
                message: format!("{failure}\n{REMEDY}"),
                signed_out: true,
            }
        );
        assert_eq!(
            remedied(&backend, explained.message.clone()),
            explained,
            "the remedy is given once"
        );
    }

    /// The instance's own agent runs under the host's login or a key in the
    /// server's environment, and no organization admin can renew either.
    #[test]
    fn the_servers_own_sign_in_failing_is_left_to_the_operator() {
        for agent in AgentKind::ALL {
            let backend = LlmBackend::cli(agent, CliSettings::default());
            let failure = reported(agent, "Not logged in");

            let explained = remedied(&backend, failure.clone());

            assert!(explained.signed_out, "{agent}: {}", explained.message);
            let fix = explained
                .message
                .strip_prefix(&failure)
                .unwrap_or_else(|| panic!("the failure comes first: {}", explained.message))
                .to_ascii_lowercase();
            assert!(!fix.contains("organization settings"), "{fix}");
            assert!(
                fix.contains("operator") && fix.contains("sign in again"),
                "{fix}"
            );
            assert!(fix.contains(agent.as_str()), "{fix}");
        }
    }

    #[test]
    fn a_bounded_agent_runs_for_the_budget_it_is_given_and_an_endpoint_is_left_alone() {
        let budget = Duration::from_secs(3600);
        let organizations = CliSettings::default().with_sign_in(SignIn::Organization);

        let LlmBackend::Cli { settings, .. } =
            bounded(LlmBackend::cli(AgentKind::Codex, organizations), budget)
        else {
            panic!("the agent was moved off its CLI");
        };

        assert_eq!(settings.timeout, budget);
        assert_eq!(
            settings.sign_in,
            SignIn::Organization,
            "only the timeout moves"
        );
        assert!(matches!(
            bounded(LlmBackend::Http, budget),
            LlmBackend::Http
        ));
    }

    #[test]
    fn only_a_cli_sign_in_failure_is_given_a_remedy() {
        let endpoint = "Failed to generate response: API error (401): invalid api key";
        assert_eq!(
            remedied(&LlmBackend::Http, endpoint.to_string()),
            Remedied {
                message: endpoint.to_string(),
                signed_out: false,
            },
            "an endpoint's rejected key is not the organization's sign-in"
        );

        let backend = LlmBackend::cli(AgentKind::Codex, CliSettings::default());
        let exhausted = reported(
            AgentKind::Codex,
            "exceeded retry limit, last status: 500 Internal Server Error",
        );
        assert_eq!(
            remedied(&backend, exhausted.clone()),
            Remedied {
                message: exhausted,
                signed_out: false,
            }
        );
    }
}
