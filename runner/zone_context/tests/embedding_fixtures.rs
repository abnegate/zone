use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_context::embeddings::EmbeddingService;
use zone_context::embeddings::providers::{
    AiSettings, EmbeddingProviderFactory, PROVIDER_SELF_HOSTED,
};
use zone_context::error::{ContextError, Result};

type Embedder = Arc<dyn EmbeddingService>;

type Provider = fn(&AiSettings) -> Result<Embedder>;

const PROVIDER: Provider = EmbeddingProviderFactory::create;

const FIXTURE_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/embeddings");

const OLLAMA_REQUESTS_FIXTURE: &str = "ollama_requests.json";
const OLLAMA_ERRORS_FIXTURE: &str = "ollama_errors.json";
const OLLAMA_MODELS_FIXTURE: &str = "ollama_models.json";

const OLLAMA_HOST: &str = "http://localhost:11434";
const OLLAMA_MODEL: &str = "nomic-embed-text";
const OLLAMA_DIMENSION: usize = 768;
const API_KEY: &str = "fixture-key";
const EMPTY_API_KEY: &str = "";
const SERVER_PLACEHOLDER: &str = "{server}";
const OLLAMA_PATH: &str = "/api/embeddings";
const DOUBLED_SLASH_PATH: &str = "//api/embeddings";
const EMPTY_BEARER: &str = "Bearer";
const SERVER_ERROR_PREFIX: &str =
    "Embedding generation failed: API returned error 500 Internal Server Error: ";
const TRUNCATION_MARKER: &str = "... (truncated)";
const ZONE_BODY_LIMIT: usize = 200;
const MALFORMED_JSON_DISPLAY: &str = "Embedding generation failed: Failed to parse response: EOF while parsing a list at line 1 column 15";
const CONNECTION_REFUSED_DISPLAY: &str =
    "Embedding generation failed: Request failed: error sending request";
const EMBEDDING_VARIANT: &str = "Embedding";
const DIMENSION_MISMATCH_VARIANT: &str = "EmbeddingDimensionMismatch";
const TRAILING_SLASH_SCENARIO: &str = "trailing_slash";
const EMPTY_KEY_SCENARIO: &str = "empty_key";
const LONG_BODY_SCENARIO: &str = "server_error_long_body";
const EMPTY_VECTOR_SCENARIO: &str = "empty_vector";
const MALFORMED_JSON_SCENARIO: &str = "malformed_json";
const CONNECTION_REFUSED_SCENARIO: &str = "connection_refused";
const SNOWFLAKE_LARGE_DIMENSION: usize = 1024;
const SNOWFLAKE_SIZE_DIMENSIONS: [(&str, usize); 4] = [
    ("snowflake-arctic-embed:22m", 384),
    ("snowflake-arctic-embed:33m", 384),
    ("snowflake-arctic-embed:110m", 768),
    ("snowflake-arctic-embed:137m", 768),
];
const LOOPBACK_ADDRESS: &str = "127.0.0.1:0";

const SHORT_TEXT: &str = "The quick brown fox";
const FAILURE_TEXT: &str = "hello";
const VECTOR_SCALE: f32 = 1000.0;
const CONTEXT_LIMIT_CHARACTERS: usize = 1000;
const CONTEXT_LENGTH_MESSAGE: &str = "the input length exceeds the context length";
const LONG_TEXT_CHARACTERS: usize = 5000;
const LONG_ERROR_BODY_CHARACTERS: usize = 300;
const MISMATCHED_DIMENSION: usize = 512;
const ALPHABET_LENGTH: usize = 26;

