//! Server configuration

use std::env;

/// Upstream GPT4All model catalog. Tests should override `Config::gpt4all_models_url`.
pub const DEFAULT_GPT4ALL_MODELS_URL: &str =
    "https://raw.githubusercontent.com/nomic-ai/gpt4all/main/gpt4all-chat/metadata/models3.json";

/// Upstream HuggingFace models API. Tests should override `Config::huggingface_models_url`.
pub const DEFAULT_HUGGINGFACE_MODELS_URL: &str = "https://huggingface.co/api/models";

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
    /// CORS allowed origins (comma-separated, default: *)
    pub cors_origins: Vec<String>,
    /// CORS allow credentials (default: false)
    pub cors_allow_credentials: bool,
    /// Application base URL for email links (default: http://localhost:3000)
    pub app_base_url: String,
    /// Live web search via SearXNG (through Gluetun when the VPN profile is up)
    pub web_search: WebSearchConfig,
    /// Direct ComfyUI image generation and artifact storage.
    pub comfyui: ComfyUiConfig,
    /// Background source reindex (schedule + change detection)
    pub source_index: SourceIndexConfig,
    /// Zone Prometheus / Grafana for on-call tools.
    pub monitoring: MonitoringConfig,
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

/// Direct image generation settings loaded from `COMFYUI_*` environment variables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComfyUiConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_token: Option<String>,
    pub workflow_path: std::path::PathBuf,
    pub checkpoint: String,
    pub artifact_root: std::path::PathBuf,
    pub classifier_model: String,
    pub classifier_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub generation_timeout_secs: u64,
    pub poll_interval_ms: u64,
}

impl Default for ComfyUiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "http://comfyui:8188".to_string(),
            api_token: None,
            workflow_path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../comfyui/workflows/flux1-schnell-fp8-api.json"),
            checkpoint: "flux1-schnell-fp8.safetensors".to_string(),
            artifact_root: "/app/artifacts".into(),
            classifier_model: "llama3.2:3b".to_string(),
            classifier_timeout_secs: 3,
            request_timeout_secs: 15,
            generation_timeout_secs: 300,
            poll_interval_ms: 500,
        }
    }
}

impl ComfyUiConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: env_truthy("COMFYUI_ENABLED", false),
            base_url: env::var("COMFYUI_BASE_URL")
                .unwrap_or_else(|_| "http://comfyui:8188".to_string())
                .trim_end_matches('/')
                .to_string(),
            api_token: env::var("COMFYUI_API_TOKEN")
                .ok()
                .filter(|token| !token.trim().is_empty()),
            workflow_path: env::var("COMFYUI_WORKFLOW_PATH")
                .unwrap_or_else(|_| "/app/comfyui/workflows/flux1-schnell-fp8-api.json".to_string())
                .into(),
            checkpoint: env::var("COMFYUI_CHECKPOINT")
                .unwrap_or_else(|_| "flux1-schnell-fp8.safetensors".to_string()),
            artifact_root: env::var("ARTIFACT_ROOT")
                .unwrap_or_else(|_| "/app/artifacts".to_string())
                .into(),
            classifier_model: env::var("COMFYUI_CLASSIFIER_MODEL")
                .unwrap_or_else(|_| "llama3.2:3b".to_string()),
            classifier_timeout_secs: env_u64("COMFYUI_CLASSIFIER_TIMEOUT_SECS", 3, 1, 30),
            request_timeout_secs: env_u64("COMFYUI_REQUEST_TIMEOUT_SECS", 15, 1, 120),
            generation_timeout_secs: env_u64("COMFYUI_GENERATION_TIMEOUT_SECS", 300, 10, 3600),
            poll_interval_ms: env_u64("COMFYUI_POLL_INTERVAL_MS", 500, 50, 5000),
        }
    }
}

/// Default SearXNG query URL. SearXNG shares Gluetun's network namespace, so
/// the hostname is `gluetun`, not `searxng`.
pub const DEFAULT_SEARXNG_QUERY_URL: &str = "http://gluetun:8080/search?q=<query>&format=json";

/// Zone chat web search settings loaded from `SEARCH_*` env vars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSearchConfig {
    /// Master switch. When false, chat never calls SearXNG.
    pub enabled: bool,
    /// Query URL template. `<query>` or `{query}` is replaced with the
    /// URL-encoded search string.
    pub query_url: String,
    /// Max results injected into the prompt (1–20)
    pub result_count: usize,
    /// HTTP timeout for a single SearXNG request
    pub timeout_secs: u64,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            query_url: DEFAULT_SEARXNG_QUERY_URL.to_string(),
            result_count: 5,
            timeout_secs: 15,
        }
    }
}

impl WebSearchConfig {
    /// Load from `SEARCH_*` environment variables. Missing values use defaults
    /// that match docker-compose (`SEARCH_ENABLE_WEB_SEARCH=true`
    /// and the Gluetun SearXNG URL).
    pub fn from_env() -> Self {
        let result_count = env::var("SEARCH_RESULT_COUNT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5)
            .clamp(1, 20);
        let timeout_secs = env::var("SEARCH_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15)
            .clamp(1, 60);
        Self {
            enabled: env_truthy("SEARCH_ENABLE_WEB_SEARCH", true),
            query_url: env::var("SEARCH_SEARXNG_QUERY_URL")
                .unwrap_or_else(|_| DEFAULT_SEARXNG_QUERY_URL.to_string()),
            result_count,
            timeout_secs,
        }
    }

    /// Whether this chat message should trigger a SearXNG lookup.
    ///
    /// When the server switch is on, search runs only when the message looks
    /// like it needs current web information. A boolean `metadata.web_search`
    /// value can force a lookup on or off for a single message.
    pub fn requested_for(&self, content: &str, metadata: Option<&serde_json::Value>) -> bool {
        if !self.enabled || self.query_url.trim().is_empty() {
            return false;
        }
        match metadata.and_then(|m| m.get("web_search")) {
            Some(v) if v.is_boolean() => v.as_bool() == Some(true),
            _ => crate::services::searxng::needs_web_search(content),
        }
    }
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

        // Parse CORS origins
        let cors_origins = env::var("CORS_ORIGINS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|origin| origin.trim().to_string())
                    .filter(|origin| !origin.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec!["*".to_string()]); // Default to permissive

        let cors_allow_credentials = env::var("CORS_ALLOW_CREDENTIALS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);

        let app_base_url =
            env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

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
            web_search: WebSearchConfig::from_env(),
            comfyui: ComfyUiConfig::from_env(),
            source_index: SourceIndexConfig::from_env(),
            monitoring: MonitoringConfig::from_env(),
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

    // Note: Environment variable tests are skipped because env::set_var/remove_var
    // are unsafe in Rust 2024 edition. Config::from_env() would be tested in
    // integration tests with proper environment setup.

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
            web_search: WebSearchConfig::default(),
            comfyui: ComfyUiConfig::default(),
            source_index: SourceIndexConfig::default(),
            monitoring: MonitoringConfig::default(),
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

    #[test]
    fn test_web_search_default_is_off_for_tests() {
        let config = WebSearchConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.query_url, DEFAULT_SEARXNG_QUERY_URL);
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
}
