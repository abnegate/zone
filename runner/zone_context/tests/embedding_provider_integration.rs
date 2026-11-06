//! The embedding adapter over abnegate-llm's Ollama and OpenAI-compatible
//! providers, against a mocked server.

use std::sync::Arc;
use std::time::{Duration, Instant};

use abnegate_llm::modality::vendor::{OllamaEmbeddingConfig, OllamaEmbeddingProvider};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_context::embeddings::providers::{
    AiSettings, Batching, EMBEDDING_ENGINE_OPENAI_COMPATIBLE, EmbeddingAdapter,
    EmbeddingProviderFactory, PROVIDER_SELF_HOSTED,
};
use zone_context::embeddings::{EmbeddingService, VECTOR_DIMENSION};
use zone_context::error::ContextError;

const OLLAMA_PATH: &str = "/api/embeddings";
const OPENAI_COMPATIBLE_PATH: &str = "/v1/embeddings";
const MODEL: &str = "nomic-embed-text";
const KEY: &str = "zone-fixture-key-0123456789abcdef";
const CONCURRENCY: usize = 10;
const CONTEXT_LIMIT_CHARACTERS: usize = 1000;
const LONG_TEXT_CHARACTERS: usize = 5000;
const LONG_BODY_CHARACTERS: usize = 500;
const SHORT_PROMPT_TIMEOUT: Duration = Duration::from_millis(200);
const DEADLINE_MARGIN: Duration = Duration::from_secs(1);
const SLOW_ANSWER: Duration = Duration::from_secs(3);
const INPUTS_PER_REQUEST: usize = 64;
const LARGE_BATCH: usize = INPUTS_PER_REQUEST * 2 + 5;
const OPENAI_CONTEXT_LENGTH: &str = r#"{"error":{"message":"This model's maximum context length is 512 tokens, however you requested 2250 tokens. Please reduce your prompt.","type":"invalid_request_error","param":null,"code":null}}"#;

fn adapter(config: OllamaEmbeddingConfig) -> EmbeddingAdapter {
    EmbeddingAdapter::new(
        Arc::new(OllamaEmbeddingProvider::new(config).expect("valid config")),
        Batching::PerText {
            concurrency: CONCURRENCY,
        },
    )
}

fn ollama(server: &MockServer, model: &str, dimensions: u32) -> EmbeddingAdapter {
    adapter(OllamaEmbeddingConfig::new(server.uri(), model).with_dimensions(dimensions))
}

fn embedding(values: &[f32]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "embedding": values }))
}

async fn ollama_answering(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OLLAMA_PATH))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

fn self_hosted(host: String, engine: Option<&str>, key: Option<&str>) -> AiSettings {
    AiSettings {
        provider: PROVIDER_SELF_HOSTED.to_string(),
        litellm_host: Some(host),
        litellm_key: key.map(str::to_string),
        model_embedding: Some(MODEL.to_string()),
        embedding_engine: engine.map(str::to_string),
        ..Default::default()
    }
}

async fn embed_error(server: &MockServer) -> ContextError {
    ollama(server, MODEL, 768)
        .embed("test text")
        .await
        .expect_err("the embed should fail")
}

#[tokio::test]
async fn test_ollama_embed_single() {
    let server = ollama_answering(embedding(&[0.1, 0.2, 0.3, 0.4])).await;
    let vector = ollama(&server, MODEL, 4).embed("test text").await.unwrap();
    assert_eq!(vector, vec![0.1, 0.2, 0.3, 0.4]);
}

#[tokio::test]
async fn test_ollama_embed_batch() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OLLAMA_PATH))
        .respond_with(embedding(&[0.1, 0.2, 0.3]))
        .expect(2)
        .mount(&server)
        .await;
    let vectors = ollama(&server, MODEL, 3)
        .embed_batch(&["text 1", "text 2"])
        .await
        .unwrap();
    assert_eq!(vectors, vec![vec![0.1, 0.2, 0.3], vec![0.1, 0.2, 0.3]]);
}