const OLLAMA_MODEL_NAMES: [&str; 14] = [
    "qwen3-embedding:0.6b",
    "qwen3-embedding",
    "nomic-embed-text",
    "nomic-embed-text:latest",
    "mxbai-embed-large",
    "snowflake-arctic-embed",
    "snowflake-arctic-embed:22m",
    "snowflake-arctic-embed:33m",
    "snowflake-arctic-embed:110m",
    "snowflake-arctic-embed:137m",
    "snowflake-arctic-embed:335m",
    "bge-small-en-v1.5",
    "all-minilm",
    "unknown-model",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: String,
    prompt_length: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct OllamaExchange {
    scenario: String,
    requests: Vec<RecordedRequest>,
    vectors: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct OllamaFailure {
    scenario: String,
    variant: String,
    display: String,
    is_context_length: bool,
    is_fatal_embedding: bool,
    prompt_lengths: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct OllamaModel {
    name: String,
    model: String,
    dimension: usize,
    max_tokens: usize,
}

enum Call {
    Single(String),
    Batch(Vec<String>),
}

struct RequestScenario {
    name: &'static str,
    trailing_slash: bool,
    key: Option<&'static str>,
    context_limit: Option<usize>,
    call: Call,
}

struct FailureScenario {
    name: &'static str,
    response: Option<ResponseTemplate>,
}

struct EmbeddingResponder {
    context_limit: Option<usize>,
}

impl Respond for EmbeddingResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let length = prompt_length(&request.body);
        if self.context_limit.is_some_and(|limit| length > limit) {
            return ResponseTemplate::new(500).set_body_string(CONTEXT_LENGTH_MESSAGE);
        }
        ResponseTemplate::new(200).set_body_json(json!({ "embedding": vector_for(length) }))
    }
}

fn vector_for(prompt_length: usize) -> Vec<f32> {
    (0..OLLAMA_DIMENSION)
        .map(|index| (index + prompt_length) as f32 / VECTOR_SCALE)
        .collect()
}

fn prompt_length(body: &[u8]) -> usize {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value["prompt"]
                .as_str()
                .map(|prompt| prompt.chars().count())
        })
        .unwrap_or_default()
}

fn long_text() -> String {
    (0..LONG_TEXT_CHARACTERS)
        .map(|index| char::from(b'a' + (index % ALPHABET_LENGTH) as u8))
        .collect()
}

fn ollama_settings(host: String, key: Option<&str>, model: &str) -> AiSettings {
    AiSettings {
        provider: PROVIDER_SELF_HOSTED.to_string(),
        litellm_host: Some(host),
        litellm_key: key.map(str::to_string),
        model_embedding: Some(model.to_string()),
        ..Default::default()
    }
}

fn single(name: &'static str, key: Option<&'static str>, text: String) -> RequestScenario {
    RequestScenario {
        name,
        trailing_slash: false,
        key,
        context_limit: None,
        call: Call::Single(text),
    }
}

fn request_scenarios() -> Vec<RequestScenario> {
    vec![
        single("single_text", None, SHORT_TEXT.to_string()),
        single("with_key", Some(API_KEY), SHORT_TEXT.to_string()),
        RequestScenario {
            call: Call::Batch(vec![
                "alpha".to_string(),
                "bravo charlie".to_string(),
                "delta echo foxtrot golf".to_string(),
            ]),
            ..single("batch", None, String::new())
        },
        RequestScenario {
            call: Call::Batch(Vec::new()),
            ..single("empty_batch", None, String::new())
        },
        single("long_text_truncated", None, long_text()),
        RequestScenario {
            context_limit: Some(CONTEXT_LIMIT_CHARACTERS),
            ..single("context_length_halving", None, long_text())
        },
        RequestScenario {
            trailing_slash: true,
            ..single(TRAILING_SLASH_SCENARIO, None, SHORT_TEXT.to_string())
        },
        single(
            EMPTY_KEY_SCENARIO,
            Some(EMPTY_API_KEY),
            SHORT_TEXT.to_string(),
        ),
    ]
}

fn failing(name: &'static str, response: ResponseTemplate) -> FailureScenario {
    FailureScenario {
        name,
        response: Some(response),
    }
}

