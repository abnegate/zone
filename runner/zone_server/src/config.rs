//! Server configuration

use std::env;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use uuid::Uuid;
use zone_core::llm::{AgentKind, CodexSandbox};

/// Settings live with the clients that consume them.
pub use zone_comfy::Config as ComfyUiConfig;
pub use zone_search::WebSearchConfig;

/// Upstream GPT4All model catalog. Tests should override `Config::gpt4all_models_url`.
pub const DEFAULT_GPT4ALL_MODELS_URL: &str =
    "https://raw.githubusercontent.com/nomic-ai/gpt4all/main/gpt4all-chat/metadata/models3.json";

/// Upstream HuggingFace models API. Tests should override `Config::huggingface_models_url`.
pub const DEFAULT_HUGGINGFACE_MODELS_URL: &str = "https://huggingface.co/api/models";

/// GitHub's own REST origin, which `GITHUB_API_URL` overrides for GitHub
/// Enterprise. The origin answers only for repositories on the host it names,
/// so pointing it elsewhere does not redirect github.com repositories to it.
pub const DEFAULT_GITHUB_API_URL: &str = "https://api.github.com";

/// Where a Claude authorization code is exchanged and its tokens renewed.
pub const DEFAULT_CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";

/// Server configuration loaded from environment variables
#[derive(Clone)]
pub struct Config {
    /// Server host (default: 0.0.0.0)
    pub host: String,
    /// Server port (default: 8000)
    pub port: u16,
    /// Database URL
    pub database_url: String,
    /// Redis URL
    pub redis_url: String,
    /// JWT secret (must be at least 32 characters)
    pub jwt_secret: String,
    /// JWT access token lifetime in seconds (default: 900 = 15 minutes)
    pub jwt_access_lifetime: u64,
    /// JWT refresh token lifetime in seconds (default: 604800 = 7 days)
    pub jwt_refresh_lifetime: u64,
    /// The instance-wide default backend.
    pub model_backend: ModelBackend,
    /// Coding agent CLIs that organizations sign in to.
    pub agents: AgentConfig,
    /// LiteLLM host URL. Empty when a CLI backend serves completions.
    pub litellm_host: String,
    /// LiteLLM API key. Empty when a CLI backend serves completions.
    pub litellm_key: String,
    /// Ollama host URL (for model management)
    pub ollama_host: String,
    /// GPT4All browse catalog. Override in tests so CI does not hit GitHub raw.
    pub gpt4all_models_url: String,
    /// HuggingFace browse API. Override in tests so CI does not hit huggingface.co.
    pub huggingface_models_url: String,
    /// Optional HTTP proxy for remote model catalog searches.
    pub model_search_proxy_url: Option<String>,
    /// Encryption key for source credentials (must be at least 32 characters)
    pub encryption_key: String,
    /// Browser origins allowed to call the API, comma-separated. Entries may be
    /// full origins or bare hosts; a host also covers its subdomains.
    pub cors_origins: Vec<String>,
    /// CORS allow credentials (default: false)
    pub cors_allow_credentials: bool,
    /// Application base URL for email links (default: http://localhost:3000)
    pub app_base_url: String,
    /// Origin of the GitHub REST API. Configurable so a deployment can publish
    /// to GitHub Enterprise, and so the publication path can be exercised
    /// against something other than github.com.
    pub github_api_url: String,
    /// Live web search via SearXNG (through Gluetun when the VPN profile is up)
    pub web_search: WebSearchConfig,
    /// Direct ComfyUI image generation and artifact storage.
    pub comfyui: ComfyUiConfig,
    /// Background source reindex (schedule + change detection)
    pub source_index: SourceIndexConfig,
    /// Zone Prometheus / Grafana for on-call tools.
    pub monitoring: MonitoringConfig,
    /// Chat execution and context allocations.
    pub chat: crate::services::chat::session::Settings,
    /// Megabytes one LoRA training upload may carry. Training posts its images
    /// and clips inline as base64, and the whole body is held in memory while
    /// it is read, so the ceiling is a memory budget rather than a policy.
    pub train_upload_limit_mb: u64,
    /// Auto projects: what the driver admits, how it reviews, when it merges.
    pub auto: AutoProjectConfig,
}

/// The instance-wide default backend.
const MODEL_BACKEND: &str = "ZONE_LLM_BACKEND";

/// The binary a host agent backend runs, for one that is not on `PATH`.
const MODEL_BACKEND_EXECUTABLE: &str = "ZONE_LLM_BACKEND_EXECUTABLE";

/// Where a completion comes from by default.
///
/// The environment sets this default for the whole instance. Organizations
/// choose their own provider in AI settings, and one that chooses the
/// `claude_code` or `codex` provider runs that agent's CLI instead, under the
/// organization's own sign-in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ModelBackend {
    /// The OpenAI-compatible HTTP endpoint `LITELLM_HOST` names.
    #[default]
    LiteLlm,
    /// A coding agent CLI, run as a child process.
    Cli {
        agent: AgentKind,
        /// Overrides the agent's own executable name.
        executable: Option<PathBuf>,
    },
}

impl ModelBackend {
    /// Read the selection from the environment, defaulting to LiteLLM.
    pub fn from_env() -> Result<Self, ConfigError> {
        let executable = env::var(MODEL_BACKEND_EXECUTABLE)
            .ok()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);

        let selection = env::var(MODEL_BACKEND)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());

        let agent = match selection.as_deref() {
            None | Some("litellm") => None,
            Some("claude") => Some(AgentKind::Claude),
            Some("codex") => Some(AgentKind::Codex),
            Some(_) => {
                return Err(ConfigError::Invalid(
                    "ZONE_LLM_BACKEND must be litellm, claude or codex",
                ));
            }
        };

        match agent {
            Some(agent) => Ok(Self::Cli { agent, executable }),
            None if executable.is_some() => Err(ConfigError::Invalid(
                "ZONE_LLM_BACKEND_EXECUTABLE needs ZONE_LLM_BACKEND set to claude or codex",
            )),
            None => Ok(Self::LiteLlm),
        }
    }

    /// Whether `LITELLM_HOST` and `LITELLM_KEY` have to be configured. An
    /// agent already signed in on the host needs neither, and a self-host
    /// running one may have no LiteLLM deployed at all.
    pub fn requires_litellm(&self) -> bool {
        matches!(self, Self::LiteLlm)
    }
}

/// Root of every organization's agent homes.
const AGENT_STATE: &str = "ZONE_AGENT_STATE_DIR";

/// Whether an organization with no sign-in of its own may use the host's.
const AGENT_HOST_LOGIN: &str = "ZONE_AGENT_HOST_LOGIN";

/// Overrides [`DEFAULT_CLAUDE_TOKEN_URL`].
const CLAUDE_TOKEN_URL: &str = "ZONE_CLAUDE_TOKEN_URL";

/// Where codex runs the tools of a turn that grants them.
const CODEX_SANDBOX: &str = "ZONE_CODEX_SANDBOX";

/// The XDG base directory for user state.
const STATE_HOME: &str = "XDG_STATE_HOME";

const HOME: &str = "HOME";

/// Where the XDG base directory specification puts user state when
/// `XDG_STATE_HOME` is unset, relative to `HOME`.
const DEFAULT_STATE_HOME: &str = ".local/state";

/// The agent state root inside a user state directory.
const STATE_ROOT: &str = "zone/agents";

/// The state root [`AgentConfig::default`] keeps in the temporary directory.
const TEMPORARY_STATE_ROOT: &str = "zone-agents";

/// Each agent home's working directory, where every turn it serves runs.
const WORK: &str = "work";

const DEFAULT_HOST_LOGIN: bool = true;

/// Owner-only access, for directories holding an agent's credentials and
/// transcripts.
#[cfg(unix)]
const PRIVATE: u32 = 0o700;

/// Spellings a boolean setting accepts.
const TRUE: &[&str] = &["1", "true", "yes", "on"];
const FALSE: &[&str] = &["0", "false", "no", "off"];

/// The coding agent CLIs organizations sign in to, from
/// `ZONE_AGENT_STATE_DIR`, `ZONE_AGENT_HOST_LOGIN`, `ZONE_CLAUDE_TOKEN_URL`
/// and `ZONE_CODEX_SANDBOX`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentConfig {
    /// Root of every organization's agent homes.
    pub state: PathBuf,
    /// Whether an organization with no sign-in of its own falls back to the
    /// one the server's user has on the host.
    pub host_login: bool,
    /// Where a Claude authorization code is exchanged and its tokens renewed.
    pub claude_token_url: String,
    /// Where codex runs the tools of a turn that grants them.
    pub codex_sandbox: CodexSandbox,
}

impl Default for AgentConfig {
    /// A state root in the temporary directory, so a config that never read
    /// the environment writes into no one's home.
    fn default() -> Self {
        Self {
            state: env::temp_dir().join(TEMPORARY_STATE_ROOT),
            host_login: DEFAULT_HOST_LOGIN,
            claude_token_url: DEFAULT_CLAUDE_TOKEN_URL.to_string(),
            codex_sandbox: CodexSandbox::default(),
        }
    }
}

