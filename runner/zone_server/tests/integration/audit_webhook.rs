use std::net::TcpListener;
use std::time::Duration;

use serde::Deserialize;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::audit::destination::{WebhookSecrets, WebhookSettings};
use zone_enterprise::audit::signature::{SIGNATURE_HEADER, TIMESTAMP_HEADER, verify};
use zone_enterprise::audit::{Event, FailureCode};
use zone_server::db::audit::scope::Scope;
use zone_server::services::audit::transport::webhook::{CONTENT, Webhook};
use zone_server::services::audit::transport::{Failure, Transport, client};

use crate::common::audit::{batch, header};

const SECRET: &str = "whsec-audit-test";
const HOOK: &str = "/hook";
const SCHEMA: [&str; 14] = [
    "id",
    "version",
    "created_at",
    "organization_id",
    "workspace_id",
    "actor_id",
    "actor_email",
    "action",
    "resource_type",
    "resource_id",
    "old_values",
    "new_values",
    "ip_address",
    "user_agent",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    events: Vec<Event>,
}

fn webhook(url: &str, client: reqwest::Client) -> Webhook {
    Webhook::new(
        &WebhookSettings {
            url: url.to_owned(),
        },
        &WebhookSecrets {
            signing_secret: SECRET.to_owned(),
        },
        client,
    )
}

async fn receiver(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(HOOK))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn refused(response: ResponseTemplate) -> Failure {
    let server = receiver(response).await;
    webhook(&format!("{}{HOOK}", server.uri()), client::client())
        .deliver(&batch(Scope::Server, 1))
        .await
        .expect_err("a non-2xx response is a failure")
}

fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn a_loopback_receiver_gets_a_signed_batch_of_events() {
    let server = receiver(ResponseTemplate::new(204)).await;
    let sent = batch(Scope::Organization(Uuid::new_v4()), 3);

    webhook(&format!("{}{HOOK}", server.uri()), client::client())
        .deliver(&sent)
        .await
        .expect("a 2xx response delivers the batch");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(header(request, "content-type"), CONTENT);
    let timestamp: i64 = header(request, TIMESTAMP_HEADER).parse().unwrap();
    assert!(
        verify(
            SECRET.as_bytes(),
            timestamp,
            &request.body,
            header(request, SIGNATURE_HEADER)
        ),
        "the signature verifies against the captured body"
    );
    assert!(!verify(
        b"another-secret",
        timestamp,
        &request.body,
        header(request, SIGNATURE_HEADER)
    ));
    assert!(header(request, "user-agent").starts_with("Zone/"));

    let payload: Payload = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(payload.events, sent.events);

    let text = std::str::from_utf8(&request.body).unwrap();
    assert!(text.starts_with(r#"{"events":[{"id":""#), "{text}");
    let first = &text[..text.find(r#"},{"id":""#).unwrap()];
    let positions: Vec<usize> = SCHEMA
        .iter()
        .map(|key| {
            first
                .find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("the event carries {key}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "keys follow the version 1 schema order: {first}"
    );
}

#[tokio::test]
async fn an_empty_batch_sends_nothing() {
    let server = receiver(ResponseTemplate::new(200)).await;
    let empty = batch(Scope::Server, 0);

    webhook(&format!("{}{HOOK}", server.uri()), client::client())
        .deliver(&empty)
        .await
        .unwrap();

    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_503_with_retry_after_asks_for_that_wait() {
    let failure = refused(ResponseTemplate::new(503).insert_header("Retry-After", "120")).await;

    assert_eq!(
        failure,
        Failure {
            code: FailureCode::Status,
            message: "HTTP 503".to_owned(),
            retry_after: Some(Duration::from_secs(120)),
        }
    );
}

#[tokio::test]
async fn a_429_with_retry_after_asks_for_that_wait() {
    let failure = refused(ResponseTemplate::new(429).insert_header("Retry-After", "7")).await;

    assert_eq!(failure.code, FailureCode::Status);
    assert_eq!(failure.message, "HTTP 429");
    assert_eq!(failure.retry_after, Some(Duration::from_secs(7)));
}

#[tokio::test]
async fn a_server_error_never_carries_the_response_body() {
    let failure = refused(
        ResponseTemplate::new(500)
            .insert_header("Retry-After", "30")
            .set_body_string("stack trace with secret-token-1234"),
    )
    .await;

    assert_eq!(
        failure,
        Failure::new(FailureCode::Status, "HTTP 500"),
        "only 429 and 503 honour Retry-After, and the body is never read"
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(HOOK))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", format!("{}/moved", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(path("/moved"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let failure = webhook(&format!("{}{HOOK}", server.uri()), client::client())
        .deliver(&batch(Scope::Server, 1))
        .await
        .unwrap_err();

    assert_eq!(failure, Failure::new(FailureCode::Status, "HTTP 302"));
    server.verify().await;
}

#[tokio::test]
async fn a_metadata_address_is_a_target_failure() {
    for url in [
        "http://169.254.169.254/latest/meta-data",
        "http://metadata.google.internal/hook",
    ] {
        let failure = webhook(url, client::client())
            .deliver(&batch(Scope::Server, 1))
            .await
            .unwrap_err();

        assert_eq!(failure.code, FailureCode::Target, "{url}");
        assert_eq!(failure.retry_after, None);
    }
}

#[tokio::test]
async fn a_closed_port_is_a_connect_failure() {
    let failure = webhook(
        &format!("http://127.0.0.1:{}{HOOK}", closed_port()),
        client::client(),
    )
    .deliver(&batch(Scope::Server, 1))
    .await
    .unwrap_err();

    assert_eq!(failure.code, FailureCode::Connect, "{failure:?}");
}

#[tokio::test]
async fn https_to_a_plain_http_receiver_is_a_tls_failure() {
    let server = receiver(ResponseTemplate::new(200)).await;

    let failure = webhook(
        &format!("https://{}{HOOK}", server.address()),
        client::client(),
    )
    .deliver(&batch(Scope::Server, 1))
    .await
    .unwrap_err();

    assert_eq!(failure.code, FailureCode::Tls, "{failure:?}");
}

#[tokio::test]
async fn a_slow_receiver_is_a_timeout_failure() {
    let server = receiver(ResponseTemplate::new(200).set_delay(Duration::from_secs(5))).await;
    let impatient = client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();

    let failure = webhook(&format!("{}{HOOK}", server.uri()), impatient)
        .deliver(&batch(Scope::Server, 1))
        .await
        .unwrap_err();

    assert_eq!(failure.code, FailureCode::Timeout, "{failure:?}");
}
