//! Embedding providers behind Zone's [`EmbeddingService`].
//!
//! The transport lives in `abnegate-llm`; [`EmbeddingAdapter`] puts any of
//! its providers behind [`EmbeddingService`]:
//! - Ollama's native `/api/embeddings` API;
//! - any OpenAI-compatible `/v1/embeddings` endpoint;
//! - in-process ONNX through fastembed (feature `local-embeddings`).

mod adapter;
mod batching;
mod failure;
#[cfg(any(test, feature = "test-utils"))]
mod mock;

use std::sync::Arc;

#[cfg(feature = "local-embeddings")]
use abnegate_llm::EmbeddingProvider;
#[cfg(feature = "local-embeddings")]
use abnegate_llm::modality::vendor::{FastembedConfig, FastembedProvider};
use abnegate_llm::modality::vendor::{
    OllamaEmbeddingConfig, OllamaEmbeddingProvider, OpenAiCompatibleEmbeddingConfig,
    OpenAiCompatibleEmbeddingProvider,
};

pub use adapter::EmbeddingAdapter;
pub use batching::Batching;
#[cfg(any(test, feature = "test-utils"))]
pub use mock::MockEmbeddingService;

use crate::embeddings::EmbeddingService;
use crate::error::{ContextError, Result};
use failure::context_error;

pub const PROVIDER_SELF_HOSTED: &str = "self_hosted";
pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_BEDROCK: &str = "bedrock";

/// Embedding engine identifiers for the `self_hosted` provider.
///
/// The provider says *where the LLM lives*; the engine says *how embeddings
/// are computed*. `ollama` calls the host's native Ollama API, `local` runs
/// the model in-process, and `openai_compatible` calls the host's
/// OpenAI-compatible `/v1/embeddings`.
pub const EMBEDDING_ENGINE_OLLAMA: &str = "ollama";
pub const EMBEDDING_ENGINE_LOCAL: &str = "local";
pub const EMBEDDING_ENGINE_OPENAI_COMPATIBLE: &str = "openai_compatible";

/// Default embedding model for the HTTP engines (native 1024-d,
/// instruction-aware).
pub const DEFAULT_OLLAMA_EMBEDDING_MODEL: &str = "qwen3-embedding:0.6b";

/// Default model for the `local` engine.
pub const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "nomic-embed-text";

const REQUEST_CONCURRENCY: usize = 10;
#[cfg(feature = "local-embeddings")]
const BATCH_SIZE: usize = 32;
const OPENAI_COMPATIBLE_PATH: &str = "/v1";
const PATH_SEPARATOR: char = '/';

/// AI settings for provider configuration
///
/// This mirrors the effective AI settings from zone_server
#[derive(Debug, Clone, Default)]
pub struct AiSettings {
    /// Provider name: "self_hosted", "openai", "anthropic", "bedrock"
    pub provider: String,
    /// Host URL for the self-hosted engines: Ollama's root for `ollama`, the
    /// OpenAI-compatible server's root, with or without `/v1`, for
    /// `openai_compatible`
    pub litellm_host: Option<String>,
    /// Bearer key for the self-hosted HTTP engines; blank sends none
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
    /// (default), [`EMBEDDING_ENGINE_LOCAL`] or
    /// [`EMBEDDING_ENGINE_OPENAI_COMPATIBLE`].
    pub embedding_engine: Option<String>,
}

/// Factory for creating embedding providers
pub struct EmbeddingProviderFactory;