impl AgentConfig {
    /// Read the settings from the environment. The state root defaults to
    /// `$XDG_STATE_HOME/zone/agents`, then `$HOME/.local/state/zone/agents`.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            state: state_root(env_path(AGENT_STATE), env_path(STATE_HOME), env_path(HOME))?,
            host_login: host_login(env::var(AGENT_HOST_LOGIN).ok())?,
            claude_token_url: claude_token_url(env::var(CLAUDE_TOKEN_URL).ok())?,
            codex_sandbox: codex_sandbox(env::var(CODEX_SANDBOX).ok())?,
        })
    }

    /// `<state>/<organization>/<agent>`: the agent's own home for one
    /// organization.
    pub fn home(&self, organization: Uuid, agent: AgentKind) -> PathBuf {
        agent_home(&self.state, organization, agent)
    }

    /// `<state>/<organization>/<agent>/work`: where the agent's turns for one
    /// organization run.
    pub fn work(&self, organization: Uuid, agent: AgentKind) -> PathBuf {
        agent_work(&self.state, organization, agent)
    }

    /// Create the agent's home and its working directory, and return the home.
    ///
    /// Every directory this creates, the state root included, is private to
    /// the server's user, and the organization's own directories are made
    /// private again when they already exist. An existing state root keeps
    /// its mode, because it is the operator's directory and may be shared.
    pub fn create_home(&self, organization: Uuid, agent: AgentKind) -> io::Result<PathBuf> {
        let work = self.work(organization, agent);
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(PRIVATE);
        builder.create(&work)?;
        #[cfg(unix)]
        for directory in work
            .ancestors()
            .take_while(|directory| *directory != self.state.as_path())
        {
            fs::set_permissions(directory, fs::Permissions::from_mode(PRIVATE))?;
        }
        Ok(self.home(organization, agent))
    }
}

/// `<state>/<organization>/<agent>`. Below the operator's root every component
/// is a UUID or an agent's fixed name, so the path cannot leave that root.
pub fn agent_home(state: &Path, organization: Uuid, agent: AgentKind) -> PathBuf {
    state
        .join(organization.as_hyphenated().to_string())
        .join(agent.as_str())
}

/// `<state>/<organization>/<agent>/work`, confined the same way as
/// [`agent_home`].
pub fn agent_work(state: &Path, organization: Uuid, agent: AgentKind) -> PathBuf {
    agent_home(state, organization, agent).join(WORK)
}

/// The configured root, else the XDG state directory's, else the one under
/// `HOME`. A relative root would resolve against each agent's own working
/// directory, so none is accepted.
fn state_root(
    configured: Option<PathBuf>,
    state_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, ConfigError> {
    if let Some(configured) = configured {
        return if configured.is_absolute() {
            Ok(configured)
        } else {
            Err(ConfigError::Invalid(
                "ZONE_AGENT_STATE_DIR must be an absolute path",
            ))
        };
    }
    state_home
        .filter(|directory| directory.is_absolute())
        .or_else(|| {
            home.filter(|directory| directory.is_absolute())
                .map(|home| home.join(DEFAULT_STATE_HOME))
        })
        .map(|directory| directory.join(STATE_ROOT))
        .ok_or(ConfigError::Missing(AGENT_STATE))
}

/// A path from the environment, trimmed, with blank meaning unset.
fn env_path(name: &str) -> Option<PathBuf> {
    let path = match env::var_os(name)?.into_string() {
        Ok(text) => PathBuf::from(text.trim()),
        Err(raw) => PathBuf::from(raw),
    };
    (!path.as_os_str().is_empty()).then_some(path)
}

fn host_login(value: Option<String>) -> Result<bool, ConfigError> {
    let Some(value) = value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    else {
        return Ok(DEFAULT_HOST_LOGIN);
    };
    if TRUE.contains(&value.as_str()) {
        Ok(true)
    } else if FALSE.contains(&value.as_str()) {
        Ok(false)
    } else {
        Err(ConfigError::Invalid(
            "ZONE_AGENT_HOST_LOGIN must be true or false",
        ))
    }
}

fn codex_sandbox(value: Option<String>) -> Result<CodexSandbox, ConfigError> {
    match value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => Ok(CodexSandbox::default()),
        Some(value) => CodexSandbox::named(value).ok_or(ConfigError::Invalid(
            "ZONE_CODEX_SANDBOX must be workspace-write or danger-full-access",
        )),
    }
}

/// The configured token endpoint, else Anthropic's.
fn claude_token_url(value: Option<String>) -> Result<String, ConfigError> {
    let url = value
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_CLAUDE_TOKEN_URL.to_string());
    let parsed = reqwest::Url::parse(&url)
        .ok()
        .filter(|parsed| {
            matches!(parsed.scheme(), "http" | "https")
                && parsed.host_str().is_some_and(|host| !host.is_empty())
        })
        .ok_or(ConfigError::Invalid(
            "ZONE_CLAUDE_TOKEN_URL must be an absolute http or https URL with a host",
        ))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ConfigError::Invalid(
            "ZONE_CLAUDE_TOKEN_URL must not carry credentials, a query or a fragment",
        ));
    }
    Ok(url)
}

/// Periodic source indexing settings loaded from `SOURCE_RESYNC_*` env vars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIndexConfig {
    /// Master switch for the resync worker
    pub enabled: bool,
    /// How often to poll sources for remote changes
    pub poll_interval_secs: u64,
    /// How old a non-incremental source can be before it is fetched again
    pub interval_secs: u64,
}

impl Default for SourceIndexConfig {
    /// The defaults, before the environment is read.
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval_secs: 300,
            interval_secs: 3600,
        }
    }
}

impl SourceIndexConfig {
    /// Read the settings from the environment, falling back to the defaults.
    pub fn from_env() -> Self {
        Self {
            enabled: env_truthy("SOURCE_RESYNC_ENABLED", true),
            poll_interval_secs: env_u64("SOURCE_RESYNC_POLL_SECS", 300, 30, 86_400),
            interval_secs: env_u64("SOURCE_RESYNC_INTERVAL_SECS", 3600, 60, 7 * 86_400),
        }
    }
}

/// Auto projects, loaded from `ZONE_AUTO_*`: what the driver admits, how it
/// reviews a change, and when it merges one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoProjectConfig {
    /// Master switch for the driver.
    pub enabled: bool,
    /// How often the driver looks for projects to advance.
    pub tick_secs: u64,
    /// Tasks one project may have in flight at once.
    pub parallel_tasks: u64,
    /// Unattended runs across every project, so automation cannot starve a
    /// run somebody started by hand.
    pub max_active_runs: u64,
    /// Runs one task gets -- the first, fix-ups and retries -- before the
    /// project pauses on it.
    pub max_runs_per_task: u32,
    /// Review rounds one pull request gets before the project pauses on it.
    pub max_review_rounds: u32,
    /// Refuse to merge on a review by the model that wrote the change when no
    /// other model and no bot reviewed it.
    pub require_distinct_reviewer: bool,
    /// Models to review with, tried before the workspace's own settings.
    pub review_models: Vec<String>,
    /// Review bots to wait for and read; empty means every bot this build knows.
    pub review_bots: Vec<String>,
    /// How long to wait for an expected bot to review a new head.
    pub bot_review_grace_secs: u64,
    /// How long checks may stay silent on a head before they count as absent.
    pub checks_grace_secs: u64,
    /// How long checks may stay pending before the project pauses.
    pub checks_timeout_secs: u64,
    /// Retry a merge branch protection refused through the merge mutation an
    /// administrator can use.
    pub admin_merge: bool,
    /// Delete the branch once its pull request merged.
    pub delete_branch: bool,
    /// How long to watch the jobs a merge triggers on the base branch.
    pub post_merge_secs: u64,
    /// Fix tasks the driver may add to one project for jobs that failed after a merge.
    pub max_fix_tasks: u32,
}

impl Default for AutoProjectConfig {
    /// The defaults, before the environment is read.
    fn default() -> Self {
        Self {
            enabled: true,
            tick_secs: 15,
            parallel_tasks: 3,
            max_active_runs: 4,
            max_runs_per_task: 3,
            max_review_rounds: 4,
            require_distinct_reviewer: false,
            review_models: Vec::new(),
            review_bots: Vec::new(),
            bot_review_grace_secs: 600,
            checks_grace_secs: 120,
            checks_timeout_secs: 3600,
            admin_merge: true,
            delete_branch: true,
            post_merge_secs: 1800,
            max_fix_tasks: 5,
        }
    }
}

