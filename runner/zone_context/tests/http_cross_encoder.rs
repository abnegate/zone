//! The HTTP cross-encoder over abnegate-llm's rerank provider, against a
//! mocked rerank endpoint.

use std::net::TcpListener;

use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;
use zone_context::embeddings::rerank::CrossEncoder;
use zone_context::embeddings::rerank::HttpCrossEncoder;
use zone_context::error::ContextError;

const RERANK_PATH: &str = "/v1/rerank";
const REDIRECT_TARGET_PATH: &str = "/elsewhere";
const MODEL: &str = "zone-fixture-reranker";
const QUERY: &str = "What does should_skip_blob do?";
const DOCUMENTS: [&str; 2] = [
    "fn should_skip_blob(uri: &str, sha: &str) -> bool",
    "generic authentication helper with a login form",
];
const LOOPBACK: &str = "127.0.0.1:0";
const FOUND: u16 = 302;
const SERVICE_UNAVAILABLE: u16 = 503;
const REQUEST_PREFIX: &str = "rerank request: ";
const PARSE_PREFIX: &str = "rerank parse: ";
const CLIENT_PREFIX: &str = "rerank client: ";

fn scored(results: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "results": results }))
}

fn discriminating() -> ResponseTemplate {
    scored(json!([
        {"index": 0, "relevance_score": 0.9},
        {"index": 1, "relevance_score": 0.1},
    ]))
}

async fn endpoint(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RERANK_PATH))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

fn rerank_url(server: &MockServer) -> String {
    format!("{}{RERANK_PATH}", server.uri())
}

fn closed_url() -> String {
    let listener = TcpListener::bind(LOOPBACK).expect("bind a loopback port");
    let address = listener.local_addr().expect("read the bound address");
    drop(listener);
    format!("http://{address}/rerank")
}

fn encoder(url: &str) -> HttpCrossEncoder {
    match HttpCrossEncoder::new(url, MODEL) {
        Ok(encoder) => encoder,
        Err(error) => panic!("{url} should build an encoder: {error}"),
    }
}

async fn scores(server: &MockServer) -> Result<Vec<f32>, ContextError> {
    encoder(&rerank_url(server))
        .score_documents(QUERY, &DOCUMENTS)
        .await
}

fn embedding_failure(result: Result<Vec<f32>, ContextError>) -> String {
    match result {
        Err(ContextError::Embedding(message)) => message,
        other => panic!("expected an embedding failure, got {other:?}"),
    }
}

#[tokio::test]
async fn an_answer_that_scores_no_document_is_an_error() {
    for results in [
        json!([{"index": 5, "relevance_score": 0.9}]),
        json!([{"index": 0}]),
    ] {
        let server = endpoint(scored(results.clone())).await;

        let message = embedding_failure(scores(&server).await);

        assert_eq!(message, "rerank response scored no document", "{results}");
    }
}

#[tokio::test]
async fn a_later_hit_without_a_score_keeps_the_earlier_score() {
    let server = endpoint(scored(json!([
        {"index": 0, "relevance_score": 0.9},
        {"index": 0},
        {"index": 1, "score": 0.2},
    ])))
    .await;

    let scores = scores(&server).await.expect("scores");

    assert_eq!(scores, vec![0.9, 0.2]);
}

#[tokio::test]
async fn redirects_are_refused() {
    let server = MockServer::start().await;
    Mock::given(path(REDIRECT_TARGET_PATH))
        .respond_with(discriminating())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(RERANK_PATH))
        .respond_with(ResponseTemplate::new(FOUND).insert_header(
            "Location",
            format!("{}{REDIRECT_TARGET_PATH}", server.uri()).as_str(),
        ))
        .mount(&server)
        .await;

    let message = embedding_failure(scores(&server).await);
    let probed = HttpCrossEncoder::probe_url(&rerank_url(&server), Some(MODEL)).await;

    assert_eq!(message, "rerank API 302 Found");
    assert!(probed.is_none(), "a redirecting endpoint must not attach");
}

#[tokio::test]
async fn the_url_drops_trailing_slashes_but_keeps_the_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RERANK_PATH))
        .and(query_param("tenant", "a"))
        .respond_with(discriminating())
        .expect(1)
        .mount(&server)
        .await;

    let scores = encoder(&format!("{}{RERANK_PATH}/?tenant=a", server.uri()))
        .score_documents(QUERY, &DOCUMENTS)
        .await
        .expect("scores");

    assert_eq!(scores, vec![0.9, 0.1]);
}

#[tokio::test]
async fn an_invalid_url_is_refused_at_construction() {
    for url in ["not a url", "ftp://host/rerank"] {
        match HttpCrossEncoder::new(url, MODEL) {
            Err(ContextError::Config(message)) => {
                assert!(message.starts_with(CLIENT_PREFIX), "{url}: {message}")
            }
            Err(other) => panic!("{url}: expected a configuration error, got {other:?}"),
            Ok(_) => panic!("{url}: expected a configuration error, got an encoder"),
        }
    }

    let probed = HttpCrossEncoder::probe_url("not a url", Some(MODEL)).await;

    assert!(probed.is_none(), "an invalid URL must not attach");
}

#[tokio::test]
async fn a_refusal_reports_only_its_status() {
    let server =
        endpoint(ResponseTemplate::new(SERVICE_UNAVAILABLE).set_body_string("secret document"))
            .await;

    let message = embedding_failure(scores(&server).await);

    assert_eq!(message, "rerank API 503 Service Unavailable");
}

#[tokio::test]
async fn an_empty_answer_is_an_error() {
    for body in [json!({ "results": [] }), json!({})] {
        let server = endpoint(ResponseTemplate::new(200).set_body_json(body.clone())).await;

        let message = embedding_failure(scores(&server).await);

        assert_eq!(message, "empty rerank response", "{body}");
    }
}

#[tokio::test]
async fn an_unparsable_answer_is_a_parse_error() {
    let server = endpoint(ResponseTemplate::new(200).set_body_string("not json")).await;

    let message = embedding_failure(scores(&server).await);

    assert!(message.starts_with(PARSE_PREFIX), "{message}");
}

#[tokio::test]
async fn a_misshapen_answer_does_not_echo_its_content() {
    let server =
        endpoint(ResponseTemplate::new(200).set_body_json(json!({ "results": "secret document" })))
            .await;

    let message = embedding_failure(scores(&server).await);

    assert_eq!(message, "rerank parse: invalid response");
    assert!(!message.contains("secret"), "{message}");
}

#[tokio::test]
async fn an_unreachable_endpoint_is_a_request_error() {
    let message = embedding_failure(
        encoder(&closed_url())
            .score_documents(QUERY, &DOCUMENTS)
            .await,
    );

    assert!(message.starts_with(REQUEST_PREFIX), "{message}");
}

#[tokio::test]
async fn probe_accepts_only_a_discriminating_scorer() {
    let ranking = endpoint(discriminating()).await;
    let flat = endpoint(scored(json!([
        {"index": 0, "relevance_score": 0.5},
        {"index": 1, "relevance_score": 0.5},
    ])))
    .await;

    let attached = HttpCrossEncoder::probe_url(&rerank_url(&ranking), Some(MODEL)).await;
    let refused = HttpCrossEncoder::probe_url(&rerank_url(&flat), Some(MODEL)).await;

    assert!(attached.is_some(), "a discriminating scorer must attach");
    assert!(refused.is_none(), "a flat scorer must not attach");
}