fn failure_scenarios() -> Vec<FailureScenario> {
    vec![
        failing(
            "unauthorized",
            ResponseTemplate::new(401).set_body_string("invalid api key"),
        ),
        failing(
            "forbidden",
            ResponseTemplate::new(403).set_body_string("forbidden"),
        ),
        failing(
            "rate_limited",
            ResponseTemplate::new(429).set_body_string("slow down"),
        ),
        failing(
            LONG_BODY_SCENARIO,
            ResponseTemplate::new(500).set_body_string("x".repeat(LONG_ERROR_BODY_CHARACTERS)),
        ),
        failing(
            "context_length_exhausted",
            ResponseTemplate::new(500).set_body_string(CONTEXT_LENGTH_MESSAGE),
        ),
        failing(
            MALFORMED_JSON_SCENARIO,
            ResponseTemplate::new(200).set_body_string("{\"embedding\": ["),
        ),
        failing(
            "width_mismatch",
            ResponseTemplate::new(200)
                .set_body_json(json!({ "embedding": vec![0.0_f32; MISMATCHED_DIMENSION] })),
        ),
        failing(
            EMPTY_VECTOR_SCENARIO,
            ResponseTemplate::new(200).set_body_json(json!({ "embedding": [] })),
        ),
        FailureScenario {
            name: CONNECTION_REFUSED_SCENARIO,
            response: None,
        },
    ]
}

fn closed_host() -> String {
    let listener = TcpListener::bind(LOOPBACK_ADDRESS).expect("loopback bind");
    let address = listener.local_addr().expect("loopback address");
    format!("http://{address}")
}

async fn recorded_requests(server: &MockServer) -> Vec<RecordedRequest> {
    server
        .received_requests()
        .await
        .expect("request recording is enabled")
        .into_iter()
        .map(|request| RecordedRequest {
            path: request.url.path().to_string(),
            authorization: request
                .headers
                .get("authorization")
                .map(|value| value.to_str().expect("ASCII header").to_string()),
            body: String::from_utf8(request.body.clone()).expect("UTF-8 body"),
            prompt_length: prompt_length(&request.body),
        })
        .collect()
}

async fn exchange(provider: Provider, scenario: &RequestScenario) -> OllamaExchange {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(EmbeddingResponder {
            context_limit: scenario.context_limit,
        })
        .mount(&server)
        .await;
    let host = if scenario.trailing_slash {
        format!("{}/", server.uri())
    } else {
        server.uri()
    };
    let embedder = provider(&ollama_settings(host, scenario.key, OLLAMA_MODEL))
        .unwrap_or_else(|error| panic!("{}: provider: {error}", scenario.name));
    let vectors = match &scenario.call {
        Call::Single(text) => vec![
            embedder
                .embed(text)
                .await
                .unwrap_or_else(|error| panic!("{}: embed: {error}", scenario.name)),
        ],
        Call::Batch(texts) => {
            let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
            embedder
                .embed_batch(&texts)
                .await
                .unwrap_or_else(|error| panic!("{}: embed_batch: {error}", scenario.name))
        }
    };
    let mut requests = recorded_requests(&server).await;
    if matches!(scenario.call, Call::Batch(_)) {
        requests.sort();
    }
    OllamaExchange {
        scenario: scenario.name.to_string(),
        requests,
        vectors,
    }
}

fn variant_name(error: &ContextError) -> String {
    format!("{error:?}")
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect()
}

async fn failure(provider: Provider, scenario: FailureScenario) -> OllamaFailure {
    let server = match scenario.response {
        Some(response) => {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(response)
                .mount(&server)
                .await;
            Some(server)
        }
        None => None,
    };
    let host = server.as_ref().map_or_else(closed_host, MockServer::uri);
    let embedder = provider(&ollama_settings(host.clone(), None, OLLAMA_MODEL))
        .unwrap_or_else(|error| panic!("{}: provider: {error}", scenario.name));
    let error = embedder.embed(FAILURE_TEXT).await.expect_err(scenario.name);
    let prompt_lengths = match &server {
        Some(server) => recorded_requests(server)
            .await
            .iter()
            .map(|request| request.prompt_length)
            .collect(),
        None => Vec::new(),
    };
    OllamaFailure {
        scenario: scenario.name.to_string(),
        variant: variant_name(&error),
        display: error.to_string().replace(&host, SERVER_PLACEHOLDER),
        is_context_length: error.is_context_length(),
        is_fatal_embedding: error.is_fatal_embedding(),
        prompt_lengths,
    }
}

