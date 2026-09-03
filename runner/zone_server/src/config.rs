//! Server configuration

use std::env;

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
}

/// Default SearXNG query URL. SearXNG shares Gluetun's network namespace, so
/// the hostname is `gluetun`, not `searxng`.
pub const DEFAULT_SEARXNG_QUERY_URL: &str = "http://gluetun:8080/search?q=<query>&format=json";

/// Chat / Open WebUI web search settings loaded from `SEARCH_*` env vars.
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
    /// that match Open WebUI / docker-compose (`SEARCH_ENABLE_WEB_SEARCH=true`
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
    /// Enabled by default when the server switch is on. A boolean
    /// `metadata.web_search` value opts a single message in or out.
    pub fn requested_for(&self, metadata: Option<&serde_json::Value>) -> bool {
        if !self.enabled || self.query_url.trim().is_empty() {
            return false;
        }
        match metadata.and_then(|m| m.get("web_search")) {
            Some(v) if v.is_boolean() => v.as_bool() == Some(true),
            _ => true,
        }
    }
}

fn env_truthy(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(s) => matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
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
            encryption_key,
            cors_origins,
            cors_allow_credentials,
            app_base_url,
            web_search: WebSearchConfig::from_env(),
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
            .field("encryption_key", &"[REDACTED]")
            .field("cors_origins", &self.cors_origins)
            .field("cors_allow_credentials", &self.cors_allow_credentials)
            .field("app_base_url", &self.app_base_url)
            .field("web_search", &self.web_search)
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
            encryption_key: "12345678901234567890123456789012".to_string(),
            cors_origins: vec!["*".to_string()],
            cors_allow_credentials: false,
            app_base_url: "http://localhost:3000".to_string(),
            web_search: WebSearchConfig::default(),
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

        // Should contain [REDACTED] placeholders
        assert!(debug_str.contains("[REDACTED]"));
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
        assert!(!config.requested_for(None));
    }

    #[test]
    fn test_web_search_requested_for_respects_metadata() {
        let config = WebSearchConfig {
            enabled: true,
            ..WebSearchConfig::default()
        };
        assert!(config.requested_for(None));
        assert!(config.requested_for(Some(&serde_json::json!({}))));
        assert!(config.requested_for(Some(&serde_json::json!({ "web_search": true }))));
        assert!(!config.requested_for(Some(&serde_json::json!({ "web_search": false }))));
        assert!(config.requested_for(Some(&serde_json::json!({ "web_search": "yes" }))));
    }

    #[test]
    fn test_web_search_requested_for_disabled_or_empty_url() {
        let disabled = WebSearchConfig {
            enabled: false,
            ..WebSearchConfig::default()
        };
        assert!(!disabled.requested_for(Some(&serde_json::json!({ "web_search": true }))));

        let empty_url = WebSearchConfig {
            enabled: true,
            query_url: "  ".to_string(),
            ..WebSearchConfig::default()
        };
        assert!(!empty_url.requested_for(None));
    }
}
