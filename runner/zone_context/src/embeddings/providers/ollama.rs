//! Ollama embedding provider via LiteLLM
//!
//! Provides embedding generation using Ollama/LiteLLM API

use async_trait::async_trait;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::content::{embed_char_budget, truncate_chars};
use crate::embeddings::EmbeddingService;
use crate::embeddings::providers::AiSettings;
use crate::error::{ContextError, Result};

/// Ollama embedding provider
#[derive(Debug, Clone)]
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimension: usize,
    api_key: Option<String>,
}

/// Request body for Ollama embeddings API
#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<String>,
}

/// Response from Ollama embeddings API
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    embedding: Vec<f32>,
}

/// Default Ollama embedding model (native 1024-d, instruction-aware).
pub const DEFAULT_OLLAMA_EMBEDDING_MODEL: &str = "qwen3-embedding:0.6b";

fn model_base(model: &str) -> String {
    model
        .split(':')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase()
}

/// Get the dimension for a given model name
fn get_model_dimension(model: &str) -> usize {
    let name = model_base(model);
    if name.starts_with("qwen3-embedding") || name.contains("qwen3-embedding") {
        return 1024;
    }
    match name.as_str() {
        "nomic-embed-text" | "nomic-embed-text-v1.5" | "nomic-embed-text-v1" => 768,
        "mxbai-embed-large" => 1024,
        "snowflake-arctic-embed" => 1024,
        "bge-small-en" | "bge-small-en-v1.5" => 384,
        "all-minilm" | "all-minilm-l6-v2" => 384,
        _ => 768, // Pad shorter unknowns to VECTOR_DIMENSION
    }
}

/// Get the maximum tokens supported by a model
fn get_model_max_tokens(model: &str) -> usize {
    let name = model_base(model);
    if name.starts_with("qwen3-embedding") {
        return 8192;
    }
    match name.as_str() {
        "nomic-embed-text" | "nomic-embed-text-v1.5" | "nomic-embed-text-v1" => 8192,
        "mxbai-embed-large" => 512,
        "bge-small-en" | "bge-small-en-v1.5" => 512,
        "all-minilm" | "all-minilm-l6-v2" => 256,
        _ => 512, // Conservative default
    }
}

/// Validate and normalize a base URL
fn validate_base_url(url: &str) -> Result<String> {
    let parsed = Url::parse(url).map_err(|e| {
        ContextError::InvalidSourceConfig(format!("Invalid base URL '{}': {}", url, e))
    })?;

    match parsed.scheme() {
        "http" | "https" => Ok(url.to_string()),
        scheme => Err(ContextError::InvalidSourceConfig(format!(
            "Invalid URL scheme '{}': only http and https are supported",
            scheme
        ))),
    }
}

/// Sanitize response body for error messages (truncate and remove sensitive data)
fn sanitize_response_body(body: &str) -> String {
    const MAX_LEN: usize = 200;
    if body.len() > MAX_LEN {
        format!("{}... (truncated)", &body[..MAX_LEN])
    } else {
        body.to_string()
    }
}