fn ollama_model(provider: Provider, name: &str) -> OllamaModel {
    let embedder = provider(&ollama_settings(OLLAMA_HOST.to_string(), None, name))
        .unwrap_or_else(|error| panic!("{name}: provider: {error}"));
    OllamaModel {
        name: name.to_string(),
        model: embedder.model().to_string(),
        dimension: embedder.dimension(),
        max_tokens: embedder.max_tokens(),
    }
}

fn fixture_path(file: &str) -> PathBuf {
    PathBuf::from(FIXTURE_DIRECTORY).join(file)
}

fn read_fixture<T: DeserializeOwned>(file: &str) -> T {
    let contents =
        fs::read_to_string(fixture_path(file)).unwrap_or_else(|error| panic!("{file}: {error}"));
    serde_json::from_str(&contents).unwrap_or_else(|error| panic!("{file}: {error}"))
}

fn scenario<'a, T>(fixtures: &'a mut [T], name: &str, scenario: impl Fn(&T) -> &str) -> &'a mut T {
    fixtures
        .iter_mut()
        .find(|fixture| scenario(fixture) == name)
        .unwrap_or_else(|| panic!("{name} is recorded"))
}

/// The recorded exchanges with the crate's request departures applied, each
/// asserting what Zone sent before replacing it.
fn departed_exchanges(mut exchanges: Vec<OllamaExchange>) -> Vec<OllamaExchange> {
    let trailing_slash = scenario(&mut exchanges, TRAILING_SLASH_SCENARIO, |exchange| {
        &exchange.scenario
    });
    assert_eq!(trailing_slash.requests[0].path, DOUBLED_SLASH_PATH);
    trailing_slash.requests[0].path = OLLAMA_PATH.to_string();

    let empty_key = scenario(&mut exchanges, EMPTY_KEY_SCENARIO, |exchange| {
        &exchange.scenario
    });
    assert_eq!(
        empty_key.requests[0].authorization.as_deref(),
        Some(EMPTY_BEARER)
    );
    empty_key.requests[0].authorization = None;

    exchanges
}

/// The recorded failures with the crate's wording departures applied, each
/// asserting what Zone reported before replacing it.
fn departed_failures(mut failures: Vec<OllamaFailure>) -> Vec<OllamaFailure> {
    let long_body = scenario(&mut failures, LONG_BODY_SCENARIO, |failure| {
        &failure.scenario
    });
    let whole = format!(
        "{SERVER_ERROR_PREFIX}{}",
        "x".repeat(LONG_ERROR_BODY_CHARACTERS)
    );
    assert_eq!(
        long_body.display,
        format!(
            "{}{TRUNCATION_MARKER}",
            &whole[..SERVER_ERROR_PREFIX.len() + ZONE_BODY_LIMIT]
        )
    );
    long_body.display = whole;

    let empty_vector = scenario(&mut failures, EMPTY_VECTOR_SCENARIO, |failure| {
        &failure.scenario
    });
    assert_eq!(empty_vector.variant, DIMENSION_MISMATCH_VARIANT);
    assert_eq!(
        empty_vector.display,
        format!("Unsupported embedding dimension: expected {OLLAMA_DIMENSION}, got 0")
    );
    empty_vector.variant = EMBEDDING_VARIANT.to_string();
    empty_vector.display = format!(
        "Embedding generation failed: Failed to parse response: Ollama sent an empty embedding, so {OLLAMA_MODEL} may not be an embedding model"
    );

    let malformed = scenario(&mut failures, MALFORMED_JSON_SCENARIO, |failure| {
        &failure.scenario
    });
    assert_eq!(
        malformed.display,
        format!(
            "Embedding generation failed: Failed to parse response: error decoding response body for url ({SERVER_PLACEHOLDER}{OLLAMA_PATH})"
        )
    );
    malformed.display = MALFORMED_JSON_DISPLAY.to_string();

    let refused = scenario(&mut failures, CONNECTION_REFUSED_SCENARIO, |failure| {
        &failure.scenario
    });
    assert_eq!(
        refused.display,
        format!(
            "Embedding generation failed: Request failed: error sending request for url ({SERVER_PLACEHOLDER}{OLLAMA_PATH})"
        )
    );
    refused.display = CONNECTION_REFUSED_DISPLAY.to_string();

    failures
}

