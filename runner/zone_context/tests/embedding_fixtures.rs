use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_context::embeddings::EmbeddingService;
use zone_context::embeddings::providers::{
    AiSettings, EmbeddingProviderFactory, PROVIDER_SELF_HOSTED,
};
use zone_context::error::{ContextError, Result};

type Provider = fn(&AiSettings) -> Result<Arc<dyn EmbeddingService>>;

const PROVIDER: Provider = EmbeddingProviderFactory::create;

const RECORD_ENVIRONMENT: &str = "ZONE_RECORD_EMBEDDING_FIXTURES";
const RECORD_ENABLED: &str = "1";
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: String,
    prompt_length: usize,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OllamaExchange {
    scenario: String,
    requests: Vec<RecordedRequest>,
    vectors: Vec<Vec<f32>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OllamaFailure {
    scenario: String,
    variant: String,
    display: String,
    is_context_length: bool,
    is_fatal_embedding: bool,
    prompt_lengths: Vec<usize>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
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
            ..single("trailing_slash", None, SHORT_TEXT.to_string())
        },
        single("empty_key", Some(EMPTY_API_KEY), SHORT_TEXT.to_string()),
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
            "server_error_long_body",
            ResponseTemplate::new(500).set_body_string("x".repeat(LONG_ERROR_BODY_CHARACTERS)),
        ),
        failing(
            "context_length_exhausted",
            ResponseTemplate::new(500).set_body_string(CONTEXT_LENGTH_MESSAGE),
        ),
        failing(
            "malformed_json",
            ResponseTemplate::new(200).set_body_string("{\"embedding\": ["),
        ),
        failing(
            "width_mismatch",
            ResponseTemplate::new(200)
                .set_body_json(json!({ "embedding": vec![0.0_f32; MISMATCHED_DIMENSION] })),
        ),
        failing(
            "empty_vector",
            ResponseTemplate::new(200).set_body_json(json!({ "embedding": [] })),
        ),
        FailureScenario {
            name: "connection_refused",
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

fn recording() -> bool {
    std::env::var(RECORD_ENVIRONMENT).is_ok_and(|value| value == RECORD_ENABLED)
}

fn record_fixture<T: Serialize>(file: &str, actual: &T) {
    let mut contents = serde_json::to_string_pretty(actual).expect("fixture serialises");
    contents.push('\n');
    fs::create_dir_all(FIXTURE_DIRECTORY).expect("fixture directory");
    fs::write(fixture_path(file), contents).expect("fixture written");
}

fn read_fixture<T: DeserializeOwned>(file: &str) -> T {
    let contents = fs::read_to_string(fixture_path(file)).unwrap_or_else(|error| {
        panic!("{file}: {error}; record it with {RECORD_ENVIRONMENT}={RECORD_ENABLED}")
    });
    serde_json::from_str(&contents).unwrap_or_else(|error| panic!("{file}: {error}"))
}

fn expected_fixture<T: Serialize + DeserializeOwned>(file: &str, actual: &T) -> T {
    if recording() {
        record_fixture(file, actual);
    }
    read_fixture(file)
}

async fn check_ollama_requests(provider: Provider) {
    let mut actual = Vec::new();
    for scenario in request_scenarios() {
        actual.push(exchange(provider, &scenario).await);
    }
    let expected: Vec<OllamaExchange> = expected_fixture(OLLAMA_REQUESTS_FIXTURE, &actual);
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
    let expected: Vec<OllamaFailure> = expected_fixture(OLLAMA_ERRORS_FIXTURE, &actual);
    assert_eq!(expected, actual);
}

fn check_ollama_models(provider: Provider) {
    let actual: Vec<OllamaModel> = OLLAMA_MODEL_NAMES
        .iter()
        .map(|name| ollama_model(provider, name))
        .collect();
    let expected: Vec<OllamaModel> = expected_fixture(OLLAMA_MODELS_FIXTURE, &actual);
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
    use super::{PROVIDER, Provider, expected_fixture};

    use serde::{Deserialize, Serialize};
    use zone_context::embeddings::providers::{
        AiSettings, EMBEDDING_ENGINE_LOCAL, PROVIDER_SELF_HOSTED,
    };

    const LOCAL_FIXTURE: &str = "local_nomic_embed_text.json";
    const DEFAULT_LOCAL_MODEL: &str = "nomic-embed-text";
    const DEFAULT_LOCAL_DIMENSION: usize = 768;
    const DEFAULT_LOCAL_MAXIMUM_TOKENS: usize = 2048;
    const VECTOR_TOLERANCE: f32 = 1e-5;
    const LONG_PARAGRAPH_SENTENCES: usize = 90;
    const MEDIUM_PARAGRAPH_SENTENCES: usize = 22;

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

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct LocalVectors {
        name: String,
        text: String,
        batch: Vec<f32>,
        single: Vec<f32>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct LocalFixture {
        model: String,
        dimension: usize,
        max_tokens: usize,
        texts: Vec<LocalVectors>,
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

    async fn check_local_vectors(provider: Provider) {
        let settings = AiSettings {
            provider: PROVIDER_SELF_HOSTED.to_string(),
            embedding_engine: Some(EMBEDDING_ENGINE_LOCAL.to_string()),
            ..Default::default()
        };
        let embedder = tokio::task::spawn_blocking(move || provider(&settings))
            .await
            .expect("load task")
            .expect("default local model loads");

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

        let expected: LocalFixture = expected_fixture(LOCAL_FIXTURE, &actual);
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

    #[tokio::test]
    #[ignore = "loads the nomic-embed-text weights, about 550 MB on a cold cache"]
    async fn local_nomic_embed_text_matches_fixture() {
        check_local_vectors(PROVIDER).await;
    }
}