impl OllamaProvider {
    /// Create a new Ollama provider with explicit configuration
    pub fn new(
        base_url: &str,
        model: &str,
        dimension: usize,
        api_key: Option<String>,
    ) -> Result<Self> {
        // Validate URL
        let validated_url = validate_base_url(base_url)?;

        // Create HTTP client with timeouts (C1)
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30)) // 30s request timeout
            .connect_timeout(Duration::from_secs(10)) // 10s connect timeout
            .build()
            .map_err(|e| ContextError::Config(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: validated_url,
            model: model.to_string(),
            dimension,
            api_key,
        })
    }

    /// Create from AI settings
    pub fn from_settings(settings: &AiSettings) -> Result<Self> {
        let base_url = settings
            .litellm_host
            .as_ref()
            .ok_or_else(|| {
                ContextError::InvalidSourceConfig(
                    "litellm_host is required for self-hosted provider".to_string(),
                )
            })?
            .clone();

        let model = settings
            .model_embedding
            .clone()
            .unwrap_or_else(|| DEFAULT_OLLAMA_EMBEDDING_MODEL.to_string());

        let dimension = get_model_dimension(&model);

        Self::new(&base_url, &model, dimension, settings.litellm_key.clone())
    }

    async fn embed_prompt(&self, prompt: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.base_url);
        let request_body = EmbeddingRequest {
            model: self.model.clone(),
            prompt: prompt.to_string(),
            keep_alive: Some("30m".into()),
        };
        let timeout = if prompt.chars().count() < 2000 {
            Duration::from_secs(8)
        } else {
            Duration::from_secs(30)
        };

        let mut request = self.client.post(&url).timeout(timeout).json(&request_body);

        if let Some(ref key) = self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let response = request
            .send()
            .await
            .map_err(|e| ContextError::Embedding(format!("Request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read response body".to_string());
            let sanitized_body = sanitize_response_body(&body);

            return match status.as_u16() {
                401 | 403 => Err(ContextError::Auth(format!(
                    "Authentication failed ({}): {}",
                    status, sanitized_body
                ))),
                429 => Err(ContextError::RateLimited {
                    retry_after_secs: 60,
                }),
                _ => Err(ContextError::Embedding(format!(
                    "API returned error {}: {}",
                    status, sanitized_body
                ))),
            };
        }

        let embedding_response = response
            .json::<EmbeddingResponse>()
            .await
            .map_err(|e| ContextError::Embedding(format!("Failed to parse response: {}", e)))?;

        let actual_len = embedding_response.embedding.len();
        if actual_len != self.dimension {
            return Err(ContextError::EmbeddingDimensionMismatch {
                expected: self.dimension,
                actual: actual_len,
            });
        }

        Ok(embedding_response.embedding)
    }
}