#[tokio::test]
async fn test_ollama_handles_api_error() {
    let server =
        ollama_answering(ResponseTemplate::new(500).set_body_string("Internal server error")).await;
    match embed_error(&server).await {
        ContextError::Embedding(message) => assert!(message.contains("500"), "{message}"),
        other => panic!("Expected Embedding error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_short_prompt_past_its_deadline_is_an_embedding_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OLLAMA_PATH))
        .respond_with(embedding(&[0.5; 768]))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(OLLAMA_PATH))
        .respond_with(embedding(&[0.5; 768]).set_delay(SLOW_ANSWER))
        .mount(&server)
        .await;
    let provider = adapter(
        OllamaEmbeddingConfig::new(server.uri(), MODEL)
            .with_dimensions(768)
            .with_short_prompt_timeout(SHORT_PROMPT_TIMEOUT),
    );
    provider
        .embed("warm up")
        .await
        .expect("the first answer is immediate");
    let started = Instant::now();
    let error = provider
        .embed("test text")
        .await
        .expect_err("the answer comes after the deadline");
    let elapsed = started.elapsed();
    assert!(
        matches!(error, ContextError::Embedding(_)),
        "Expected Embedding error, got {error:?}"
    );
    assert!(!error.is_fatal_embedding(), "{error:?}");
    assert!(
        elapsed < SHORT_PROMPT_TIMEOUT + DEADLINE_MARGIN,
        "gave up after {elapsed:?}, not at the {SHORT_PROMPT_TIMEOUT:?} deadline"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_ollama_factory_creates_provider() {
    let server = MockServer::start().await;
    let provider =
        EmbeddingProviderFactory::create(&self_hosted(server.uri(), None, Some("test-key")))
            .unwrap();
    assert_eq!(provider.model(), MODEL);
    assert_eq!(provider.dimension(), 768);
}

#[tokio::test]
async fn test_ollama_embed_empty_text() {
    let server = ollama_answering(embedding(&[0.0, 0.0, 0.0])).await;
    let vector = ollama(&server, MODEL, 3).embed("").await.unwrap();
    assert_eq!(vector.len(), 3);
}

#[tokio::test]
async fn test_ollama_with_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OLLAMA_PATH))
        .and(header("Authorization", "Bearer my-secret-key"))
        .respond_with(embedding(&[0.1, 0.2]))
        .mount(&server)
        .await;
    let provider = adapter(
        OllamaEmbeddingConfig::new(server.uri(), MODEL)
            .with_dimensions(2)
            .with_api_key("my-secret-key"),
    );
    assert!(provider.embed("test").await.is_ok());
}

#[tokio::test]
async fn test_ollama_different_models() {
    let server = ollama_answering(embedding(&[0.1; 1024])).await;
    let provider = adapter(OllamaEmbeddingConfig::new(
        server.uri(),
        "mxbai-embed-large",
    ));
    assert_eq!(provider.dimension(), 1024);
    assert_eq!(provider.model(), "mxbai-embed-large");
    assert!(provider.embed("test").await.is_ok());
}

#[tokio::test]
async fn test_ollama_from_settings_with_default_model() {
    let settings = AiSettings {
        model_embedding: None,
        ..self_hosted("http://localhost:11434".to_string(), None, None)
    };
    let provider = EmbeddingProviderFactory::create(&settings).unwrap();
    assert_eq!(provider.model(), "qwen3-embedding:0.6b");
    assert_eq!(provider.dimension(), 1024);
}

#[tokio::test]
async fn test_ollama_handles_malformed_json() {
    let server =
        ollama_answering(ResponseTemplate::new(200).set_body_string("not valid json")).await;
    match embed_error(&server).await {
        ContextError::Embedding(message) => assert!(message.contains("parse"), "{message}"),
        other => panic!("Expected Embedding error for malformed JSON, got {other:?}"),
    }
}