/// The recorded model table with the crate's Snowflake size widths applied,
/// asserting Zone's 1024 for each tag before replacing it.
fn departed_models(mut models: Vec<OllamaModel>) -> Vec<OllamaModel> {
    for (name, dimension) in SNOWFLAKE_SIZE_DIMENSIONS {
        let model = scenario(&mut models, name, |model| &model.name);
        assert_eq!(model.dimension, SNOWFLAKE_LARGE_DIMENSION, "{name}");
        model.dimension = dimension;
    }
    models
}

async fn check_ollama_requests(provider: Provider) {
    let mut actual = Vec::new();
    for scenario in request_scenarios() {
        actual.push(exchange(provider, &scenario).await);
    }
    let expected = departed_exchanges(read_fixture(OLLAMA_REQUESTS_FIXTURE));
    assert_eq!(expected.len(), actual.len(), "scenario count");
    for (expected, actual) in expected.iter().zip(&actual) {
        assert_eq!(expected, actual, "{}", expected.scenario);
    }
}

async fn check_ollama_errors(provider: Provider) {
    let mut actual = Vec::new();
    for scenario in failure_scenarios() {
        actual.push(failure(provider, scenario).await);
    }
    let expected = departed_failures(read_fixture(OLLAMA_ERRORS_FIXTURE));
    assert_eq!(expected.len(), actual.len(), "scenario count");
    for (expected, actual) in expected.iter().zip(&actual) {
        assert_eq!(expected, actual, "{}", expected.scenario);
    }
}

fn check_ollama_models(provider: Provider) {
    let actual: Vec<OllamaModel> = OLLAMA_MODEL_NAMES
        .iter()
        .map(|name| ollama_model(provider, name))
        .collect();
    let expected = departed_models(read_fixture(OLLAMA_MODELS_FIXTURE));
    assert_eq!(expected, actual);
}

#[tokio::test]
async fn ollama_requests_match_fixture() {
    check_ollama_requests(PROVIDER).await;
}

#[tokio::test]
async fn ollama_errors_match_fixture() {
    check_ollama_errors(PROVIDER).await;
}

#[test]
fn ollama_models_match_fixture() {
    check_ollama_models(PROVIDER);
}

#[cfg(feature = "local-embeddings")]
mod local {
    use abnegate_llm::EmbeddingProvider;
    use abnegate_llm::modality::vendor::{FastembedConfig, FastembedProvider};
    use fastembed::{EmbeddingModel, TextEmbedding};
    use serde::Deserialize;
    use zone_context::embeddings::cache;
    use zone_context::embeddings::providers::{
        AiSettings, EMBEDDING_ENGINE_LOCAL, PROVIDER_SELF_HOSTED,
    };

    use super::{PROVIDER, Provider, read_fixture};

    const LOCAL_FIXTURE: &str = "local_nomic_embed_text.json";
    const LOCAL_MODELS_FIXTURE: &str = "local_models.json";
    const DEFAULT_LOCAL_MODEL: &str = "nomic-embed-text";
    const DEFAULT_LOCAL_DIMENSION: usize = 768;
    const DEFAULT_LOCAL_MAXIMUM_TOKENS: usize = 2048;
    const VECTOR_TOLERANCE: f32 = 1e-5;
    const LONG_PARAGRAPH_SENTENCES: usize = 90;
    const MEDIUM_PARAGRAPH_SENTENCES: usize = 22;
    const SNOWFLAKE_LARGE_MODEL: &str = "SnowflakeArcticEmbedL";
    const PROBE_TEXT: &str = "hello";
    const SNOWFLAKE_SIZE_MODELS: [(&str, &str); 4] = [
        ("snowflake-arctic-embed:22m", "SnowflakeArcticEmbedXS"),
        ("snowflake-arctic-embed:33m", "SnowflakeArcticEmbedS"),
        ("snowflake-arctic-embed:110m", "SnowflakeArcticEmbedM"),
        ("snowflake-arctic-embed:137m", "SnowflakeArcticEmbedMLong"),
    ];

