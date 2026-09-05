//! Embedding service factory
//!
//! Creates embedding service providers based on AI settings from database.

use std::sync::Arc;
use zone_context::embeddings::{
    EmbeddingService,
    providers::{AiSettings, EmbeddingProviderFactory},
};
use zone_context::error::Result as ContextResult;

use crate::db::ai_settings::EffectiveAiSettings;

/// Environment variable selecting the self-hosted embedding engine
/// (`ollama` or `local`).
pub const EMBEDDING_ENGINE_ENV: &str = "EMBEDDING_ENGINE";

/// Create an embedding service from effective AI settings.
///
/// `engine` selects how `self_hosted` embeddings are computed; `None` keeps
/// the Ollama-over-HTTP default. Callers normally pass
/// [`embedding_engine_from_env`].
pub fn create_embedding_service(
    settings: &EffectiveAiSettings,
    engine: Option<&str>,
) -> ContextResult<Arc<dyn EmbeddingService>> {
    // Convert EffectiveAiSettings to zone_context AiSettings
    let ai_settings = AiSettings {
        provider: settings.provider.clone(),
        litellm_host: settings.litellm_host.clone(),
        litellm_key: settings.litellm_key.clone(),
        openai_api_key: settings.openai_api_key.clone(),
        openai_base_url: settings.openai_base_url.clone(),
        bedrock_region: settings.bedrock_region.clone(),
        model_embedding: settings.model_embedding.clone(),
        embedding_engine: engine.map(str::to_string),
    };

    EmbeddingProviderFactory::create(&ai_settings)
}

/// Read `EMBEDDING_ENGINE` from the environment, treating blank as unset.
pub fn embedding_engine_from_env() -> Option<String> {
    std::env::var(EMBEDDING_ENGINE_ENV)
        .ok()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

    #[test]
    fn test_create_embedding_service_ollama() {
        // Given: Effective AI settings for self-hosted Ollama
        let settings = EffectiveAiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some("http://localhost:11434".to_string()),
            litellm_key: Some("test-key".to_string()),
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: Some("nomic-embed-text".to_string()),
            model_image: None,
        };

        // When: Creating embedding service
        let result = create_embedding_service(&settings, None);

        // Then: Should successfully create Ollama provider
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.model(), "nomic-embed-text");
        assert_eq!(service.dimension(), 768);
    }

    #[test]
    fn test_create_embedding_service_missing_host() {
        // Given: Settings without litellm_host
        let settings = EffectiveAiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: None,
            litellm_key: None,
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: None,
            model_image: None,
        };

        // When: Creating embedding service
        let result = create_embedding_service(&settings, None);

        // Then: Should fail with error
        assert!(result.is_err());
    }

    #[test]
    fn test_create_embedding_service_unsupported_provider() {
        // Given: Settings with unsupported provider
        let settings = EffectiveAiSettings {
            provider: "openai".to_string(),
            litellm_host: None,
            litellm_key: None,
            openai_api_key: Some("sk-test".to_string()),
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: Some("text-embedding-3-small".to_string()),
            model_image: None,
        };

        // When: Creating embedding service
        let result = create_embedding_service(&settings, None);

        // Then: Should fail (OpenAI not yet implemented)
        assert!(result.is_err());
    }

    #[test]
    fn test_create_embedding_service_default_model() {
        // Given: Settings without explicit model
        let settings = EffectiveAiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some("http://localhost:11434".to_string()),
            litellm_key: None,
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: None, // No model specified
            model_image: None,
        };

        // When: Creating embedding service
        let result = create_embedding_service(&settings, None);

        // Then: Should use default model
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(
            service.model(),
            zone_context::embeddings::providers::DEFAULT_OLLAMA_EMBEDDING_MODEL
        );
    }
}