#[async_trait]
impl EmbeddingService for OllamaProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let budget = embed_char_budget(self.max_tokens());
        let mut prompt = truncate_chars(text, budget);
        let mut last_err = None;
        let attempts = if text.chars().count() < 2000 { 2 } else { 3 };
        for attempt in 0..attempts {
            match self.embed_prompt(&prompt).await {
                Ok(vector) => return Ok(vector),
                Err(err) if err.is_context_length() => {
                    last_err = Some(err);
                    prompt = truncate_chars(&prompt, prompt.chars().count() / 2);
                    if prompt.is_empty() {
                        break;
                    }
                    tracing::warn!(
                        model = %self.model,
                        attempt,
                        chars = prompt.chars().count(),
                        "retrying embedding after context-length error"
                    );
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            ContextError::Embedding("input exceeds embedding context length".to_string())
        }))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        // M2: Concurrent batch embedding with order preservation
        use futures::stream;

        const CONCURRENCY_LIMIT: usize = 10;

        // Convert to owned strings to avoid lifetime issues with the stream
        let owned_texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();

        // Use enumerate to track indices and preserve order
        let stream = stream::iter(owned_texts.into_iter().enumerate().map(
            |(idx, text)| async move {
                let result = self.embed(&text).await;
                (idx, result)
            },
        ));

        let mut buffered = stream.buffer_unordered(CONCURRENCY_LIMIT);

        // Collect results with their original indices
        let mut indexed_results: Vec<(usize, Vec<f32>)> = Vec::with_capacity(texts.len());
        while let Some((idx, result)) = buffered.next().await {
            indexed_results.push((idx, result?));
        }

        // Sort by index to restore original order
        indexed_results.sort_by_key(|(idx, _)| *idx);

        // Extract embeddings in correct order
        Ok(indexed_results.into_iter().map(|(_, v)| v).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn max_tokens(&self) -> usize {
        // M4: Per-model max tokens
        get_model_max_tokens(&self.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_provider_new() {
        let provider = OllamaProvider::new(
            "http://localhost:11434",
            "nomic-embed-text",
            768,
            Some("test-key".to_string()),
        )
        .unwrap();

        assert_eq!(provider.base_url, "http://localhost:11434");
        assert_eq!(provider.model, "nomic-embed-text");
        assert_eq!(provider.dimension, 768);
        assert_eq!(provider.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_ollama_provider_dimension() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "nomic-embed-text", 768, None).unwrap();
        assert_eq!(provider.dimension(), 768);
    }

    #[test]
    fn test_ollama_provider_model() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "nomic-embed-text", 768, None).unwrap();
        assert_eq!(provider.model(), "nomic-embed-text");
    }

    #[test]
    fn test_ollama_provider_max_tokens_nomic() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "nomic-embed-text", 768, None).unwrap();
        assert_eq!(provider.max_tokens(), 8192);
    }

    #[test]
    fn test_ollama_provider_max_tokens_mxbai() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "mxbai-embed-large", 1024, None).unwrap();
        assert_eq!(provider.max_tokens(), 512);
    }

    #[test]
    fn test_ollama_provider_max_tokens_bge() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "bge-small-en", 384, None).unwrap();
        assert_eq!(provider.max_tokens(), 512);
    }

    #[test]
    fn test_ollama_provider_max_tokens_all_minilm() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "all-minilm", 384, None).unwrap();
        assert_eq!(provider.max_tokens(), 256);
    }

    #[test]
    fn test_ollama_provider_max_tokens_unknown() {
        let provider =
            OllamaProvider::new("http://localhost:11434", "unknown-model", 768, None).unwrap();
        assert_eq!(provider.max_tokens(), 512); // Conservative default
    }

    #[test]
    fn test_get_model_dimension_nomic() {
        assert_eq!(get_model_dimension("nomic-embed-text"), 768);
        assert_eq!(get_model_dimension("nomic-embed-text:latest"), 768);
    }

    #[test]
    fn test_get_model_dimension_qwen() {
        assert_eq!(get_model_dimension("qwen3-embedding:0.6b"), 1024);
        assert_eq!(get_model_dimension("qwen3-embedding"), 1024);
    }

    #[test]
    fn test_get_model_dimension_mxbai() {
        assert_eq!(get_model_dimension("mxbai-embed-large"), 1024);
    }

    #[test]
    fn test_get_model_dimension_bge() {
        assert_eq!(get_model_dimension("bge-small-en"), 384);
        assert_eq!(get_model_dimension("bge-small-en-v1.5"), 384);
    }

    #[test]
    fn test_get_model_dimension_default() {
        assert_eq!(get_model_dimension("unknown-model"), 768);
    }

    #[test]
    fn test_from_settings_valid() {
        use super::super::PROVIDER_SELF_HOSTED;

        let settings = AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: Some("http://localhost:11434".to_string()),
            litellm_key: Some("test-key".to_string()),
            model_embedding: Some("nomic-embed-text".to_string()),
            ..Default::default()
        };

        let provider = OllamaProvider::from_settings(&settings).unwrap();
        assert_eq!(provider.base_url, "http://localhost:11434");
        assert_eq!(provider.model, "nomic-embed-text");
        assert_eq!(provider.dimension, 768);
        assert_eq!(provider.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_from_settings_missing_host() {
        use super::super::PROVIDER_SELF_HOSTED;

        let settings = AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            litellm_host: None,
            ..Default::default()
        };

        let result = OllamaProvider::from_settings(&settings);
        assert!(result.is_err());
        match result {
            Err(ContextError::InvalidSourceConfig(msg)) => {
                assert!(msg.contains("litellm_host"));
            }
            _ => panic!("Expected InvalidSourceConfig error"),
        }
    }

    #[test]
    fn test_validate_base_url_http() {
        let result = validate_base_url("http://localhost:11434");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_base_url_https() {
        let result = validate_base_url("https://api.example.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_base_url_invalid_scheme() {
        let result = validate_base_url("ftp://example.com");
        assert!(result.is_err());
        match result {
            Err(ContextError::InvalidSourceConfig(msg)) => {
                assert!(msg.contains("ftp"));
                assert!(msg.contains("only http and https are supported"));
            }
            _ => panic!("Expected InvalidSourceConfig error"),
        }
    }

    #[test]
    fn test_validate_base_url_malformed() {
        let result = validate_base_url("not-a-url");
        assert!(result.is_err());
        match result {
            Err(ContextError::InvalidSourceConfig(_)) => {}
            _ => panic!("Expected InvalidSourceConfig error"),
        }
    }

    #[test]
    fn test_sanitize_response_body_short() {
        let body = "Short error message";
        assert_eq!(sanitize_response_body(body), body);
    }

    #[test]
    fn test_sanitize_response_body_long() {
        let body = "a".repeat(300);
        let sanitized = sanitize_response_body(&body);
        assert!(sanitized.len() < body.len());
        assert!(sanitized.contains("truncated"));
    }

    #[test]
    fn test_new_rejects_invalid_url() {
        let result = OllamaProvider::new("ftp://invalid.com", "nomic-embed-text", 768, None);
        assert!(result.is_err());
    }
}
