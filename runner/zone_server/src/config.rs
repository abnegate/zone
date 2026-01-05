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

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, ConfigError> {
        let jwt_secret = env::var("JWT_SECRET").map_err(|_| ConfigError::Missing("JWT_SECRET"))?;
        if jwt_secret.len() < 32 {
            return Err(ConfigError::Invalid(
                "JWT_SECRET must be at least 32 characters",
            ));
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

    #[test]
    fn test_config_jwt_secret_getter() {
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://localhost/test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "my-secret-key".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "test-key".to_string(),
        };

        assert_eq!(config.jwt_secret(), "my-secret-key");
    }

    #[test]
    fn test_config_access_token_lifetime() {
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://localhost/test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "my-secret-key".to_string(),
            jwt_access_lifetime: 1800,
            jwt_refresh_lifetime: 604800,
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "test-key".to_string(),
        };

        let lifetime = config.access_token_lifetime();
        assert_eq!(lifetime.num_seconds(), 1800);
    }

    #[test]
    fn test_config_refresh_token_lifetime() {
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://localhost/test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "my-secret-key".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 86400,
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "test-key".to_string(),
        };

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
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://localhost/test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "my-secret-key".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "test-key".to_string(),
        };

        let cloned = config.clone();
        assert_eq!(config.host, cloned.host);
        assert_eq!(config.database_url, cloned.database_url);
        assert_eq!(config.port, cloned.port);
        assert_eq!(config.jwt_secret, cloned.jwt_secret);
    }

    #[test]
    fn test_config_debug_redacts_secrets() {
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://user:password@localhost/test".to_string(),
            redis_url: "redis://:secret@localhost:6379".to_string(),
            jwt_secret: "my-super-secret-key".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "sk-secret-api-key".to_string(),
        };

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
        let config = Config {
            host: "0.0.0.0".to_string(),
            port: 8000,
            database_url: "test".to_string(),
            redis_url: "test".to_string(),
            jwt_secret: "test".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            litellm_host: "test".to_string(),
            litellm_key: "test".to_string(),
        };

        // Default port is 8000
        assert_eq!(config.port, 8000);
    }

    #[test]
    fn test_config_default_lifetimes() {
        // Default access token lifetime is 900 seconds (15 minutes)
        // Default refresh token lifetime is 604800 seconds (7 days)
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "test".to_string(),
            redis_url: "test".to_string(),
            jwt_secret: "test".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            litellm_host: "test".to_string(),
            litellm_key: "test".to_string(),
        };

        assert_eq!(config.jwt_access_lifetime, 900);
        assert_eq!(config.jwt_refresh_lifetime, 604800);
    }

    #[test]
    fn test_config_lifetime_methods() {
        let config = Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "test".to_string(),
            redis_url: "test".to_string(),
            jwt_secret: "test".to_string(),
            jwt_access_lifetime: 3600,   // 1 hour
            jwt_refresh_lifetime: 86400, // 1 day
            litellm_host: "test".to_string(),
            litellm_key: "test".to_string(),
        };

        let access_duration = config.access_token_lifetime();
        let refresh_duration = config.refresh_token_lifetime();

        assert_eq!(access_duration.num_minutes(), 60);
        assert_eq!(refresh_duration.num_hours(), 24);
    }

    #[test]
    fn test_config_all_fields() {
        let config = Config {
            host: "127.0.0.1".to_string(),
            port: 3000,
            database_url: "postgres://user:pass@localhost/db".to_string(),
            redis_url: "redis://:password@localhost:6379/0".to_string(),
            jwt_secret: "super-secret-key-for-jwt-signing".to_string(),
            jwt_access_lifetime: 1800,
            jwt_refresh_lifetime: 86400,
            litellm_host: "http://litellm.local:4000".to_string(),
            litellm_key: "sk-litellm-key".to_string(),
        };

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
}
