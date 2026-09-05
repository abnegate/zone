//! Embedding provider implementations
//!
//! Supports multiple embedding providers:
//! - Ollama (self-hosted via LiteLLM)
//! - In-process ONNX (self-hosted, feature `local-embeddings`)
//! - OpenAI
//! - AWS Bedrock

mod ollama;
// mod openai;
// mod bedrock;

#[cfg(feature = "local-embeddings")]
mod local;

pub use ollama::{DEFAULT_OLLAMA_EMBEDDING_MODEL, OllamaProvider};

#[cfg(feature = "local-embeddings")]
pub use local::LocalEmbeddingProvider;

/// Provider identifier constants
pub const PROVIDER_SELF_HOSTED: &str = "self_hosted";
pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_BEDROCK: &str = "bedrock";

/// Embedding engine identifiers for the `self_hosted` provider.
///
/// The provider says *where the LLM lives*; the engine says *how embeddings
/// are computed*. `ollama` calls the configured host over HTTP; `local` runs
/// the model in-process.
pub const EMBEDDING_ENGINE_OLLAMA: &str = "ollama";
pub const EMBEDDING_ENGINE_LOCAL: &str = "local";

// Mock provider for testing - available both as cfg(test) and cfg(feature = "test-utils")
#[cfg(any(test, feature = "test-utils"))]
mod mock;

#[cfg(any(test, feature = "test-utils"))]
pub use mock::MockEmbeddingService;

use crate::embeddings::EmbeddingService;
use crate::error::{ContextError, Result};
use std::sync::Arc;

/// Embedding provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProviderType {
    /// Self-hosted via Ollama/LiteLLM
    SelfHosted,
    /// OpenAI API
    OpenAI,
    /// AWS Bedrock
    Bedrock,
}

/// Configuration for embedding providers
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Provider type
    pub provider: EmbeddingProviderType,
    /// Model name
    pub model: String,
    /// Batch size for embedding requests
    pub batch_size: usize,
    /// Maximum tokens per request
    pub max_tokens: usize,
    /// Embedding dimension
    pub dimension: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProviderType::SelfHosted,
            model: DEFAULT_OLLAMA_EMBEDDING_MODEL.to_string(),
            batch_size: 32,
            max_tokens: 8192,
            dimension: 1024,
        }
    }
}

/// AI settings for provider configuration
///
/// This mirrors the effective AI settings from zone_server
#[derive(Debug, Clone, Default)]
pub struct AiSettings {
    /// Provider name: "self_hosted", "openai", "anthropic", "bedrock"
    pub provider: String,
    /// LiteLLM host URL (for self-hosted)
    pub litellm_host: Option<String>,
    /// LiteLLM API key
    pub litellm_key: Option<String>,
    /// OpenAI API key
    pub openai_api_key: Option<String>,
    /// OpenAI base URL (for custom endpoints)
    pub openai_base_url: Option<String>,
    /// Bedrock region
    pub bedrock_region: Option<String>,
    /// Embedding model name
    pub model_embedding: Option<String>,
    /// Embedding engine for `self_hosted`: [`EMBEDDING_ENGINE_OLLAMA`]
    /// (default) or [`EMBEDDING_ENGINE_LOCAL`].
    pub embedding_engine: Option<String>,
}

/// Factory for creating embedding providers
pub struct EmbeddingProviderFactory;

impl EmbeddingProviderFactory {
    /// Create an embedding provider based on AI settings
    pub fn create(settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        match settings.provider.as_str() {
            PROVIDER_SELF_HOSTED => match settings.embedding_engine.as_deref() {
                None | Some(EMBEDDING_ENGINE_OLLAMA) => {
                    let provider = OllamaProvider::from_settings(settings)?;
                    Ok(Arc::new(provider))
                }
                Some(EMBEDDING_ENGINE_LOCAL) => Self::create_local(settings),
                Some(other) => Err(ContextError::InvalidSourceConfig(format!(
                    "Unknown embedding engine '{}' (expected '{}' or '{}')",
                    other, EMBEDDING_ENGINE_OLLAMA, EMBEDDING_ENGINE_LOCAL
                ))),
            },
            _ => Err(ContextError::EmbeddingProviderNotConfigured),
        }
    }

    #[cfg(feature = "local-embeddings")]
    fn create_local(settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        let provider = LocalEmbeddingProvider::from_settings(settings)?;
        Ok(Arc::new(provider))
    }

    #[cfg(not(feature = "local-embeddings"))]
    fn create_local(_settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        Err(ContextError::InvalidSourceConfig(
            "Embedding engine 'local' requires the `local-embeddings` feature".to_string(),
        ))
    }

    /// Create a provider with explicit configuration
    pub fn create_with_config(config: EmbeddingConfig) -> Result<Arc<dyn EmbeddingService>> {
        match config.provider {
            EmbeddingProviderType::SelfHosted => {
                // For self-hosted, we need a base URL from settings
                // Since EmbeddingConfig doesn't have URL info, this is primarily for testing
                // In production, use create() with AiSettings instead
                Err(ContextError::InvalidSourceConfig(
                    "create_with_config requires base_url for self-hosted provider. Use create() with AiSettings instead.".to_string()
                ))
            }
            EmbeddingProviderType::OpenAI | EmbeddingProviderType::Bedrock => {
                Err(ContextError::EmbeddingProviderNotConfigured)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_config_default() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.provider, EmbeddingProviderType::SelfHosted);
        assert_eq!(config.model, DEFAULT_OLLAMA_EMBEDDING_MODEL);
        assert_eq!(config.batch_size, 32);
        assert_eq!(config.dimension, 1024);
    }

    #[test]
    fn test_ai_settings_default() {
        let settings = AiSettings::default();
        assert!(settings.provider.is_empty());
        assert!(settings.litellm_host.is_none());
        assert!(settings.openai_api_key.is_none());
    }

    #[test]
    fn test_provider_factory_not_configured() {
        let settings = AiSettings::default();
        let result = EmbeddingProviderFactory::create(&settings);
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_factory_creates_ollama() {
        let settings = AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some("http://localhost:11434".to_string()),
            model_embedding: Some("nomic-embed-text".to_string()),
            ..Default::default()
        };
        let result = EmbeddingProviderFactory::create(&settings);
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.model(), "nomic-embed-text");
    }

    #[test]
    fn test_provider_factory_rejects_unknown_engine() {
        let settings = AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some("http://localhost:11434".to_string()),
            embedding_engine: Some("gpu-magic".to_string()),
            ..Default::default()
        };
        match EmbeddingProviderFactory::create(&settings) {
            Err(ContextError::InvalidSourceConfig(_)) => {}
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("unknown engine should be rejected"),
        }
    }

    // With the feature on this would load a real model (network + seconds),
    // which belongs in the ignored integration test, not a unit test.
    #[cfg(not(feature = "local-embeddings"))]
    #[test]
    fn test_provider_factory_local_engine_requires_feature() {
        let settings = AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            embedding_engine: Some(EMBEDDING_ENGINE_LOCAL.to_string()),
            ..Default::default()
        };
        match EmbeddingProviderFactory::create(&settings) {
            Err(ContextError::InvalidSourceConfig(_)) => {}
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("local engine should need the feature"),
        }
    }
}
