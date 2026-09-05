//! Integration tests for Ollama embedding provider

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_context::embeddings::EmbeddingService;
use zone_context::embeddings::providers::{AiSettings, EmbeddingProviderFactory, OllamaProvider};
use zone_context::error::ContextError;

#[tokio::test]
async fn test_ollama_embed_single() {
    let mock_server = MockServer::start().await;

    // Mock the embeddings endpoint
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "embedding": [0.1, 0.2, 0.3, 0.4]
        })))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 4, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_ok());

    let embedding = result.unwrap();
    assert_eq!(embedding.len(), 4);
    assert_eq!(embedding, vec![0.1, 0.2, 0.3, 0.4]);
}

#[tokio::test]
async fn test_ollama_embed_batch() {
    let mock_server = MockServer::start().await;

    // Mock the embeddings endpoint - will be called twice
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "embedding": [0.1, 0.2, 0.3]
        })))
        .expect(2)
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 3, None).unwrap();

    let texts = vec!["text 1", "text 2"];
    let result = provider.embed_batch(&texts).await;
    assert!(result.is_ok());

    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 2);
    assert_eq!(embeddings[0], vec![0.1, 0.2, 0.3]);
    assert_eq!(embeddings[1], vec![0.1, 0.2, 0.3]);
}

#[tokio::test]
async fn test_ollama_handles_api_error() {
    let mock_server = MockServer::start().await;

    // Mock server error
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal server error"))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::Embedding(msg)) => {
            assert!(msg.contains("500"));
        }
        _ => panic!("Expected Embedding error"),
    }
}

#[tokio::test]
async fn test_ollama_handles_timeout() {
    // Use an invalid URL that will timeout
    let provider =
        OllamaProvider::new("http://192.0.2.1:1234", "nomic-embed-text", 768, None).unwrap();

    let result = tokio::time::timeout(
        tokio::time::Duration::from_millis(100),
        provider.embed("test text"),
    )
    .await;

    // Should timeout or fail with network error
    assert!(result.is_err() || result.unwrap().is_err());
}

#[tokio::test]
async fn test_ollama_factory_creates_provider() {
    let mock_server = MockServer::start().await;

    let settings = AiSettings {
        provider: "self_hosted".to_string(),
        litellm_host: Some(mock_server.uri()),
        litellm_key: Some("test-key".to_string()),
        model_embedding: Some("nomic-embed-text".to_string()),
        ..Default::default()
    };

    let provider = EmbeddingProviderFactory::create(&settings);
    assert!(provider.is_ok());

    let provider = provider.unwrap();
    assert_eq!(provider.model(), "nomic-embed-text");
    assert_eq!(provider.dimension(), 768);
}

#[tokio::test]
async fn test_ollama_embed_empty_text() {
    let mock_server = MockServer::start().await;

    // Mock the embeddings endpoint
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "embedding": [0.0, 0.0, 0.0]
        })))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 3, None).unwrap();

    let result = provider.embed("").await;
    assert!(result.is_ok());

    let embedding = result.unwrap();
    assert_eq!(embedding.len(), 3);
}

#[tokio::test]
async fn test_ollama_with_api_key() {
    let mock_server = MockServer::start().await;

    // Mock the embeddings endpoint with authorization check
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .and(header("Authorization", "Bearer my-secret-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "embedding": [0.1, 0.2]
        })))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(
        &mock_server.uri(),
        "nomic-embed-text",
        2,
        Some("my-secret-key".to_string()),
    )
    .unwrap();

    let result = provider.embed("test").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ollama_different_models() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "embedding": vec![0.1; 1024]
        })))
        .mount(&mock_server)
        .await;

    let provider =
        OllamaProvider::new(&mock_server.uri(), "mxbai-embed-large", 1024, None).unwrap();

    assert_eq!(provider.dimension(), 1024);
    assert_eq!(provider.model(), "mxbai-embed-large");

    let result = provider.embed("test").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ollama_from_settings_with_default_model() {
    let settings = AiSettings {
        provider: "self_hosted".to_string(),
        litellm_host: Some("http://localhost:11434".to_string()),
        model_embedding: None, // Should default to qwen3-embedding:0.6b
        ..Default::default()
    };

    let provider = OllamaProvider::from_settings(&settings).unwrap();
    assert_eq!(provider.model(), "qwen3-embedding:0.6b");
    assert_eq!(provider.dimension(), 1024);
}