#[tokio::test]
async fn test_ollama_validates_dimension() {
    let server = ollama_answering(embedding(&[0.1, 0.2, 0.3])).await;
    match embed_error(&server).await {
        ContextError::EmbeddingDimensionMismatch { expected, actual } => {
            assert_eq!(expected, 768);
            assert_eq!(actual, 3);
        }
        other => panic!("Expected EmbeddingDimensionMismatch error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_ollama_handles_auth_errors() {
    for (status, body) in [(401, "Unauthorized"), (403, "Forbidden")] {
        let server = ollama_answering(ResponseTemplate::new(status).set_body_string(body)).await;
        match embed_error(&server).await {
            ContextError::Auth(message) => {
                assert!(message.contains(&status.to_string()), "{message}");
            }
            other => panic!("Expected Auth error for {status}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn test_ollama_handles_rate_limit() {
    let server =
        ollama_answering(ResponseTemplate::new(429).set_body_string("Rate limit exceeded")).await;
    match embed_error(&server).await {
        ContextError::RateLimited { retry_after_secs } => assert_eq!(retry_after_secs, 60),
        other => panic!("Expected RateLimited error for 429, got {other:?}"),
    }
}

#[tokio::test]
async fn test_ollama_validates_url() {
    let message = |host: &str| match EmbeddingProviderFactory::create(&self_hosted(
        host.to_string(),
        None,
        None,
    )) {
        Err(ContextError::InvalidSourceConfig(message)) => message,
        Err(other) => panic!("Expected InvalidSourceConfig for {host}, got {other:?}"),
        Ok(_) => panic!("{host} should be refused"),
    };
    assert!(message("ftp://invalid.com").contains("ftp"));
    assert!(message("not-a-url").contains("not-a-url"));
}

#[tokio::test]
async fn an_invalid_url_scheme_is_refused_in_the_crates_wording() {
    let result =
        EmbeddingProviderFactory::create(&self_hosted("ftp://invalid.com".to_string(), None, None));
    match result {
        Err(ContextError::InvalidSourceConfig(message)) => assert_eq!(
            message, "invalid base URL ftp://invalid.com: the ftp scheme is not http or https",
            "Zone said \"Invalid URL scheme 'ftp': only http and https are supported\""
        ),
        Err(other) => panic!("Expected InvalidSourceConfig, got {other:?}"),
        Ok(_) => panic!("ftp should be refused"),
    }
}

#[tokio::test]
async fn an_error_body_is_kept_whole_with_the_key_concealed() {
    let body = format!("Error: {} rejected {KEY}", "a".repeat(LONG_BODY_CHARACTERS));
    let server = ollama_answering(ResponseTemplate::new(500).set_body_string(body)).await;
    let error = adapter(
        OllamaEmbeddingConfig::new(server.uri(), MODEL)
            .with_dimensions(768)
            .with_api_key(KEY),
    )
    .embed("test text")
    .await
    .unwrap_err();
    let ContextError::Embedding(message) = error else {
        panic!("Expected Embedding error, got {error:?}");
    };
    assert!(
        message.contains(&"a".repeat(LONG_BODY_CHARACTERS)),
        "Zone cut the body at 200 characters; the crate keeps it whole: {message}"
    );
    assert!(!message.contains("truncated"), "{message}");
    assert!(!message.contains(KEY), "the key leaked: {message}");
}

#[tokio::test]
async fn an_empty_key_sends_no_bearer() {
    let server = ollama_answering(embedding(&[0.5; 768])).await;
    let provider =
        EmbeddingProviderFactory::create(&self_hosted(server.uri(), None, Some(""))).unwrap();
    provider.embed("test").await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].headers.get("authorization").is_none(),
        "Zone sent \"Bearer \" for an empty key; the crate sends no header"
    );
}

#[tokio::test]
async fn a_redirect_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OLLAMA_PATH))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/elsewhere"))
        .mount(&server)
        .await;
    Mock::given(path("/elsewhere"))
        .respond_with(embedding(&[0.5; 768]))
        .expect(0)
        .mount(&server)
        .await;
    match embed_error(&server).await {
        ContextError::Embedding(message) => assert!(message.contains("302"), "{message}"),
        other => panic!("Expected Embedding error for a redirect, got {other:?}"),
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_trailing_slash_base_posts_to_api_embeddings() {
    let server = ollama_answering(embedding(&[0.5; 768])).await;
    let provider =
        EmbeddingProviderFactory::create(&self_hosted(format!("{}/", server.uri()), None, None))
            .unwrap();
    provider.embed("test").await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].url.path(),
        OLLAMA_PATH,
        "Zone posted to //api/embeddings"
    );
}

#[tokio::test]
async fn an_empty_vector_is_an_embedding_failure() {
    let server = ollama_answering(embedding(&[])).await;
    match embed_error(&server).await {
        ContextError::Embedding(message) => assert!(
            message.contains("empty embedding"),
            "Zone reported a dimension mismatch against 0: {message}"
        ),
        other => panic!("Expected Embedding error, got {other:?}"),
    }
}

#[test]
fn snowflake_size_tags_get_their_own_width_on_ollama() {
    for (model, dimension) in [
        ("snowflake-arctic-embed", 1024),
        ("snowflake-arctic-embed:22m", 384),
        ("snowflake-arctic-embed:33m", 384),
        ("snowflake-arctic-embed:110m", 768),
        ("snowflake-arctic-embed:137m", 768),
        ("snowflake-arctic-embed:335m", 1024),
    ] {
        let provider = adapter(OllamaEmbeddingConfig::new("http://localhost:11434", model));
        assert_eq!(
            provider.dimension(),
            dimension,
            "{model}: Zone said 1024 for every tag"
        );
        assert_eq!(provider.max_tokens(), 512, "{model}");
    }
}

struct OpenAiCompatibleResponder {
    width: usize,
    context_limit: Option<usize>,
}

impl Respond for OpenAiCompatibleResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON body");
        let inputs: Vec<String> = body["input"]
            .as_array()
            .expect("input array")
            .iter()
            .map(|input| input.as_str().expect("string input").to_string())
            .collect();
        if self
            .context_limit
            .is_some_and(|limit| inputs.iter().any(|input| input.chars().count() > limit))
        {
            return ResponseTemplate::new(400).set_body_string(OPENAI_CONTEXT_LENGTH);
        }
        let data: Vec<Value> = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                json!({
                    "object": "embedding",
                    "index": index,
                    "embedding": vec![input.chars().count() as f32; self.width],
                })
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({ "object": "list", "data": data }))
    }
}

