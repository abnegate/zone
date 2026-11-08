use abnegate_agent::context;
use abnegate_agent::context::ContextError;
use abnegate_agent::context::ContextSource;
use abnegate_agent::context::ContextStatus;
use abnegate_agent::context::Entry;
use abnegate_agent::context::Policy;
use abnegate_agent::context::Prepared;
use abnegate_llm::CompletionProvider;
use abnegate_llm::CompletionRequest;
use abnegate_llm::Effort;
use abnegate_llm::LlmConfig;
use abnegate_llm::Message;
use abnegate_llm::RequestOptions;
use abnegate_llm::finish_reason;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;

use crate::llm::LlmClient;

const MODEL: &str = "test";

const STOPS: [&str; 2] = ["}", "<|end|>"];

const RESERVED: u32 = 1024;

fn structured() -> String {
    json!({
        "objective": "Complete requested work",
        "constraints": ["Keep all corrections"],
        "corrections": [],
        "decisions": [],
        "completed": ["Read evidence"],
        "evidence": [],
        "failed": [],
        "pending": ["Check the result"],
        "questions": []
    })
    .to_string()
}

fn answer(content: &str, reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "completion",
        "object": "chat.completion",
        "created": 0,
        "model": MODEL,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": reason
        }]
    }))
}

/// Whether `request` asks the way a summary must: at temperature 0, with no
/// stop sequence and no reasoning.
fn summarizing(request: &Request) -> bool {
    request.body_json::<Value>().is_ok_and(|body| {
        body["temperature"] == 0.0
            && body.get("stop").is_none()
            && body.get("reasoning_effort").is_none()
    })
}

async fn answering() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(answer(&structured(), finish_reason::STOP))
        .mount(&server)
        .await;
    server
}

fn client(server: &MockServer, api_key: &str) -> LlmClient {
    LlmClient::new(LlmConfig::new(server.uri(), MODEL, api_key).with_maximum_tokens(RESERVED))
}

fn stopping(client: LlmClient) -> LlmClient {
    client.with_stop(STOPS.map(String::from).into())
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .expect("recorded requests")
        .iter()
        .map(|request| request.body_json().expect("a JSON body"))
        .collect()
}

async fn ask(provider: &dyn CompletionProvider, temperature: Option<f32>) {
    let messages = [Message::user("hello")];
    let request = CompletionRequest::new(MODEL, &messages, RequestOptions::new(64));
    let request = match temperature {
        Some(temperature) => request.with_temperature(temperature),
        None => request,
    };
    provider.complete(request).await.expect("a completion");
}

fn policy() -> Policy {
    Policy::new(Some(5_000), RESERVED, ContextSource::Configured)
}

fn overflowing() -> Vec<Entry> {
    vec![
        Entry::new("old", Message::user("x".repeat(40_000)))
            .with_preserve(false)
            .with_consumed(true),
        Entry::new("current", Message::user("Current"))
            .with_preserve(true)
            .with_consumed(false),
    ]
}

async fn compact(llm: &LlmClient) -> Result<Prepared, ContextError> {
    context::prepare(
        &llm.summarizer(),
        MODEL,
        &overflowing(),
        None,
        &policy(),
        None,
    )
    .await
}

async fn ordinary(llm: &LlmClient) {
    llm.chat_with_options(
        MODEL,
        &[Message::user("hello")],
        None,
        RequestOptions::new(RESERVED),
    )
    .await
    .expect("an ordinary completion");
}

#[tokio::test]
async fn a_request_temperature_reaches_the_wire() {
    let server = answering().await;
    let llm = stopping(client(&server, ""));

    ask(&llm, Some(0.25)).await;
    ask(&llm.summarizer(), Some(0.25)).await;

    let bodies = bodies(&server).await;
    let [ordinary, summary] = &bodies[..] else {
        panic!("two requests: {bodies:?}");
    };
    assert_eq!(ordinary["temperature"], 0.25);
    assert_eq!(ordinary["stop"], json!(STOPS));
    assert_eq!(summary["temperature"], 0.25);
    assert!(summary.get("stop").is_none(), "{summary}");
}

#[tokio::test]
async fn a_request_without_temperature_sends_the_clients_own() {
    let server = answering().await;
    let llm = LlmClient::new(
        LlmConfig::new(server.uri(), MODEL, "")
            .with_maximum_tokens(RESERVED)
            .with_temperature(0.7),
    );

    ask(&llm, None).await;
    ask(&llm.summarizer(), None).await;

    let bodies = bodies(&server).await;
    let [ordinary, summary] = &bodies[..] else {
        panic!("two requests: {bodies:?}");
    };
    assert_eq!(
        ordinary["temperature"].as_f64().map(|value| value as f32),
        Some(0.7)
    );
    assert_eq!(summary["temperature"], 0.0);
}

#[tokio::test]
async fn summaries_carry_no_stop() {
    let server = answering().await;
    let llm = stopping(client(&server, ""));

    let prepared = compact(&llm).await.expect("a compaction");
    ordinary(&llm).await;

    assert_eq!(prepared.usage.status, ContextStatus::Compacted);
    let bodies = bodies(&server).await;
    let (ordinary, summaries) = bodies.split_last().expect("requests");
    assert!(!summaries.is_empty());
    for summary in summaries {
        assert!(summary.get("stop").is_none(), "{summary}");
        assert_eq!(summary["temperature"], 0.0);
    }
    assert_eq!(ordinary["stop"], json!(STOPS));
}

#[tokio::test]
async fn summaries_carry_no_reasoning_while_ordinary_requests_keep_it() {
    let server = answering().await;
    let llm = client(&server, "").with_reasoning(MODEL, Effort::Medium);

    compact(&llm).await.expect("a compaction");
    ordinary(&llm).await;

    let bodies = bodies(&server).await;
    let (ordinary, summaries) = bodies.split_last().expect("requests");
    assert!(!summaries.is_empty());
    for summary in summaries {
        assert!(summary.get("reasoning_effort").is_none(), "{summary}");
    }
    assert_eq!(ordinary["reasoning_effort"], "medium");
}

#[tokio::test]
async fn a_summary_cut_off_by_its_token_limit_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(summarizing)
        .respond_with(answer(&structured(), finish_reason::LENGTH))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(answer(&structured(), finish_reason::STOP))
        .mount(&server)
        .await;
    let llm = stopping(client(&server, "")).with_reasoning(MODEL, Effort::Medium);

    let error = compact(&llm).await.unwrap_err();

    assert!(
        matches!(&error, ContextError::Summary(reason) if reason.contains("truncated")),
        "{error}"
    );
}

#[tokio::test]
async fn a_compaction_the_endpoint_refuses_never_reports_its_key() {
    const KEY: &str = "sk-proj-AbCdEfGh1234567890wxyz";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(summarizing)
        .respond_with(ResponseTemplate::new(401).set_body_string(format!(
            r#"{{"error":{{"message":"Incorrect API key provided: {KEY}, sk-proj-****wxyz, api_key=sk-proj-AbCd****"}}}}"#
        )))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(answer(&structured(), finish_reason::STOP))
        .mount(&server)
        .await;
    let llm = stopping(client(&server, KEY)).with_reasoning(MODEL, Effort::Medium);

    let error = compact(&llm)
        .await
        .expect_err("a refused compaction that no longer fits fails the turn")
        .to_string();

    assert!(error.contains("401"), "{error}");
    assert!(!error.contains(KEY), "{error}");
    assert!(!error.contains("wxyz"), "{error}");
    assert!(!error.contains("sk-proj-AbCd"), "{error}");
}