impl AutoProjectConfig {
    /// Read the settings from the environment, keeping every value inside its bounds.
    pub fn from_env() -> Self {
        Self {
            enabled: env_truthy("ZONE_AUTO_ENABLED", true),
            tick_secs: env_u64("ZONE_AUTO_TICK_SECS", 15, 5, 300),
            parallel_tasks: env_u64("ZONE_AUTO_PARALLEL_TASKS", 3, 1, 5),
            max_active_runs: env_u64("ZONE_AUTO_MAX_ACTIVE_RUNS", 4, 1, 5),
            max_runs_per_task: env_u64("ZONE_AUTO_MAX_RUNS_PER_TASK", 3, 1, 10) as u32,
            max_review_rounds: env_u64("ZONE_AUTO_MAX_REVIEW_ROUNDS", 4, 1, 10) as u32,
            require_distinct_reviewer: env_truthy("ZONE_AUTO_REVIEW_REQUIRE_DISTINCT_MODEL", false),
            review_models: env_list("ZONE_AUTO_REVIEW_MODELS"),
            review_bots: env_list("ZONE_AUTO_REVIEW_BOTS"),
            bot_review_grace_secs: env_u64("ZONE_AUTO_BOT_REVIEW_GRACE_SECS", 600, 60, 3600),
            checks_grace_secs: env_u64("ZONE_AUTO_CHECKS_GRACE_SECS", 120, 30, 1800),
            checks_timeout_secs: env_u64("ZONE_AUTO_CHECKS_TIMEOUT_SECS", 3600, 300, 86_400),
            admin_merge: env_truthy("ZONE_AUTO_ADMIN_MERGE", true),
            delete_branch: env_truthy("ZONE_AUTO_DELETE_BRANCH", true),
            post_merge_secs: env_u64("ZONE_AUTO_POST_MERGE_SECS", 1800, 0, 86_400),
            max_fix_tasks: env_u64("ZONE_AUTO_MAX_FIX_TASKS", 5, 0, 50) as u32,
        }
    }

    /// How often the driver looks for due projects.
    pub fn tick(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.tick_secs)
    }
}

/// A comma-separated list, trimmed, with blanks dropped.
fn env_list(name: &str) -> Vec<String> {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Live cluster metrics and dashboards loaded from `MONITORING_*`.
#[derive(Clone, PartialEq, Eq)]
pub struct MonitoringConfig {
    pub enabled: bool,
    pub prometheus_url: String,
    pub grafana_url: String,
    pub grafana_token: Option<String>,
    pub grafana_user: Option<String>,
    pub grafana_password: Option<String>,
}

impl Default for MonitoringConfig {
    /// The defaults, before the environment is read.
    fn default() -> Self {
        Self {
            enabled: false,
            prometheus_url: "http://prometheus:9090".to_string(),
            grafana_url: "http://grafana:3000".to_string(),
            grafana_token: None,
            grafana_user: None,
            grafana_password: None,
        }
    }
}

impl std::fmt::Debug for MonitoringConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitoringConfig")
            .field("enabled", &self.enabled)
            .field("prometheus_url", &self.prometheus_url)
            .field("grafana_url", &self.grafana_url)
            .field(
                "grafana_token",
                &self.grafana_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("grafana_user", &self.grafana_user)
            .field(
                "grafana_secret",
                &self.grafana_password.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl MonitoringConfig {
    /// Read the settings from the environment, falling back to the defaults.
    pub fn from_env() -> Self {
        Self {
            enabled: env_truthy("MONITORING_ENABLED", true),
            prometheus_url: env::var("MONITORING_PROMETHEUS_URL")
                .or_else(|_| env::var("PROMETHEUS_URL"))
                .unwrap_or_else(|_| "http://prometheus:9090".to_string())
                .trim_end_matches('/')
                .to_string(),
            grafana_url: env::var("MONITORING_GRAFANA_URL")
                .or_else(|_| env::var("GRAFANA_URL"))
                .unwrap_or_else(|_| "http://grafana:3000".to_string())
                .trim_end_matches('/')
                .to_string(),
            grafana_token: env::var("MONITORING_GRAFANA_TOKEN")
                .or_else(|_| env::var("GRAFANA_TOKEN"))
                .ok()
                .filter(|token| !token.trim().is_empty()),
            grafana_user: env::var("MONITORING_GRAFANA_ADMIN_USER")
                .ok()
                .filter(|user| !user.trim().is_empty()),
            grafana_password: env::var("MONITORING_GRAFANA_ADMIN_PASSWORD")
                .ok()
                .filter(|password| !password.trim().is_empty()),
        }
    }
}

/// Base domain the browser-facing subdomains hang off, shared with Traefik.
const DOMAIN_HOST: &str = "DOMAIN_HOST_WEBUI";

/// Development hosts that stay reachable with no configuration at all.
///
/// Matched by equality, never by prefix: `localhost.attacker.com` starts with
/// `localhost` and is not loopback.
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Browser origins the API answers, credentials included.
///
/// The match is on the origin's host, and an allowed host must be a configured
/// host or a subdomain of one. A substring test would admit
/// `evil-zone.attacker.com` for a `zone.` deployment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AllowedOrigins {
    hosts: Vec<String>,
}

impl AllowedOrigins {
    /// `CORS_ORIGINS` plus the deployment's base domain. With neither set this
    /// admits loopback only, so a deployment that forgot to configure the
    /// console fails closed rather than open.
    pub fn from_config(config: &Config) -> Self {
        Self::new(
            config
                .cors_origins
                .iter()
                .cloned()
                .chain(env::var(DOMAIN_HOST).ok()),
        )
    }

    pub fn new(entries: impl IntoIterator<Item = String>) -> Self {
        let mut hosts: Vec<String> = entries
            .into_iter()
            .filter_map(|entry| configured_host(&entry))
            .collect();
        hosts.sort();
        hosts.dedup();
        Self { hosts }
    }

    /// Whether an `Origin` header value may call the API.
    pub fn allows(&self, origin: &str) -> bool {
        let Some(host) = origin_host(origin) else {
            return false;
        };
        LOOPBACK_HOSTS.contains(&host.as_str())
            || self.hosts.iter().any(|allowed| within(&host, allowed))
    }
}

/// A configured entry, written either as an origin or as a bare host.
fn configured_host(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry == "*" {
        return None;
    }
    if entry.contains("://") {
        return origin_host(entry);
    }
    origin_host(&format!("https://{entry}"))
}

/// The lowercased host of a `scheme://host[:port]` origin.
fn origin_host(origin: &str) -> Option<String> {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))?;
    if rest.is_empty()
        || rest.contains('/')
        || rest.contains('@')
        || rest.contains(char::is_whitespace)
    {
        return None;
    }
    let host = match rest.strip_prefix('[') {
        Some(tail) => {
            let (inside, after) = tail.split_once(']')?;
            match after.strip_prefix(':') {
                Some(port) if is_port(port) => inside,
                None if after.is_empty() => inside,
                _ => return None,
            }
        }
        None => match rest.split_once(':') {
            Some((host, port)) if is_port(port) => host,
            Some(_) => return None,
            None => rest,
        },
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn is_port(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

/// Equal to the allowed host, or a subdomain of it on a label boundary.
/// Whether a host is the allowed host or a subdomain of it.
///
/// The prefix has to be whole labels, not merely something ending in a dot:
/// `.zone.example.com` strips to a bare `"."`, which satisfies "ends with a
/// dot" while naming an empty label.
fn within(host: &str, allowed: &str) -> bool {
    if host == allowed {
        return true;
    }
    host.strip_suffix(allowed)
        .and_then(|prefix| prefix.strip_suffix('.'))
        .is_some_and(|labels| {
            !labels.is_empty() && labels.split('.').all(|label| !label.is_empty())
        })
}

fn env_truthy(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => TRUE.contains(&value.to_ascii_lowercase().as_str()),
        Err(_) => default,
    }
}

fn env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

impl Config {
    /// Get the JWT secret
    pub fn jwt_secret(&self) -> &str {
        &self.jwt_secret
    }

    /// Get the access token lifetime as a chrono Duration
    pub fn access_token_lifetime(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.jwt_access_lifetime as i64)
    }

    /// Get the refresh token lifetime as a chrono Duration
    pub fn refresh_token_lifetime(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.jwt_refresh_lifetime as i64)
    }

    /// Get the encryption key
    pub fn encryption_key(&self) -> &str {
        &self.encryption_key
    }

    /// Which backend serves completions
    pub fn model_backend(&self) -> &ModelBackend {
        &self.model_backend
    }

    /// The binary `agent` runs from: the one `ZONE_LLM_BACKEND_EXECUTABLE`
    /// names when the environment selected this same agent, else the agent's
    /// own name, looked up on `PATH`.
    pub fn agent_executable(&self, agent: AgentKind) -> PathBuf {
        match &self.model_backend {
            ModelBackend::Cli {
                agent: selected,
                executable: Some(executable),
            } if *selected == agent => executable.clone(),
            _ => PathBuf::from(agent.executable()),
        }
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, ConfigError> {
        let jwt_secret = env::var("JWT_SECRET").map_err(|_| ConfigError::Missing("JWT_SECRET"))?;
        if jwt_secret.len() < 32 {
            return Err(ConfigError::Invalid(
                "JWT_SECRET must be at least 32 characters",
            ));
        }

        let encryption_key =
            env::var("ENCRYPTION_KEY").map_err(|_| ConfigError::Missing("ENCRYPTION_KEY"))?;
        if encryption_key.len() < 32 {
            return Err(ConfigError::Invalid(
                "ENCRYPTION_KEY must be at least 32 characters",
            ));
        }

        let cors_origins = env::var("CORS_ORIGINS")
            .ok()
            .filter(|list| !list.trim().is_empty())
            .map(|list| {
                list.split(',')
                    .map(|origin| origin.trim().to_string())
                    .filter(|origin| !origin.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec!["*".to_string()]);

        let cors_allow_credentials = env::var("CORS_ALLOW_CREDENTIALS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);

        let app_base_url =
            env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

        let github_api_url = env::var("GITHUB_API_URL")
            .ok()
            .map(|url| url.trim().trim_end_matches('/').to_string())
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| DEFAULT_GITHUB_API_URL.to_string());
        // A prefix is not an origin: `https:///api/v3` passes one and leaves
        // PrService building request URLs with no host to send them to. Parsing
        // also refuses embedded credentials, which would otherwise reach any
        // log that prints this config.
        match reqwest::Url::parse(&github_api_url) {
            Ok(url)
                if matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some_and(|host| !host.is_empty()) =>
            {
                if !url.username().is_empty() || url.password().is_some() {
                    return Err(ConfigError::Invalid(
                        "GITHUB_API_URL must not carry credentials; set GITHUB_TOKEN instead",
                    ));
                }
                // An origin every caller joins a rooted path onto has no use
                // for either, and both are places a token gets written down.
                if url.query().is_some() || url.fragment().is_some() {
                    return Err(ConfigError::Invalid(
                        "GITHUB_API_URL must be an origin, without a query or fragment",
                    ));
                }
            }
            _ => {
                return Err(ConfigError::Invalid(
                    "GITHUB_API_URL must be an absolute http or https URL with a host",
                ));
            }
        }

        let model_backend = ModelBackend::from_env()?;
        let litellm_host = env::var("LITELLM_HOST").ok();
        let litellm_key = env::var("LITELLM_KEY").ok();
        if model_backend.requires_litellm() {
            if litellm_host.is_none() {
                return Err(ConfigError::Missing("LITELLM_HOST"));
            }
            if litellm_key.is_none() {
                return Err(ConfigError::Missing("LITELLM_KEY"));
            }
        }

        Ok(Self {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000),
            database_url: env::var("DATABASE_URL")
                .map_err(|_| ConfigError::Missing("DATABASE_URL"))?,
            redis_url: env::var("REDIS_URL").map_err(|_| ConfigError::Missing("REDIS_URL"))?,
            jwt_secret,
            jwt_access_lifetime: env::var("JWT_ACCESS_LIFETIME")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            jwt_refresh_lifetime: env::var("JWT_REFRESH_LIFETIME")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(604_800),
            model_backend,
            agents: AgentConfig::from_env()?,
            litellm_host: litellm_host.unwrap_or_default(),
            litellm_key: litellm_key.unwrap_or_default(),
            ollama_host: env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://ollama:11434".to_string()),
            gpt4all_models_url: env::var("GPT4ALL_MODELS_URL")
                .unwrap_or_else(|_| DEFAULT_GPT4ALL_MODELS_URL.to_string()),
            huggingface_models_url: env::var("HUGGINGFACE_MODELS_URL")
                .unwrap_or_else(|_| DEFAULT_HUGGINGFACE_MODELS_URL.to_string()),
            model_search_proxy_url: env::var("MODEL_SEARCH_PROXY_URL")
                .ok()
                .map(|url| url.trim().to_string())
                .filter(|url| !url.is_empty()),
            encryption_key,
            cors_origins,
            cors_allow_credentials,
            app_base_url,
            github_api_url,
            web_search: WebSearchConfig::from_env(),
            comfyui: ComfyUiConfig::from_env(),
            source_index: SourceIndexConfig::from_env(),
            monitoring: MonitoringConfig::from_env(),
            train_upload_limit_mb: env_u64("TRAIN_UPLOAD_LIMIT_MB", 512, 4, 8192),
            auto: AutoProjectConfig::from_env(),
            chat: crate::services::chat::session::Settings::from_env().map_err(|_| {
                ConfigError::Invalid(
                    "ZONE_CHAT_* settings must be positive integers within the supported range",
                )
            })?,
        })
    }
}

/// Custom Debug implementation that redacts sensitive fields
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database_url", &"[REDACTED]")
            .field("redis_url", &"[REDACTED]")
            .field("jwt_secret", &"[REDACTED]")
            .field("jwt_access_lifetime", &self.jwt_access_lifetime)
            .field("jwt_refresh_lifetime", &self.jwt_refresh_lifetime)
            .field("model_backend", &self.model_backend)
            .field("agents", &self.agents)
            .field("litellm_host", &self.litellm_host)
            .field("litellm_key", &"[REDACTED]")
            .field("ollama_host", &self.ollama_host)
            .field("gpt4all_models_url", &self.gpt4all_models_url)
            .field("huggingface_models_url", &self.huggingface_models_url)
            .field(
                "model_search_proxy_url",
                &self.model_search_proxy_url.as_ref().map(|_| "[configured]"),
            )
            .field("encryption_key", &"[REDACTED]")
            .field("cors_origins", &self.cors_origins)
            .field("cors_allow_credentials", &self.cors_allow_credentials)
            .field("app_base_url", &self.app_base_url)
            .field("github_api_url", &self.github_api_url)
            .field("web_search", &self.web_search)
            .field("comfyui", &self.comfyui)
            .field("source_index", &self.source_index)
            .field("monitoring", &self.monitoring)
            .field("auto", &self.auto)
            .finish()
    }
}