#[tokio::test]
async fn test_ollama_handles_malformed_json() {
    let mock_server = MockServer::start().await;

    // Mock endpoint that returns invalid JSON
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not valid json"))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::Embedding(msg)) => {
            assert!(msg.contains("parse"));
        }
        _ => panic!("Expected Embedding error for malformed JSON"),
    }
}

#[tokio::test]
async fn test_ollama_validates_dimension() {
    let mock_server = MockServer::start().await;

    // Mock endpoint that returns wrong dimension
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "embedding": [0.1, 0.2, 0.3] // 3 dimensions instead of expected 768
        })))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::EmbeddingDimensionMismatch { expected, actual }) => {
            assert_eq!(expected, 768);
            assert_eq!(actual, 3);
        }
        _ => panic!("Expected EmbeddingDimensionMismatch error"),
    }
}

#[tokio::test]
async fn test_ollama_handles_auth_error() {
    let mock_server = MockServer::start().await;

    // Mock 401 unauthorized
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::Auth(msg)) => {
            assert!(msg.contains("401"));
        }
        _ => panic!("Expected Auth error for 401"),
    }
}

#[tokio::test]
async fn test_ollama_handles_auth_error_403() {
    let mock_server = MockServer::start().await;

    // Mock 403 forbidden
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::Auth(msg)) => {
            assert!(msg.contains("403"));
        }
        _ => panic!("Expected Auth error for 403"),
    }
}

#[tokio::test]
async fn test_ollama_handles_rate_limit() {
    let mock_server = MockServer::start().await;

    // Mock 429 rate limited
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limit exceeded"))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::RateLimited { retry_after_secs }) => {
            assert_eq!(retry_after_secs, 60);
        }
        _ => panic!("Expected RateLimited error for 429"),
    }
}

#[tokio::test]
async fn test_ollama_validates_url() {
    // Test invalid scheme
    let result = OllamaProvider::new("ftp://invalid.com", "nomic-embed-text", 768, None);
    assert!(result.is_err());
    match result {
        Err(ContextError::InvalidSourceConfig(msg)) => {
            assert!(msg.contains("ftp"));
        }
        _ => panic!("Expected InvalidSourceConfig for invalid scheme"),
    }

    // Test malformed URL
    let result = OllamaProvider::new("not-a-url", "nomic-embed-text", 768, None);
    assert!(result.is_err());
    match result {
        Err(ContextError::InvalidSourceConfig(_)) => {}
        _ => panic!("Expected InvalidSourceConfig for malformed URL"),
    }
}

#[tokio::test]
async fn test_ollama_sanitizes_error_body() {
    let mock_server = MockServer::start().await;

    // Mock endpoint with very long error message
    let long_error = "Error: ".to_string() + &"a".repeat(500);
    Mock::given(method("POST"))
        .and(path("/api/embeddings"))
        .respond_with(ResponseTemplate::new(500).set_body_string(long_error))
        .mount(&mock_server)
        .await;

    let provider = OllamaProvider::new(&mock_server.uri(), "nomic-embed-text", 768, None).unwrap();

    let result = provider.embed("test text").await;
    assert!(result.is_err());

    match result {
        Err(ContextError::Embedding(msg)) => {
            // Should be truncated
            assert!(msg.len() < 500);
            assert!(msg.contains("truncated"));
        }
        _ => panic!("Expected Embedding error"),
    }
}