    const SUBJECTS: [&str; 7] = [
        "The indexer",
        "A background worker",
        "The scheduler",
        "Every agent",
        "The retrieval service",
        "A reviewer",
        "The runner",
    ];
    const ACTIONS: [&str; 5] = ["rewrites", "inspects", "compresses", "validates", "streams"];
    const OBJECTS: [&str; 6] = [
        "the cached chunk index",
        "a pull request summary",
        "the embedding queue",
        "each repository manifest",
        "the nightly test report",
        "a vector search result",
    ];

    #[derive(Debug, PartialEq, Deserialize)]
    struct LocalVectors {
        name: String,
        text: String,
        batch: Vec<f32>,
        single: Vec<f32>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct LocalFixture {
        model: String,
        dimension: usize,
        max_tokens: usize,
        texts: Vec<LocalVectors>,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    struct ResolvedModel {
        name: String,
        model: Option<String>,
        dimension: Option<usize>,
        max_tokens: Option<usize>,
        error: Option<String>,
    }

    fn local_settings(model: Option<&str>) -> AiSettings {
        AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            embedding_engine: Some(EMBEDDING_ENGINE_LOCAL.to_string()),
            model_embedding: model.map(str::to_string),
            ..Default::default()
        }
    }

    fn paragraph(sentences: usize) -> String {
        (0..sentences)
            .map(|index| {
                format!(
                    "{} {} {} during step {}.",
                    SUBJECTS[index % SUBJECTS.len()],
                    ACTIONS[index % ACTIONS.len()],
                    OBJECTS[index % OBJECTS.len()],
                    index
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn texts() -> Vec<(&'static str, String)> {
        vec![
            (
                "short_sentence",
                "Zone indexes a repository so agents can retrieve the code they need."
                    .to_string(),
            ),
            (
                "rust_snippet",
                "fn main() {\n    let values: Vec<u32> = (1..=10).collect();\n    println!(\"{}\", values.iter().sum::<u32>());\n}"
                    .to_string(),
            ),
            (
                "cjk_and_emoji",
                "向量检索让代理找到相关的代码。日本語のテキストも含みます。🚀🔍✨".to_string(),
            ),
            (
                "whitespace_heavy",
                "  \t indented\t\twith   tabs \n\n and    trailing   spaces   \t ".to_string(),
            ),
            ("long_paragraph", paragraph(LONG_PARAGRAPH_SENTENCES)),
            ("medium_paragraph", paragraph(MEDIUM_PARAGRAPH_SENTENCES)),
        ]
    }

    fn assert_close(expected: &[f32], actual: &[f32], context: &str) {
        assert_eq!(expected.len(), actual.len(), "{context}: width");
        for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
            assert!(
                (expected - actual).abs() <= VECTOR_TOLERANCE,
                "{context}[{index}]: expected {expected}, got {actual}"
            );
        }
    }

    /// The recorded resolutions with the crate's Snowflake size models
    /// applied, asserting Zone's large model for each tag before replacing
    /// it.
    fn departed_resolutions(mut resolutions: Vec<ResolvedModel>) -> Vec<ResolvedModel> {
        for (name, model) in SNOWFLAKE_SIZE_MODELS {
            let resolution = resolutions
                .iter_mut()
                .find(|resolution| resolution.name == name)
                .unwrap_or_else(|| panic!("{name} is recorded"));
            assert_eq!(
                resolution.model.as_deref(),
                Some(SNOWFLAKE_LARGE_MODEL),
                "{name}"
            );
            let variant: EmbeddingModel = model.parse().expect("a fastembed model");
            resolution.model = Some(model.to_string());
            resolution.dimension = Some(TextEmbedding::get_model_info(&variant).unwrap().dim);
        }
        resolutions
    }

    fn check_local_model_resolutions(provider: Provider) {
        let expected = departed_resolutions(read_fixture(LOCAL_MODELS_FIXTURE));
        for resolution in expected {
            let name = resolution.name.as_str();
            let config = FastembedConfig::new().with_model(name);
            match (&resolution.error, config.model_maximum_input_tokens()) {
                (Some(error), Err(_)) => {
                    let refused = provider(&local_settings(Some(name)))
                        .err()
                        .unwrap_or_else(|| panic!("{name} should be refused"));
                    assert_eq!(&refused.to_string(), error, "{name}");
                }
                (None, Ok(maximum_input_tokens)) => {
                    assert_eq!(
                        resolution.max_tokens,
                        Some(maximum_input_tokens as usize),
                        "{name}"
                    );
                    let model = resolution.model.as_deref().expect("a resolved model");
                    assert_eq!(
                        FastembedConfig::new()
                            .with_model(model)
                            .model_maximum_input_tokens()
                            .ok(),
                        Some(maximum_input_tokens),
                        "{name}: the crate reads {model} at another length"
                    );
                    let variant: EmbeddingModel = model.parse().expect("a fastembed model");
                    assert_eq!(
                        resolution.dimension,
                        Some(TextEmbedding::get_model_info(&variant).unwrap().dim),
                        "{name}"
                    );
                }
                (error, resolved) => panic!("{name}: recorded {error:?}, resolved {resolved:?}"),
            }
        }
    }

    async fn load(provider: Provider, model: Option<&'static str>) -> super::Embedder {
        tokio::task::spawn_blocking(move || provider(&local_settings(model)))
            .await
            .expect("load task")
            .unwrap_or_else(|error| panic!("{model:?} loads: {error}"))
    }

    async fn load_directly(name: &'static str) -> FastembedProvider {
        tokio::task::spawn_blocking(move || {
            FastembedProvider::new(
                FastembedConfig::new()
                    .with_model(name)
                    .with_cache_directory(cache::default_directory()),
            )
        })
        .await
        .expect("load task")
        .unwrap_or_else(|error| panic!("{name} loads: {error}"))
    }

    fn loadable_names(resolutions: &[ResolvedModel]) -> Vec<&str> {
        resolutions
            .iter()
            .filter(|resolution| resolution.error.is_none())
            .map(|resolution| resolution.name.as_str())
            .collect()
    }

    /// Loads `name` through the factory and through the crate directly, so
    /// a wrong name-to-model mapping in the crate fails here rather than
    /// changing the width of stored vectors.
    async fn check_loaded_model(provider: Provider, name: &'static str) {
        let expected = departed_resolutions(read_fixture(LOCAL_MODELS_FIXTURE))
            .into_iter()
            .find(|resolution| resolution.name == name)
            .unwrap_or_else(|| panic!("{name} is recorded"));
        let embedder = load(provider, Some(name)).await;
        assert_eq!(embedder.model(), name);
        assert_eq!(
            Some(embedder.dimension()),
            expected.dimension,
            "{name}: width"
        );
        assert_eq!(
            Some(embedder.max_tokens()),
            expected.max_tokens,
            "{name}: maximum tokens"
        );
        let vector = embedder.embed(PROBE_TEXT).await.expect("embed");
        assert_eq!(
            Some(vector.len()),
            expected.dimension,
            "{name}: vector width"
        );
        drop(embedder);

        let loaded = load_directly(name).await;
        assert_eq!(
            Some(loaded.loaded_model()),
            expected.model.as_deref(),
            "{name}: loaded model"
        );
        assert_eq!(
            Some(loaded.dimensions() as usize),
            expected.dimension,
            "{name}: loaded width"
        );
    }

    async fn check_local_vectors(provider: Provider) {
        let embedder = load(provider, None).await;

        let texts = texts();
        let inputs: Vec<&str> = texts.iter().map(|(_, text)| text.as_str()).collect();
        let batch = embedder.embed_batch(&inputs).await.expect("embed_batch");
        let mut vectors = Vec::with_capacity(texts.len());
        for ((name, text), batch) in texts.iter().zip(batch) {
            let single = embedder.embed(text).await.expect("embed");
            vectors.push(LocalVectors {
                name: name.to_string(),
                text: text.clone(),
                batch,
                single,
            });
        }
        let actual = LocalFixture {
            model: embedder.model().to_string(),
            dimension: embedder.dimension(),
            max_tokens: embedder.max_tokens(),
            texts: vectors,
        };

        assert_eq!(actual.model, DEFAULT_LOCAL_MODEL);
        assert_eq!(actual.dimension, DEFAULT_LOCAL_DIMENSION);
        assert_eq!(actual.max_tokens, DEFAULT_LOCAL_MAXIMUM_TOKENS);

        let expected: LocalFixture = read_fixture(LOCAL_FIXTURE);
        assert_eq!(expected.model, actual.model);
        assert_eq!(expected.dimension, actual.dimension);
        assert_eq!(expected.max_tokens, actual.max_tokens);
        assert_eq!(expected.texts.len(), actual.texts.len(), "text count");
        for (expected, actual) in expected.texts.iter().zip(&actual.texts) {
            assert_eq!(expected.name, actual.name);
            assert_eq!(expected.text, actual.text, "{}", expected.name);
            assert_close(
                &expected.batch,
                &actual.batch,
                &format!("{} batch", expected.name),
            );
            assert_close(
                &expected.single,
                &actual.single,
                &format!("{} single", expected.name),
            );
        }
    }

    #[test]
    fn local_model_refusals_input_limits_and_recorded_widths_match_fixture() {
        check_local_model_resolutions(PROVIDER);
    }

    #[tokio::test]
    #[ignore = "loads the nomic-embed-text weights, about 550 MB on a cold cache"]
    async fn local_nomic_embed_text_matches_fixture() {
        check_local_vectors(PROVIDER).await;
    }

    #[test]
    fn every_loadable_model_has_a_load_test() {
        let resolutions: Vec<ResolvedModel> = read_fixture(LOCAL_MODELS_FIXTURE);
        assert_eq!(loadable_names(&resolutions), LOADED_MODEL_NAMES);
    }

    macro_rules! loaded_model_tests {
        ($($test:ident: $name:literal, $reason:literal;)+) => {
            const LOADED_MODEL_NAMES: &[&str] = &[$($name),+];

            $(
                #[tokio::test]
                #[ignore = $reason]
                async fn $test() {
                    check_loaded_model(PROVIDER, $name).await;
                }
            )+
        };
    }

    loaded_model_tests! {
        nomic_embed_text_loads_its_recorded_model:
            "nomic-embed-text",
            "loads the nomic-embed-text-v1.5 weights, about 550 MB on a cold cache";
        nomic_embed_text_latest_loads_its_recorded_model:
            "nomic-embed-text:latest",
            "loads the nomic-embed-text-v1.5 weights, about 550 MB on a cold cache";
        mxbai_embed_large_loads_its_recorded_model:
            "mxbai-embed-large",
            "loads the mxbai-embed-large-v1 weights, about 1.3 GB on a cold cache";
        snowflake_arctic_embed_loads_its_recorded_model:
            "snowflake-arctic-embed",
            "loads the snowflake-arctic-embed-l weights, about 1.3 GB on a cold cache";
        snowflake_arctic_embed_22m_loads_the_extra_small_model:
            "snowflake-arctic-embed:22m",
            "loads the snowflake-arctic-embed-xs weights, about 90 MB on a cold cache";
        snowflake_arctic_embed_33m_loads_the_small_model:
            "snowflake-arctic-embed:33m",
            "loads the snowflake-arctic-embed-s weights, about 130 MB on a cold cache";
        snowflake_arctic_embed_110m_loads_the_medium_model:
            "snowflake-arctic-embed:110m",
            "loads the snowflake-arctic-embed-m weights, about 440 MB on a cold cache";
        snowflake_arctic_embed_137m_loads_the_medium_long_model:
            "snowflake-arctic-embed:137m",
            "loads the snowflake-arctic-embed-m-long weights, about 550 MB on a cold cache";
        snowflake_arctic_embed_335m_loads_the_large_model:
            "snowflake-arctic-embed:335m",
            "loads the snowflake-arctic-embed-l weights, about 1.3 GB on a cold cache";
        bge_small_en_loads_its_recorded_model:
            "bge-small-en-v1.5",
            "loads the bge-small-en-v1.5 weights, about 130 MB on a cold cache";
        all_minilm_loads_its_recorded_model:
            "all-minilm",
            "loads the all-MiniLM-L6-v2 weights, about 90 MB on a cold cache";
        nomic_hugging_face_id_loads_its_recorded_model:
            "nomic-ai/nomic-embed-text-v1.5",
            "loads the nomic-embed-text-v1.5 weights, about 550 MB on a cold cache";
    }
}
