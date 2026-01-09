//! Context error types

use thiserror::Error;

/// Error type for context operations
///
/// SECURITY NOTE: All error messages must be sanitized to prevent leaking
/// sensitive data like API keys, credentials, or authentication tokens.
#[derive(Error, Debug)]
pub enum ContextError {
    // Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid source configuration: {0}")]
    InvalidSourceConfig(String),

    // Adapter errors
    #[error("Adapter error for {source_type}: {message}")]
    Adapter {
        source_type: String,
        message: String,
    },

    #[error("Source not found: {0}")]
    SourceNotFound(String),

    #[error("Unsupported source type: {0}")]
    UnsupportedSourceType(String),

    // Authentication errors
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Credentials required for source: {0}")]
    CredentialsRequired(String),

    // Rate limiting
    #[error("Rate limited: retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    // Embedding errors
    #[error("Embedding generation failed: {0}")]
    Embedding(String),

    #[error("Embedding provider not configured")]
    EmbeddingProviderNotConfigured,

    #[error("Unsupported embedding dimension: expected {expected}, got {actual}")]
    EmbeddingDimensionMismatch { expected: usize, actual: usize },

    // Analysis errors
    #[error("Analysis failed: {0}")]
    Analysis(String),

    #[error("Entity extraction failed: {0}")]
    EntityExtraction(String),

    #[error("Categorization failed: {0}")]
    Categorization(String),

    // Storage errors
    #[error("Database error: {0}")]
    Database(String),

    #[error("Vector store error: {0}")]
    VectorStore(String),

    #[error("Cache error: {0}")]
    Cache(String),

    // Content errors
    #[error("Token budget exceeded: {used} > {budget}")]
    TokenBudgetExceeded { used: usize, budget: usize },

    #[error("Content too large: {size_bytes} bytes (max: {max_bytes} bytes)")]
    ContentTooLarge { size_bytes: usize, max_bytes: usize },

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Chunk error: {0}")]
    Chunk(String),

    // Context building errors
    #[error("Context building failed: {0}")]
    ContextBuild(String),

    #[error("No relevant content found")]
    NoRelevantContent,

    // Network errors
    #[error("Network error: {0}")]
    Network(String),

    #[error("Timeout: {operation} after {timeout_ms}ms")]
    Timeout { operation: String, timeout_ms: u64 },

    #[error("Too many redirects")]
    TooManyRedirects,

    #[error("Invalid redirect")]
    InvalidRedirect,

    // Permission errors
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    // Standard error conversions
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),

    #[error("{0}")]
    Other(String),
}

impl ContextError {
    /// Create an adapter error
    pub fn adapter(source_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Adapter {
            source_type: source_type.into(),
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Network(_) | Self::Timeout { .. } | Self::Http(_)
        )
    }

    /// Get retry delay in seconds if applicable
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after_secs } => Some(*retry_after_secs),
            Self::Timeout { .. } => Some(1),
            Self::Network(_) | Self::Http(_) => Some(5),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, ContextError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_display() {
        let err = ContextError::Config("Invalid setting".to_string());
        assert_eq!(err.to_string(), "Configuration error: Invalid setting");
    }

    #[test]
    fn test_adapter_error_display() {
        let err = ContextError::adapter("github", "API rate limit exceeded");
        assert_eq!(
            err.to_string(),
            "Adapter error for github: API rate limit exceeded"
        );
    }

    #[test]
    fn test_rate_limited_error() {
        let err = ContextError::RateLimited {
            retry_after_secs: 60,
        };
        assert_eq!(err.to_string(), "Rate limited: retry after 60s");
        assert!(err.is_retryable());
        assert_eq!(err.retry_after(), Some(60));
    }

    #[test]
    fn test_token_budget_exceeded() {
        let err = ContextError::TokenBudgetExceeded {
            used: 150000,
            budget: 100000,
        };
        assert_eq!(err.to_string(), "Token budget exceeded: 150000 > 100000");
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_embedding_dimension_mismatch() {
        let err = ContextError::EmbeddingDimensionMismatch {
            expected: 1536,
            actual: 768,
        };
        assert_eq!(
            err.to_string(),
            "Unsupported embedding dimension: expected 1536, got 768"
        );
    }

    #[test]
    fn test_timeout_error() {
        let err = ContextError::Timeout {
            operation: "fetch".to_string(),
            timeout_ms: 30000,
        };
        assert_eq!(err.to_string(), "Timeout: fetch after 30000ms");
        assert!(err.is_retryable());
        assert_eq!(err.retry_after(), Some(1));
    }

    #[test]
    fn test_network_error_retryable() {
        let err = ContextError::Network("Connection reset".to_string());
        assert!(err.is_retryable());
        assert_eq!(err.retry_after(), Some(5));
    }

    #[test]
    fn test_non_retryable_errors() {
        let errors = vec![
            ContextError::Config("bad config".to_string()),
            ContextError::Auth("invalid token".to_string()),
            ContextError::PermissionDenied("access denied".to_string()),
            ContextError::Parse("invalid json".to_string()),
        ];

        for err in errors {
            assert!(!err.is_retryable(), "Error should not be retryable: {err}");
            assert_eq!(err.retry_after(), None);
        }
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ContextError>();
        assert_sync::<ContextError>();
    }

    #[test]
    fn test_result_type() {
        fn get_result() -> Result<i32> {
            Ok(42)
        }

        let result = get_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: ContextError = io_err.into();
        assert!(matches!(err, ContextError::Io(_)));
    }

    #[test]
    fn test_error_from_serde() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err: ContextError = json_err.into();
        assert!(matches!(err, ContextError::Serialization(_)));
    }
}