impl EmbeddingProviderFactory {
    /// The embedding service `settings` select. Only `self_hosted` has
    /// embeddings; every other provider is
    /// [`ContextError::EmbeddingProviderNotConfigured`].
    pub fn create(settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        match settings.provider.as_str() {
            PROVIDER_SELF_HOSTED => match settings.embedding_engine.as_deref() {
                None | Some(EMBEDDING_ENGINE_OLLAMA) => Self::create_ollama(settings),
                Some(EMBEDDING_ENGINE_LOCAL) => Self::create_local(settings),
                Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE) => {
                    Self::create_openai_compatible(settings)
                }
                Some(other) => Err(ContextError::InvalidSourceConfig(format!(
                    "Unknown embedding engine '{other}' (expected '{EMBEDDING_ENGINE_OLLAMA}', '{EMBEDDING_ENGINE_LOCAL}' or '{EMBEDDING_ENGINE_OPENAI_COMPATIBLE}')"
                ))),
            },
            _ => Err(ContextError::EmbeddingProviderNotConfigured),
        }
    }

    fn host(settings: &AiSettings) -> Result<&str> {
        settings.litellm_host.as_deref().ok_or_else(|| {
            ContextError::InvalidSourceConfig(
                "litellm_host is required for self-hosted provider".to_string(),
            )
        })
    }

    fn http_model(settings: &AiSettings) -> &str {
        settings
            .model_embedding
            .as_deref()
            .unwrap_or(DEFAULT_OLLAMA_EMBEDDING_MODEL)
    }

    fn create_ollama(settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        let mut config =
            OllamaEmbeddingConfig::new(Self::host(settings)?, Self::http_model(settings));
        if let Some(key) = &settings.litellm_key {
            config = config.with_api_key(key.as_str());
        }
        let provider = OllamaEmbeddingProvider::new(config).map_err(context_error)?;
        Ok(Arc::new(EmbeddingAdapter::new(
            Arc::new(provider),
            Batching::PerText {
                concurrency: REQUEST_CONCURRENCY,
            },
        )))
    }

    fn openai_compatible_base(host: &str) -> String {
        let root = host.trim_end_matches(PATH_SEPARATOR);
        let root = root.strip_suffix(OPENAI_COMPATIBLE_PATH).unwrap_or(root);
        format!("{root}{OPENAI_COMPATIBLE_PATH}")
    }

    fn create_openai_compatible(settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        let mut config = OpenAiCompatibleEmbeddingConfig::new(
            Self::openai_compatible_base(Self::host(settings)?),
            Self::http_model(settings),
        );
        if let Some(key) = &settings.litellm_key {
            config = config.with_api_key(key.as_str());
        }
        let provider = OpenAiCompatibleEmbeddingProvider::new(config).map_err(context_error)?;
        Ok(Arc::new(EmbeddingAdapter::new(
            Arc::new(provider),
            Batching::Batched {
                concurrency: REQUEST_CONCURRENCY,
            },
        )))
    }

    #[cfg(feature = "local-embeddings")]
    fn create_local(settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        let model = settings
            .model_embedding
            .as_deref()
            .unwrap_or(DEFAULT_LOCAL_EMBEDDING_MODEL);
        let cache_directory = crate::embeddings::cache::default_directory();
        let config = FastembedConfig::new()
            .with_model(model)
            .with_cache_directory(cache_directory.clone())
            .with_batch_size(BATCH_SIZE);
        let maximum_input_tokens = config.model_maximum_input_tokens().map_err(|_| {
            ContextError::InvalidSourceConfig(format!(
                "Unknown embedding model '{model}' for local engine"
            ))
        })?;
        let provider =
            FastembedProvider::new(config.with_maximum_input_tokens(maximum_input_tokens))
                .map_err(|error| {
                    ContextError::Config(format!(
                        "Failed to load local embedding model {model}: {error}"
                    ))
                })?;
        tracing::info!(
            model = %EmbeddingProvider::model(&provider),
            loaded = %provider.loaded_model(),
            dimension = provider.dimensions(),
            max_tokens = provider.maximum_input_tokens(),
            cache_directory = %cache_directory.display(),
            "Loaded in-process embedding model"
        );
        Ok(Arc::new(EmbeddingAdapter::new(
            Arc::new(provider),
            Batching::Local,
        )))
    }

    #[cfg(not(feature = "local-embeddings"))]
    fn create_local(_settings: &AiSettings) -> Result<Arc<dyn EmbeddingService>> {
        Err(ContextError::InvalidSourceConfig(
            "Embedding engine 'local' requires the `local-embeddings` feature".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "http://localhost:11434";

    fn self_hosted(engine: Option<&str>, model: Option<&str>) -> AiSettings {
        AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some(HOST.to_string()),
            model_embedding: model.map(str::to_string),
            embedding_engine: engine.map(str::to_string),
            ..Default::default()
        }
    }

    fn invalid_source_config(settings: &AiSettings) -> String {
        match EmbeddingProviderFactory::create(settings) {
            Err(ContextError::InvalidSourceConfig(message)) => message,
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("settings should be rejected"),
        }
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
        let result = EmbeddingProviderFactory::create(&AiSettings::default());
        assert!(matches!(
            result,
            Err(ContextError::EmbeddingProviderNotConfigured)
        ));
    }

    #[test]
    fn providers_other_than_self_hosted_have_no_embeddings() {
        for provider in [PROVIDER_OPENAI, PROVIDER_BEDROCK, "anthropic"] {
            let settings = AiSettings {
                provider: provider.to_string(),
                litellm_host: Some(HOST.to_string()),
                openai_api_key: Some("sk-test".to_string()),
                ..Default::default()
            };
            assert!(
                matches!(
                    EmbeddingProviderFactory::create(&settings),
                    Err(ContextError::EmbeddingProviderNotConfigured)
                ),
                "{provider}"
            );
        }
    }

    #[test]
    fn test_provider_factory_creates_ollama() {
        for engine in [None, Some(EMBEDDING_ENGINE_OLLAMA)] {
            let provider =
                EmbeddingProviderFactory::create(&self_hosted(engine, Some("nomic-embed-text")))
                    .unwrap();
            assert_eq!(provider.model(), "nomic-embed-text");
            assert_eq!(provider.dimension(), 768);
            assert_eq!(provider.max_tokens(), 8192);
        }
    }

    #[test]
    fn the_ollama_engine_defaults_to_qwen3_embedding() {
        let provider = EmbeddingProviderFactory::create(&self_hosted(None, None)).unwrap();
        assert_eq!(provider.model(), DEFAULT_OLLAMA_EMBEDDING_MODEL);
        assert_eq!(provider.dimension(), 1024);
    }

    #[test]
    fn the_openai_compatible_engine_learns_its_width() {
        let provider = EmbeddingProviderFactory::create(&self_hosted(
            Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE),
            Some("text-embedding-3-small"),
        ))
        .unwrap();
        assert_eq!(provider.model(), "text-embedding-3-small");
        assert_eq!(provider.dimension(), 0);
        assert_eq!(provider.max_tokens(), 512);
    }

    #[test]
    fn the_openai_compatible_engine_defaults_to_the_http_model() {
        let provider = EmbeddingProviderFactory::create(&self_hosted(
            Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE),
            None,
        ))
        .unwrap();
        assert_eq!(provider.model(), DEFAULT_OLLAMA_EMBEDDING_MODEL);
    }

    #[test]
    fn the_openai_compatible_base_ends_in_one_v1() {
        for host in ["http://h", "http://h/", "http://h/v1", "http://h/v1/"] {
            assert_eq!(
                EmbeddingProviderFactory::openai_compatible_base(host),
                "http://h/v1",
                "{host}"
            );
        }
    }

    #[test]
    fn the_http_engines_need_a_host() {
        for engine in [None, Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE)] {
            let settings = AiSettings {
                litellm_host: None,
                ..self_hosted(engine, None)
            };
            assert_eq!(
                invalid_source_config(&settings),
                "litellm_host is required for self-hosted provider"
            );
        }
    }

    #[test]
    fn the_http_engines_refuse_a_host_that_is_not_http() {
        for engine in [None, Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE)] {
            let settings = AiSettings {
                litellm_host: Some("ftp://example.com".to_string()),
                ..self_hosted(engine, None)
            };
            let message = invalid_source_config(&settings);
            assert!(message.contains("ftp"), "{message}");
        }
    }

    #[test]
    fn test_provider_factory_rejects_unknown_engine() {
        let message = invalid_source_config(&self_hosted(Some("gpu-magic"), None));
        assert_eq!(
            message,
            "Unknown embedding engine 'gpu-magic' (expected 'ollama', 'local' or 'openai_compatible')"
        );
    }

    #[cfg(not(feature = "local-embeddings"))]
    #[test]
    fn test_provider_factory_local_engine_requires_feature() {
        let message = invalid_source_config(&self_hosted(Some(EMBEDDING_ENGINE_LOCAL), None));
        assert!(message.contains("local-embeddings"), "{message}");
    }

    #[cfg(feature = "local-embeddings")]
    #[test]
    fn the_local_engine_refuses_a_model_it_cannot_run() {
        let message = invalid_source_config(&self_hosted(
            Some(EMBEDDING_ENGINE_LOCAL),
            Some("qwen3-embedding:0.6b"),
        ));
        assert_eq!(
            message,
            "Unknown embedding model 'qwen3-embedding:0.6b' for local engine"
        );
    }
}
