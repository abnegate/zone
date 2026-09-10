//! Server configuration

use std::env;

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
    /// LiteLLM host URL
    pub litellm_host: String,
    /// LiteLLM API key
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
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval_secs: 300,
            interval_secs: 3600,
        }
    }
}

impl SourceIndexConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: env_truthy("SOURCE_RESYNC_ENABLED", true),
            poll_interval_secs: env_u64("SOURCE_RESYNC_POLL_SECS", 300, 30, 86_400),
            interval_secs: env_u64("SOURCE_RESYNC_INTERVAL_SECS", 3600, 60, 7 * 86_400),
        }
    }
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
        Ok(s) => matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
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
            }
            _ => {
                return Err(ConfigError::Invalid(
                    "GITHUB_API_URL must be an absolute http or https URL with a host",
                ));
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
            litellm_host: env::var("LITELLM_HOST")
                .map_err(|_| ConfigError::Missing("LITELLM_HOST"))?,
            litellm_key: env::var("LITELLM_KEY")
                .map_err(|_| ConfigError::Missing("LITELLM_KEY"))?,
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
    use std::sync::{LazyLock, Mutex};

    static ENVIRONMENT: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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
        let _lock = ENVIRONMENT.lock().expect("environment lock");
        let names = [
            "DATABASE_URL",
            "ENCRYPTION_KEY",
            "GITHUB_API_URL",
            "JWT_SECRET",
            "LITELLM_HOST",
            "LITELLM_KEY",
            "REDIS_URL",
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
        let _lock = ENVIRONMENT.lock().expect("environment lock");
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
}