/// Configuration error
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    Missing(&'static str),
    #[error("Invalid configuration: {0}")]
    Invalid(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

    static ENVIRONMENT: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    /// Serialises the tests that change the process environment. A test that
    /// panics holding it has already had its variables restored, because its
    /// `Environment` is declared after the guard and so dropped first.
    fn lock() -> MutexGuard<'static, ()> {
        ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
    }

    struct Environment(Vec<(&'static str, Option<OsString>)>);

    impl Environment {
        fn isolated(names: &[&'static str]) -> Self {
            let values = names
                .iter()
                .map(|name| (*name, env::var_os(name)))
                .collect::<Vec<_>>();
            for name in names {
                // SAFETY: every environment-mutating test in this module holds
                // ENVIRONMENT for the guard's lifetime.
                unsafe { env::remove_var(name) };
            }
            Self(values)
        }

        fn set(name: &'static str, value: &str) {
            // SAFETY: every environment-mutating test in this module holds
            // ENVIRONMENT for the duration of the mutation.
            unsafe { env::set_var(name, value) };
        }

        fn remove(name: &'static str) {
            // SAFETY: every environment-mutating test in this module holds
            // ENVIRONMENT for the duration of the mutation.
            unsafe { env::remove_var(name) };
        }
    }

    impl Drop for Environment {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                // SAFETY: the caller still holds ENVIRONMENT while the saved
                // process environment is restored.
                unsafe {
                    match value {
                        Some(value) => env::set_var(name, value),
                        None => env::remove_var(name),
                    }
                }
            }
        }
    }

    fn create_test_config() -> Config {
        Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://localhost/test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "test-secret-key-with-at-least-32-chars".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            model_backend: ModelBackend::default(),
            agents: AgentConfig::default(),
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "test-key".to_string(),
            ollama_host: "http://localhost:11434".to_string(),
            gpt4all_models_url: DEFAULT_GPT4ALL_MODELS_URL.to_string(),
            huggingface_models_url: DEFAULT_HUGGINGFACE_MODELS_URL.to_string(),
            model_search_proxy_url: None,
            encryption_key: "12345678901234567890123456789012".to_string(),
            cors_origins: vec!["*".to_string()],
            cors_allow_credentials: false,
            app_base_url: "http://localhost:3000".to_string(),
            github_api_url: DEFAULT_GITHUB_API_URL.to_string(),
            web_search: WebSearchConfig::default(),
            comfyui: ComfyUiConfig::default(),
            source_index: SourceIndexConfig::default(),
            monitoring: MonitoringConfig::default(),
            chat: Default::default(),
            train_upload_limit_mb: 512,
            auto: Default::default(),
        }
    }

    #[test]
    fn test_config_jwt_secret_getter() {
        let config = create_test_config();
        assert_eq!(
            config.jwt_secret(),
            "test-secret-key-with-at-least-32-chars"
        );
    }

    #[test]
    fn test_config_access_token_lifetime() {
        let mut config = create_test_config();
        config.jwt_access_lifetime = 1800;

        let lifetime = config.access_token_lifetime();
        assert_eq!(lifetime.num_seconds(), 1800);
    }

    #[test]
    fn test_config_refresh_token_lifetime() {
        let mut config = create_test_config();
        config.jwt_refresh_lifetime = 86400;

        let lifetime = config.refresh_token_lifetime();
        assert_eq!(lifetime.num_seconds(), 86400);
    }

    #[test]
    fn test_config_error_display() {
        let missing_err = ConfigError::Missing("TEST_VAR");
        assert_eq!(
            missing_err.to_string(),
            "Missing required environment variable: TEST_VAR"
        );

        let invalid_err = ConfigError::Invalid("test error");
        assert_eq!(invalid_err.to_string(), "Invalid configuration: test error");
    }

    #[test]
    fn test_config_clone() {
        let config = create_test_config();

        let cloned = config.clone();
        assert_eq!(config.host, cloned.host);
        assert_eq!(config.database_url, cloned.database_url);
        assert_eq!(config.port, cloned.port);
        assert_eq!(config.jwt_secret, cloned.jwt_secret);
    }

    #[test]
    fn test_config_debug_redacts_secrets() {
        let mut config = create_test_config();
        config.database_url = "postgres://user:password@localhost/test".to_string();
        config.redis_url = "redis://:secret@localhost:6379".to_string();
        config.jwt_secret = "my-super-secret-key".to_string();
        config.litellm_key = "sk-secret-api-key".to_string();
        config.model_search_proxy_url =
            Some("http://proxy-user:proxy-password@proxy.example:8888".to_string());

        let debug_str = format!("{:?}", config);

        // Should contain non-sensitive fields
        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("localhost")); // host is fine
        assert!(debug_str.contains("8000")); // port is fine

        // Should NOT contain sensitive values
        assert!(!debug_str.contains("password"));
        assert!(!debug_str.contains("my-super-secret-key"));
        assert!(!debug_str.contains("sk-secret-api-key"));
        assert!(!debug_str.contains("user:password"));
        assert!(!debug_str.contains("proxy-password"));

        // Should contain [REDACTED] placeholders
        assert!(debug_str.contains("[REDACTED]"));
        assert!(debug_str.contains("model_search_proxy_url: Some(\"[configured]\")"));
    }

    #[test]
    fn test_config_default_port() {
        let config = create_test_config();

        // Default port is 8000
        assert_eq!(config.port, 8000);
    }

    #[test]
    fn test_config_default_lifetimes() {
        // Default access token lifetime is 900 seconds (15 minutes)
        // Default refresh token lifetime is 604800 seconds (7 days)
        let config = create_test_config();

        assert_eq!(config.jwt_access_lifetime, 900);
        assert_eq!(config.jwt_refresh_lifetime, 604800);
    }

    #[test]
    fn test_config_lifetime_methods() {
        let mut config = create_test_config();
        config.jwt_access_lifetime = 3600; // 1 hour
        config.jwt_refresh_lifetime = 86400; // 1 day

        let access_duration = config.access_token_lifetime();
        let refresh_duration = config.refresh_token_lifetime();

        assert_eq!(access_duration.num_minutes(), 60);
        assert_eq!(refresh_duration.num_hours(), 24);
    }

    #[test]
    fn test_config_all_fields() {
        let mut config = create_test_config();
        config.host = "127.0.0.1".to_string();
        config.port = 3000;
        config.database_url = "postgres://user:pass@localhost/db".to_string();
        config.redis_url = "redis://:password@localhost:6379/0".to_string();
        config.jwt_secret = "super-secret-key-for-jwt-signing".to_string();
        config.jwt_access_lifetime = 1800;
        config.jwt_refresh_lifetime = 86400;
        config.litellm_host = "http://litellm.local:4000".to_string();
        config.litellm_key = "sk-litellm-key".to_string();

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert_eq!(config.database_url, "postgres://user:pass@localhost/db");
        assert_eq!(config.redis_url, "redis://:password@localhost:6379/0");
        assert_eq!(config.jwt_secret, "super-secret-key-for-jwt-signing");
        assert_eq!(config.jwt_access_lifetime, 1800);
        assert_eq!(config.jwt_refresh_lifetime, 86400);
        assert_eq!(config.litellm_host, "http://litellm.local:4000");
        assert_eq!(config.litellm_key, "sk-litellm-key");
    }

    #[test]
    fn test_config_error_variants() {
        // Test that error variants exist and display correctly
        let missing = ConfigError::Missing("VAR_NAME");
        assert!(missing.to_string().contains("Missing"));
        assert!(missing.to_string().contains("VAR_NAME"));

        let invalid = ConfigError::Invalid("reason here");
        assert!(invalid.to_string().contains("Invalid"));
        assert!(invalid.to_string().contains("reason here"));
    }

    fn allowed(entries: &[&str]) -> AllowedOrigins {
        AllowedOrigins::new(entries.iter().map(|entry| entry.to_string()))
    }

    /// `PrService` carries a configurable-origin constructor and the App issuer an
    /// `at` one, both documented for GitHub Enterprise, and neither was
    /// reachable from configuration -- every production caller hardcoded
    /// github.com. That left Enterprise unusable and the publication path
    /// impossible to exercise against anything but the real API.
    #[test]
    fn the_github_origin_is_configurable_and_must_be_absolute() {
        let _lock = lock();
        let names = [
            "DATABASE_URL",
            "ENCRYPTION_KEY",
            "GITHUB_API_URL",
            "JWT_SECRET",
            "LITELLM_HOST",
            "LITELLM_KEY",
            "REDIS_URL",
            MODEL_BACKEND,
            MODEL_BACKEND_EXECUTABLE,
            AGENT_STATE,
            AGENT_HOST_LOGIN,
            CLAUDE_TOKEN_URL,
            CODEX_SANDBOX,
        ];
        let _environment = Environment::isolated(&names);
        Environment::set("JWT_SECRET", "12345678901234567890123456789012");
        Environment::set("ENCRYPTION_KEY", "12345678901234567890123456789012");
        Environment::set("DATABASE_URL", "postgres://localhost/zone");
        Environment::set("REDIS_URL", "redis://localhost");
        Environment::set("LITELLM_HOST", "http://localhost:4000");
        Environment::set("LITELLM_KEY", "key");

        assert_eq!(
            Config::from_env().expect("unset falls back").github_api_url,
            DEFAULT_GITHUB_API_URL
        );

        Environment::set("GITHUB_API_URL", "https://github.example.com/api/v3/");
        assert_eq!(
            Config::from_env()
                .expect("an enterprise origin is valid")
                .github_api_url,
            "https://github.example.com/api/v3",
            "the trailing slash goes, because every caller joins a rooted path"
        );

        Environment::set("GITHUB_API_URL", "   ");
        assert_eq!(
            Config::from_env()
                .expect("blank is not a setting")
                .github_api_url,
            DEFAULT_GITHUB_API_URL
        );

        Environment::set("GITHUB_API_URL", "github.example.com");
        assert!(
            matches!(
                Config::from_env(),
                Err(ConfigError::Invalid(
                    "GITHUB_API_URL must be an absolute http or https URL with a host"
                ))
            ),
            "a scheme-less origin would produce relative request URLs"
        );

        Environment::set("GITHUB_API_URL", "ftp://github.example.com");
        assert!(
            matches!(
                Config::from_env(),
                Err(ConfigError::Invalid(
                    "GITHUB_API_URL must be an absolute http or https URL with a host"
                ))
            ),
            "a scheme this client cannot speak is not an origin"
        );

        Environment::set("GITHUB_API_URL", "https://someone:t0ken@github.example.com");
        assert!(
            matches!(
                Config::from_env(),
                Err(ConfigError::Invalid(
                    "GITHUB_API_URL must not carry credentials; set GITHUB_TOKEN instead"
                ))
            ),
            "credentials in the origin reach every log that prints the config"
        );

        for carrier in [
            "https://github.example.com/api/v3?access_token=t0ken",
            "https://github.example.com/api/v3#t0ken",
        ] {
            Environment::set("GITHUB_API_URL", carrier);
            assert!(
                matches!(
                    Config::from_env(),
                    Err(ConfigError::Invalid(
                        "GITHUB_API_URL must be an origin, without a query or fragment"
                    ))
                ),
                "{carrier} would reach every log that prints the config"
            );
        }

        Environment::set("GITHUB_API_URL", "http://localhost:3000");
        assert_eq!(
            Config::from_env()
                .expect("an operator may point this at a host they run")
                .github_api_url,
            "http://localhost:3000",
            "http stays configurable: the origin is operator configuration, and \
             it is routinely a loopback or LAN host"
        );
    }

    #[test]
    fn a_subdomain_prefix_must_be_whole_labels() {
        let origins = allowed(&["https://zone.example.com"]);
        for origin in [
            "https://.zone.example.com",
            "https://a..zone.example.com",
            "https://..zone.example.com",
        ] {
            assert!(
                !origins.allows(origin),
                "{origin} names an empty label and must be rejected"
            );
        }
        for origin in [
            "https://zone.example.com",
            "https://manager.zone.example.com",
            "https://a.b.zone.example.com",
        ] {
            assert!(origins.allows(origin), "{origin} must still be allowed");
        }
    }

    #[test]
    fn allowed_origins_reject_hosts_that_merely_contain_a_configured_one() {
        let origins = allowed(&["https://zone.example.com", "manager.example.com"]);
        for origin in [
            "https://evil-zone.attacker.com",
            "https://manager.attacker.com",
            "https://zone.example.com.attacker.com",
            "https://attacker.com/?x=https://zone.example.com",
            "https://notzone.example.com",
            "https://zone.example.como",
        ] {
            assert!(!origins.allows(origin), "{origin} must be rejected");
        }
    }

    #[test]
    fn allowed_origins_accept_the_configured_host_and_its_subdomains() {
        let origins = allowed(&["https://zone.example.com"]);
        for origin in [
            "https://zone.example.com",
            "http://zone.example.com",
            "https://zone.example.com:8443",
            "https://manager.zone.example.com",
            "https://ZONE.example.com",
        ] {
            assert!(origins.allows(origin), "{origin} must be accepted");
        }
    }

    #[test]
    fn allowed_origins_scope_loopback_to_the_loopback_hosts() {
        let origins = AllowedOrigins::default();
        for origin in [
            "http://localhost",
            "http://localhost:3001",
            "https://127.0.0.1:8000",
            "http://[::1]:5173",
        ] {
            assert!(origins.allows(origin), "{origin} must be accepted");
        }
        for origin in [
            "http://localhost.attacker.com",
            "https://127.0.0.1.attacker.com",
            "http://sub.localhost",
            "http://evil.127.0.0.1",
        ] {
            assert!(!origins.allows(origin), "{origin} must be rejected");
        }
    }

    #[test]
    fn allowed_origins_fail_closed_without_configuration() {
        let origins = allowed(&["*", "", "   "]);
        assert_eq!(origins, AllowedOrigins::default());
        assert!(origins.allows("http://localhost:3001"));
        assert!(!origins.allows("https://anything.example.com"));
    }

    #[test]
    fn allowed_origins_reject_values_that_are_not_plain_origins() {
        let origins = allowed(&["zone.example.com"]);
        for origin in [
            "null",
            "",
            "zone.example.com",
            "file://zone.example.com",
            "https://",
            "https://user@zone.example.com",
            "https://zone.example.com/path",
            "https://zone.example.com:notaport",
            "https://zone.example.com evil.com",
        ] {
            assert!(!origins.allows(origin), "{origin} must be rejected");
        }
    }

    /// The environment picks the default every organization without an agent
    /// provider of its own falls back to.
    #[test]
    fn the_model_backend_is_selected_by_the_environment() {
        let _lock = lock();
        let _environment = Environment::isolated(&[MODEL_BACKEND, MODEL_BACKEND_EXECUTABLE]);

        assert_eq!(
            ModelBackend::from_env().expect("unset falls back"),
            ModelBackend::LiteLlm,
            "a deployment that configures nothing keeps paying for the HTTP API"
        );

        for (value, agent) in [
            ("claude", AgentKind::Claude),
            ("codex", AgentKind::Codex),
            ("  CODEX  ", AgentKind::Codex),
        ] {
            Environment::set(MODEL_BACKEND, value);
            assert_eq!(
                ModelBackend::from_env().expect("a known agent"),
                ModelBackend::Cli {
                    agent,
                    executable: None
                },
                "{value} names a CLI this build drives"
            );
        }

        Environment::set(MODEL_BACKEND, "litellm");
        assert_eq!(
            ModelBackend::from_env().expect("the HTTP backend, named"),
            ModelBackend::LiteLlm
        );

        Environment::set(MODEL_BACKEND, "gemini");
        assert!(
            matches!(
                ModelBackend::from_env(),
                Err(ConfigError::Invalid(
                    "ZONE_LLM_BACKEND must be litellm, claude or codex"
                ))
            ),
            "an unrecognised backend must be refused, not silently ignored"
        );

        Environment::set(MODEL_BACKEND, "claude");
        Environment::set(MODEL_BACKEND_EXECUTABLE, "  /opt/homebrew/bin/claude  ");
        assert_eq!(
            ModelBackend::from_env().expect("a binary that is not on PATH"),
            ModelBackend::Cli {
                agent: AgentKind::Claude,
                executable: Some(PathBuf::from("/opt/homebrew/bin/claude")),
            }
        );

        Environment::set(MODEL_BACKEND, "litellm");
        assert!(
            matches!(
                ModelBackend::from_env(),
                Err(ConfigError::Invalid(
                    "ZONE_LLM_BACKEND_EXECUTABLE needs ZONE_LLM_BACKEND set to claude or codex"
                ))
            ),
            "an executable with no CLI backend to run it means the operator \
             believes a CLI is in use while the HTTP API is being billed"
        );

        Environment::set(MODEL_BACKEND, "codex");
        Environment::set(MODEL_BACKEND_EXECUTABLE, "   ");
        assert_eq!(
            ModelBackend::from_env().expect("blank is not a path"),
            ModelBackend::Cli {
                agent: AgentKind::Codex,
                executable: None
            }
        );
    }

    /// A single-user self-host pointing Zone at the CLI it is already signed
    /// in to has no LiteLLM to name, and refusing to boot without one left
    /// that deployment impossible.
    #[test]
    fn litellm_is_required_only_by_the_http_backend() {
        let _lock = lock();
        let names = [
            "DATABASE_URL",
            "ENCRYPTION_KEY",
            "GITHUB_API_URL",
            "JWT_SECRET",
            "LITELLM_HOST",
            "LITELLM_KEY",
            "REDIS_URL",
            MODEL_BACKEND,
            MODEL_BACKEND_EXECUTABLE,
            AGENT_STATE,
            AGENT_HOST_LOGIN,
            CLAUDE_TOKEN_URL,
            CODEX_SANDBOX,
        ];
        let _environment = Environment::isolated(&names);
        Environment::set("JWT_SECRET", "12345678901234567890123456789012");
        Environment::set("ENCRYPTION_KEY", "12345678901234567890123456789012");
        Environment::set("DATABASE_URL", "postgres://database/zone");
        Environment::set("REDIS_URL", "redis://cache:6379");

        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Missing("LITELLM_HOST"))
        ));
        Environment::set("LITELLM_HOST", "http://models:4000");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Missing("LITELLM_KEY"))
        ));
        Environment::set("LITELLM_KEY", "models-key");
        assert_eq!(
            Config::from_env()
                .expect("the HTTP backend is configured")
                .model_backend(),
            &ModelBackend::LiteLlm
        );

        Environment::remove("LITELLM_HOST");
        Environment::remove("LITELLM_KEY");
        Environment::set(MODEL_BACKEND, "claude");
        let cli = Config::from_env().expect("a CLI backend needs no LiteLLM");
        assert_eq!(
            cli.model_backend(),
            &ModelBackend::Cli {
                agent: AgentKind::Claude,
                executable: None
            }
        );
        assert!(cli.litellm_host.is_empty());
        assert!(cli.litellm_key.is_empty());
        assert!(format!("{cli:?}").contains("model_backend: Cli"));
    }

    #[test]
    fn test_web_search_default_is_off_for_tests() {
        let config = WebSearchConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.query_url, zone_search::DEFAULT_SEARXNG_QUERY_URL);
        assert_eq!(config.result_count, 5);
        assert!(!config.requested_for("hello", None));
    }

    #[test]
    fn test_web_search_requested_for_respects_metadata_and_intent() {
        let config = WebSearchConfig {
            enabled: true,
            ..WebSearchConfig::default()
        };
        assert!(!config.requested_for("Explain this function", None));
        assert!(config.requested_for("What is the latest news on Rust?", None));
        assert!(config.requested_for("anything", Some(&serde_json::json!({ "web_search": true }))));
        assert!(!config.requested_for(
            "latest news",
            Some(&serde_json::json!({ "web_search": false }))
        ));
    }

    #[test]
    fn test_web_search_requested_for_disabled_or_empty_url() {
        let disabled = WebSearchConfig {
            enabled: false,
            ..WebSearchConfig::default()
        };
        assert!(!disabled.requested_for(
            "latest news",
            Some(&serde_json::json!({ "web_search": true }))
        ));

        let empty_url = WebSearchConfig {
            enabled: true,
            query_url: "  ".to_string(),
            ..WebSearchConfig::default()
        };
        assert!(!empty_url.requested_for("latest news", None));
    }

    #[test]
    fn environment_matrix_validates_secrets_and_loads_server_settings() {
        let _lock = lock();
        let names = [
            "APP_BASE_URL",
            "GITHUB_API_URL",
            "CORS_ALLOW_CREDENTIALS",
            "CORS_ORIGINS",
            "DATABASE_URL",
            "ENCRYPTION_KEY",
            "GPT4ALL_MODELS_URL",
            "HOST",
            "HUGGINGFACE_MODELS_URL",
            "JWT_ACCESS_LIFETIME",
            "JWT_REFRESH_LIFETIME",
            "JWT_SECRET",
            "LITELLM_HOST",
            "LITELLM_KEY",
            "MODEL_SEARCH_PROXY_URL",
            "MONITORING_ENABLED",
            "MONITORING_GRAFANA_ADMIN_PASSWORD",
            "MONITORING_GRAFANA_ADMIN_USER",
            "MONITORING_GRAFANA_TOKEN",
            "MONITORING_GRAFANA_URL",
            "MONITORING_PROMETHEUS_URL",
            "OLLAMA_HOST",
            "PORT",
            "PROMETHEUS_URL",
            "REDIS_URL",
            "SOURCE_RESYNC_ENABLED",
            "SOURCE_RESYNC_INTERVAL_SECS",
            "SOURCE_RESYNC_POLL_SECS",
            "TRAIN_UPLOAD_LIMIT_MB",
            MODEL_BACKEND,
            MODEL_BACKEND_EXECUTABLE,
            AGENT_STATE,
            AGENT_HOST_LOGIN,
            CLAUDE_TOKEN_URL,
            CODEX_SANDBOX,
        ];
        let _environment = Environment::isolated(&names);

        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Missing("JWT_SECRET"))
        ));
        Environment::set("JWT_SECRET", "short");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Invalid(
                "JWT_SECRET must be at least 32 characters"
            ))
        ));
        Environment::set("JWT_SECRET", "12345678901234567890123456789012");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Missing("ENCRYPTION_KEY"))
        ));
        Environment::set("ENCRYPTION_KEY", "short");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Invalid(
                "ENCRYPTION_KEY must be at least 32 characters"
            ))
        ));

        Environment::set("ENCRYPTION_KEY", "abcdefghijklmnopqrstuvwxyz123456");
        Environment::set("DATABASE_URL", "postgres://database/zone");
        Environment::set("REDIS_URL", "redis://cache:6379");
        Environment::set("LITELLM_HOST", "http://models:4000");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::Missing("LITELLM_KEY"))
        ));
        Environment::set("LITELLM_KEY", "models-key");

        let defaults = Config::from_env().expect("required values are present");
        assert_eq!(defaults.host, "0.0.0.0");
        assert_eq!(defaults.port, 8000);
        assert_eq!(defaults.jwt_access_lifetime, 900);
        assert_eq!(defaults.jwt_refresh_lifetime, 604_800);
        assert_eq!(defaults.ollama_host, "http://ollama:11434");
        assert_eq!(defaults.cors_origins, ["*"]);
        assert!(!defaults.cors_allow_credentials);
        assert_eq!(defaults.app_base_url, "http://localhost:3000");
        assert_eq!(defaults.github_api_url, DEFAULT_GITHUB_API_URL);
        assert_eq!(defaults.source_index, SourceIndexConfig::default());
        assert_eq!(defaults.monitoring, MonitoringConfig::from_env());
        assert_eq!(defaults.train_upload_limit_mb, 512);

        Environment::set("HOST", "127.0.0.1");
        Environment::set("PORT", "9001");
        Environment::set("JWT_ACCESS_LIFETIME", "1200");
        Environment::set("JWT_REFRESH_LIFETIME", "not-a-number");
        Environment::set("OLLAMA_HOST", "http://ollama.test:11434");
        Environment::set("GPT4ALL_MODELS_URL", "http://catalog.test/gpt4all");
        Environment::set("HUGGINGFACE_MODELS_URL", "http://catalog.test/huggingface");
        Environment::set("MODEL_SEARCH_PROXY_URL", "  http://proxy.test:8080  ");
        Environment::set("CORS_ORIGINS", " https://one.test, ,https://two.test ");
        Environment::set("CORS_ALLOW_CREDENTIALS", "true");
        Environment::set("APP_BASE_URL", "https://zone.test");
        Environment::set("GITHUB_API_URL", "https://github.example.com/api/v3/");
        Environment::set("SOURCE_RESYNC_ENABLED", "off");
        Environment::set("SOURCE_RESYNC_POLL_SECS", "1");
        Environment::set("SOURCE_RESYNC_INTERVAL_SECS", "9999999");
        Environment::set("MONITORING_ENABLED", "yes");
        Environment::set("MONITORING_PROMETHEUS_URL", "http://prometheus.test/");
        Environment::set("MONITORING_GRAFANA_URL", "http://grafana.test///");
        Environment::set("MONITORING_GRAFANA_TOKEN", "  ");
        Environment::set("MONITORING_GRAFANA_ADMIN_USER", "admin");
        Environment::set("MONITORING_GRAFANA_ADMIN_PASSWORD", "password");
        Environment::set("TRAIN_UPLOAD_LIMIT_MB", "1");

        let configured = Config::from_env().expect("custom values are valid");
        assert_eq!(configured.host, "127.0.0.1");
        assert_eq!(configured.port, 9001);
        assert_eq!(configured.jwt_access_lifetime, 1200);
        assert_eq!(configured.jwt_refresh_lifetime, 604_800);
        assert_eq!(configured.ollama_host, "http://ollama.test:11434");
        assert_eq!(configured.gpt4all_models_url, "http://catalog.test/gpt4all");
        assert_eq!(
            configured.huggingface_models_url,
            "http://catalog.test/huggingface"
        );
        assert_eq!(
            configured.model_search_proxy_url.as_deref(),
            Some("http://proxy.test:8080")
        );
        assert_eq!(
            configured.cors_origins,
            ["https://one.test", "https://two.test"]
        );
        assert!(configured.cors_allow_credentials);
        assert_eq!(configured.app_base_url, "https://zone.test");
        // The trailing slash goes, because every caller joins a rooted path.
        assert_eq!(
            configured.github_api_url,
            "https://github.example.com/api/v3"
        );
        assert_eq!(
            configured.source_index,
            SourceIndexConfig {
                enabled: false,
                poll_interval_secs: 30,
                interval_secs: 7 * 86_400,
            }
        );
        assert!(configured.monitoring.enabled);
        assert_eq!(
            configured.monitoring.prometheus_url,
            "http://prometheus.test"
        );
        assert_eq!(configured.monitoring.grafana_url, "http://grafana.test");
        assert!(configured.monitoring.grafana_token.is_none());
        assert_eq!(configured.monitoring.grafana_user.as_deref(), Some("admin"));
        assert_eq!(
            configured.monitoring.grafana_password.as_deref(),
            Some("password")
        );
        assert_eq!(configured.train_upload_limit_mb, 4);
        let monitoring = format!("{:?}", configured.monitoring);
        assert!(monitoring.contains("grafana_token: None"));
        assert!(monitoring.contains("grafana_secret: Some(\"[REDACTED]\")"));

        Environment::set("PORT", "invalid");
        Environment::set("SOURCE_RESYNC_ENABLED", "not-truthy");
        Environment::set("SOURCE_RESYNC_POLL_SECS", "invalid");
        Environment::set("TRAIN_UPLOAD_LIMIT_MB", "invalid");
        Environment::set("MODEL_SEARCH_PROXY_URL", "  ");
        Environment::remove("MONITORING_PROMETHEUS_URL");
        Environment::set("PROMETHEUS_URL", "http://legacy-prometheus/");
        let fallbacks = Config::from_env().expect("invalid optional values use defaults");
        assert_eq!(fallbacks.port, 8000);
        assert!(!fallbacks.source_index.enabled);
        assert_eq!(fallbacks.source_index.poll_interval_secs, 300);
        assert_eq!(fallbacks.train_upload_limit_mb, 512);
        assert!(fallbacks.model_search_proxy_url.is_none());
        assert_eq!(
            fallbacks.monitoring.prometheus_url,
            "http://legacy-prometheus"
        );
    }

    const AGENT_SETTINGS: [&str; 4] = [
        AGENT_STATE,
        AGENT_HOST_LOGIN,
        CLAUDE_TOKEN_URL,
        CODEX_SANDBOX,
    ];

    #[test]
    fn agent_settings_default_and_follow_the_environment() {
        let _lock = lock();
        let _environment = Environment::isolated(&AGENT_SETTINGS);

        let defaults = AgentConfig::from_env().expect("unset falls back");
        assert_eq!(
            defaults.state,
            state_root(None, env_path(STATE_HOME), env_path(HOME))
                .expect("the test process has a home"),
            "a native instance keeps agent homes in its user's own state directory"
        );
        assert!(defaults.state.ends_with(STATE_ROOT));
        assert!(defaults.host_login);
        assert_eq!(defaults.claude_token_url, DEFAULT_CLAUDE_TOKEN_URL);
        assert_eq!(defaults.codex_sandbox, CodexSandbox::WorkspaceWrite);

        Environment::set(AGENT_STATE, "  /app/agent-state  ");
        Environment::set(AGENT_HOST_LOGIN, " FALSE ");
        Environment::set(CLAUDE_TOKEN_URL, " http://127.0.0.1:9100/v1/oauth/token ");
        Environment::set(CODEX_SANDBOX, " danger-full-access ");
        assert_eq!(
            AgentConfig::from_env().expect("every value is valid"),
            AgentConfig {
                state: PathBuf::from("/app/agent-state"),
                host_login: false,
                claude_token_url: "http://127.0.0.1:9100/v1/oauth/token".to_string(),
                codex_sandbox: CodexSandbox::DangerFullAccess,
            },
            "a loopback token endpoint stays configurable: it is operator configuration"
        );

        for sandbox in CodexSandbox::ALL {
            Environment::set(CODEX_SANDBOX, sandbox.as_str());
            assert_eq!(
                AgentConfig::from_env()
                    .expect("a sandbox codex has")
                    .codex_sandbox,
                sandbox
            );
        }

        for (value, expected) in [
            ("true", true),
            ("1", true),
            ("On", true),
            ("0", false),
            ("no", false),
            ("off", false),
        ] {
            Environment::set(AGENT_HOST_LOGIN, value);
            assert_eq!(
                AgentConfig::from_env().expect("a boolean").host_login,
                expected,
                "{value}"
            );
        }

        for name in AGENT_SETTINGS {
            Environment::set(name, "   ");
        }
        assert_eq!(
            AgentConfig::from_env().expect("blank is not a setting"),
            defaults
        );
    }

    #[test]
    fn unreadable_agent_settings_are_refused() {
        let _lock = lock();
        let _environment = Environment::isolated(&AGENT_SETTINGS);

        Environment::set(AGENT_HOST_LOGIN, "sometimes");
        assert!(
            matches!(
                AgentConfig::from_env(),
                Err(ConfigError::Invalid(
                    "ZONE_AGENT_HOST_LOGIN must be true or false"
                ))
            ),
            "a value that is neither must not silently let organizations use the host's sign-in"
        );
        Environment::remove(AGENT_HOST_LOGIN);

        Environment::set(AGENT_STATE, "agent-state");
        assert!(
            matches!(
                AgentConfig::from_env(),
                Err(ConfigError::Invalid(
                    "ZONE_AGENT_STATE_DIR must be an absolute path"
                ))
            ),
            "a relative root resolves against each agent's own working directory"
        );
        Environment::remove(AGENT_STATE);

        for sandbox in ["read-only", "Danger-Full-Access", "none", "workspace_write"] {
            Environment::set(CODEX_SANDBOX, sandbox);
            assert!(
                matches!(
                    AgentConfig::from_env(),
                    Err(ConfigError::Invalid(
                        "ZONE_CODEX_SANDBOX must be workspace-write or danger-full-access"
                    ))
                ),
                "{sandbox} is not a sandbox codex runs granted tools in"
            );
        }
        Environment::remove(CODEX_SANDBOX);

        for url in [
            "platform.claude.com/v1/oauth/token",
            "ftp://platform.claude.com/v1/oauth/token",
            "https://",
        ] {
            Environment::set(CLAUDE_TOKEN_URL, url);
            assert!(
                matches!(
                    AgentConfig::from_env(),
                    Err(ConfigError::Invalid(
                        "ZONE_CLAUDE_TOKEN_URL must be an absolute http or https URL with a host"
                    ))
                ),
                "{url} is not an endpoint a code can be exchanged at"
            );
        }
        for url in [
            "https://someone:secret@platform.claude.com/v1/oauth/token",
            "https://platform.claude.com/v1/oauth/token?key=secret",
            "https://platform.claude.com/v1/oauth/token#secret",
        ] {
            Environment::set(CLAUDE_TOKEN_URL, url);
            assert!(
                matches!(
                    AgentConfig::from_env(),
                    Err(ConfigError::Invalid(
                        "ZONE_CLAUDE_TOKEN_URL must not carry credentials, a query or a fragment"
                    ))
                ),
                "{url} would reach every log that prints the config"
            );
        }
    }

    #[test]
    fn the_state_root_follows_the_xdg_base_directories() {
        let home = || Some(PathBuf::from("/home/zone"));
        assert_eq!(
            state_root(None, Some(PathBuf::from("/var/state")), home())
                .expect("an XDG state directory"),
            PathBuf::from("/var/state/zone/agents")
        );
        assert_eq!(
            state_root(None, None, home()).expect("a home"),
            PathBuf::from("/home/zone/.local/state/zone/agents")
        );
        assert_eq!(
            state_root(None, Some(PathBuf::from("state")), home()).expect("a home"),
            PathBuf::from("/home/zone/.local/state/zone/agents"),
            "the XDG specification has a relative XDG_STATE_HOME ignored"
        );
        assert_eq!(
            state_root(
                Some(PathBuf::from("/app/agent-state")),
                Some(PathBuf::from("/var/state")),
                home()
            )
            .expect("a configured root"),
            PathBuf::from("/app/agent-state"),
            "the configured root wins"
        );
        for home in [None, Some(PathBuf::from("zone"))] {
            assert!(
                matches!(
                    state_root(None, None, home),
                    Err(ConfigError::Missing("ZONE_AGENT_STATE_DIR"))
                ),
                "with no absolute home there is nowhere private to default to"
            );
        }
    }

    #[test]
    fn agent_homes_follow_the_state_layout() {
        let agents = AgentConfig {
            state: PathBuf::from("/app/agent-state"),
            ..AgentConfig::default()
        };
        let organization =
            Uuid::parse_str("7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c").expect("a valid UUID");
        for (agent, home) in [
            (
                AgentKind::Claude,
                "/app/agent-state/7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c/claude",
            ),
            (
                AgentKind::Codex,
                "/app/agent-state/7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c/codex",
            ),
        ] {
            let home = PathBuf::from(home);
            assert_eq!(agents.home(organization, agent), home);
            assert_eq!(agent_home(&agents.state, organization, agent), home);
            assert_eq!(agents.work(organization, agent), home.join("work"));
            assert_eq!(
                agent_work(&agents.state, organization, agent),
                home.join("work")
            );
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        fs::metadata(path)
            .expect("the directory exists")
            .permissions()
            .mode()
            & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn create_home_makes_the_home_private_from_the_state_root_down() {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let agents = AgentConfig {
            state: scratch.path().join("agents"),
            ..AgentConfig::default()
        };
        let organization = Uuid::new_v4();
        let organization_root = agents.state.join(organization.to_string());

        let home = agents
            .create_home(organization, AgentKind::Codex)
            .expect("the home is created");
        let work = agents.work(organization, AgentKind::Codex);
        assert_eq!(home, agents.home(organization, AgentKind::Codex));
        for directory in [&agents.state, &organization_root, &home, &work] {
            assert_eq!(
                mode(directory),
                PRIVATE,
                "{} is open to other users",
                directory.display()
            );
        }

        for directory in [&organization_root, &home, &work] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).expect("widened");
        }
        assert_eq!(
            agents
                .create_home(organization, AgentKind::Codex)
                .expect("an existing home is reused"),
            home
        );
        for directory in [&organization_root, &home, &work] {
            assert_eq!(
                mode(directory),
                PRIVATE,
                "{} stayed open to other users",
                directory.display()
            );
        }

        let claude = agents
            .create_home(organization, AgentKind::Claude)
            .expect("a second agent's home");
        assert_eq!(mode(&claude), PRIVATE);
        assert_eq!(mode(&agents.work(organization, AgentKind::Claude)), PRIVATE);
    }

    #[cfg(unix)]
    #[test]
    fn create_home_leaves_an_existing_state_root_as_the_operator_made_it() {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let state = scratch.path().join("agents");
        fs::create_dir(&state).expect("the operator's directory");
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).expect("shared");
        let agents = AgentConfig {
            state: state.clone(),
            ..AgentConfig::default()
        };
        let organization = Uuid::new_v4();

        agents
            .create_home(organization, AgentKind::Claude)
            .expect("the home is created");

        assert_eq!(
            mode(&state),
            0o755,
            "a chmod here would lock every other user out of a shared directory such as /var/lib"
        );
        assert_eq!(mode(&state.join(organization.to_string())), PRIVATE);
    }

    #[test]
    fn a_configured_binary_runs_only_the_agent_it_was_configured_for() {
        let mut config = create_test_config();
        for agent in AgentKind::ALL {
            assert_eq!(
                config.agent_executable(agent),
                PathBuf::from(agent.executable())
            );
        }

        config.model_backend = ModelBackend::Cli {
            agent: AgentKind::Claude,
            executable: None,
        };
        assert_eq!(
            config.agent_executable(AgentKind::Claude),
            PathBuf::from("claude")
        );

        config.model_backend = ModelBackend::Cli {
            agent: AgentKind::Claude,
            executable: Some(PathBuf::from("/opt/homebrew/bin/claude")),
        };
        assert_eq!(
            config.agent_executable(AgentKind::Claude),
            PathBuf::from("/opt/homebrew/bin/claude")
        );
        assert_eq!(
            config.agent_executable(AgentKind::Codex),
            PathBuf::from("codex"),
            "an organization choosing codex must not be run through the operator's claude binary"
        );
    }

    #[test]
    fn the_server_config_carries_and_shows_the_agent_settings() {
        let _lock = lock();
        let names = [
            "DATABASE_URL",
            "ENCRYPTION_KEY",
            "GITHUB_API_URL",
            "JWT_SECRET",
            "LITELLM_HOST",
            "LITELLM_KEY",
            "REDIS_URL",
            MODEL_BACKEND,
            MODEL_BACKEND_EXECUTABLE,
            AGENT_STATE,
            AGENT_HOST_LOGIN,
            CLAUDE_TOKEN_URL,
            CODEX_SANDBOX,
        ];
        let _environment = Environment::isolated(&names);
        Environment::set("JWT_SECRET", "12345678901234567890123456789012");
        Environment::set("ENCRYPTION_KEY", "12345678901234567890123456789012");
        Environment::set("DATABASE_URL", "postgres://database/zone");
        Environment::set("REDIS_URL", "redis://cache:6379");
        Environment::set("LITELLM_HOST", "http://models:4000");
        Environment::set("LITELLM_KEY", "models-key");
        Environment::set(AGENT_STATE, "/app/agent-state");
        Environment::set(AGENT_HOST_LOGIN, "false");
        Environment::set(CODEX_SANDBOX, "danger-full-access");

        let config = Config::from_env().expect("every value is valid");
        assert_eq!(
            config.agents,
            AgentConfig {
                state: PathBuf::from("/app/agent-state"),
                host_login: false,
                claude_token_url: DEFAULT_CLAUDE_TOKEN_URL.to_string(),
                codex_sandbox: CodexSandbox::DangerFullAccess,
            }
        );
        let debug = format!("{config:?}");
        assert!(
            debug.contains(
                r#"agents: AgentConfig { state: "/app/agent-state", host_login: false, claude_token_url: "https://platform.claude.com/v1/oauth/token", codex_sandbox: DangerFullAccess }"#
            ),
            "{debug}"
        );

        Environment::set(CODEX_SANDBOX, "read-only");
        assert!(
            matches!(
                Config::from_env(),
                Err(ConfigError::Invalid(
                    "ZONE_CODEX_SANDBOX must be workspace-write or danger-full-access"
                ))
            ),
            "a sandbox the server cannot give codex must stop it starting"
        );
        Environment::remove(CODEX_SANDBOX);

        Environment::set(AGENT_HOST_LOGIN, "sometimes");
        assert!(
            matches!(
                Config::from_env(),
                Err(ConfigError::Invalid(
                    "ZONE_AGENT_HOST_LOGIN must be true or false"
                ))
            ),
            "an agent setting the server cannot read must stop it starting"
        );
    }
}