async fn openai_compatible(
    responder: OpenAiCompatibleResponder,
) -> (MockServer, Arc<dyn EmbeddingService>) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OPENAI_COMPATIBLE_PATH))
        .respond_with(responder)
        .mount(&server)
        .await;
    let provider = EmbeddingProviderFactory::create(&self_hosted(
        server.uri(),
        Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE),
        Some(KEY),
    ))
    .unwrap();
    (server, provider)
}

#[tokio::test]
async fn the_openai_compatible_engine_posts_the_batch_to_v1_embeddings() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OPENAI_COMPATIBLE_PATH))
        .and(header("Authorization", format!("Bearer {KEY}").as_str()))
        .and(body_json(
            json!({ "model": MODEL, "input": ["alpha", "bravo charlie"] }),
        ))
        .respond_with(OpenAiCompatibleResponder {
            width: 768,
            context_limit: None,
        })
        .expect(1)
        .mount(&server)
        .await;
    let provider = EmbeddingProviderFactory::create(&self_hosted(
        format!("{}/", server.uri()),
        Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE),
        Some(KEY),
    ))
    .unwrap();
    let vectors = provider
        .embed_batch(&["alpha", "bravo charlie"])
        .await
        .unwrap();
    assert_eq!(vectors, vec![vec![5.0; 768], vec![13.0; 768]]);
    assert_eq!(provider.dimension(), 768);
    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("dimensions").is_none(), "{body}");
}

#[tokio::test]
async fn a_host_ending_in_v1_posts_to_v1_embeddings_once() {
    for suffix in ["/v1", "/v1/"] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OPENAI_COMPATIBLE_PATH))
            .respond_with(OpenAiCompatibleResponder {
                width: 768,
                context_limit: None,
            })
            .expect(1)
            .mount(&server)
            .await;
        let provider = EmbeddingProviderFactory::create(&self_hosted(
            format!("{}{suffix}", server.uri()),
            Some(EMBEDDING_ENGINE_OPENAI_COMPATIBLE),
            None,
        ))
        .unwrap();
        provider.embed_batch(&["alpha"]).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests[0].url.path(),
            OPENAI_COMPATIBLE_PATH,
            "{suffix}: Zone posted to /v1/v1/embeddings"
        );
    }
}

#[tokio::test]
async fn an_openai_compatible_batch_past_the_context_falls_back_per_text() {
    let (server, provider) = openai_compatible(OpenAiCompatibleResponder {
        width: 768,
        context_limit: Some(CONTEXT_LIMIT_CHARACTERS),
    })
    .await;
    let long = "a".repeat(LONG_TEXT_CHARACTERS);
    let vectors = provider.embed_batch(&["short", &long]).await.unwrap();
    assert_eq!(vectors[0], vec![5.0; 768]);
    assert_eq!(vectors[1], vec![750.0; 768]);
    let inputs: Vec<Vec<usize>> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            body["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|input| input.as_str().unwrap().chars().count())
                .collect()
        })
        .collect();
    assert_eq!(inputs[0], [5, 3000]);
    let mut singles: Vec<usize> = inputs[1..].iter().flatten().copied().collect();
    singles.sort_unstable();
    assert_eq!(singles, [5, 750, 1500, 3000]);
}

#[tokio::test]
async fn a_large_openai_compatible_batch_is_posted_in_requests_of_64() {
    let (server, provider) = openai_compatible(OpenAiCompatibleResponder {
        width: 768,
        context_limit: None,
    })
    .await;
    let owned: Vec<String> = (1..=LARGE_BATCH).map(|length| "a".repeat(length)).collect();
    let texts: Vec<&str> = owned.iter().map(String::as_str).collect();
    let vectors = provider.embed_batch(&texts).await.unwrap();
    let expected: Vec<Vec<f32>> = (1..=LARGE_BATCH)
        .map(|length| vec![length as f32; 768])
        .collect();
    assert_eq!(vectors, expected);
    let mut sizes: Vec<usize> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            body["input"].as_array().unwrap().len()
        })
        .collect();
    sizes.sort_unstable();
    assert_eq!(sizes, [5, INPUTS_PER_REQUEST, INPUTS_PER_REQUEST]);
}

#[tokio::test]
async fn an_openai_compatible_vector_wider_than_the_store_is_refused() {
    let width = VECTOR_DIMENSION * 3 / 2;
    let (_server, provider) = openai_compatible(OpenAiCompatibleResponder {
        width,
        context_limit: None,
    })
    .await;
    match provider.embed_batch(&["alpha"]).await.unwrap_err() {
        ContextError::EmbeddingDimensionMismatch { expected, actual } => {
            assert_eq!(expected, VECTOR_DIMENSION);
            assert_eq!(actual, width);
        }
        other => panic!("Expected EmbeddingDimensionMismatch, got {other:?}"),
    }
}
