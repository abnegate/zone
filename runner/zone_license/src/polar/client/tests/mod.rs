use std::net::TcpListener;
use std::time::Instant;

use chrono::{TimeZone, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::polar::SubscriptionStatus;

const TOKEN: &str = "polar_oat_client-test-token";
const ID: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const SUBSCRIPTION: &str = include_str!("../../../../tests/fixtures/polar/subscription.json");
const SHORT: Duration = Duration::from_millis(200);
const DELAY: Duration = Duration::from_secs(5);

fn id() -> Uuid {
    ID.parse().unwrap()
}

fn endpoint() -> String {
    format!("/{SUBSCRIPTIONS_PATH}/{ID}")
}

fn client(base: &str) -> Client {
    Client::new(AccessToken::new(TOKEN), base, Proxy::Direct).unwrap()
}

async fn answering(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(endpoint()))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn fetch(response: ResponseTemplate) -> Result<Option<OrderSubscription>, ApiError> {
    let server = answering(response).await;
    client(&server.uri()).subscription(id()).await
}

#[tokio::test]
async fn reads_a_subscription_with_the_token_as_a_bearer() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(endpoint()))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(header("accept", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SUBSCRIPTION))
        .expect(1)
        .mount(&server)
        .await;

    let subscription = client(&server.uri())
        .subscription(id())
        .await
        .unwrap()
        .expect("found");

    assert_eq!(subscription.id, ID);
    assert_eq!(subscription.status, SubscriptionStatus::Active);
    assert_eq!(subscription.seats, Some(5));
    assert_eq!(subscription.product_id.as_deref(), Some(PRODUCT));
    assert!(!subscription.cancel_at_period_end);
    assert_eq!(subscription.ended_at, None);
    assert_eq!(
        subscription.current_period_end,
        Utc.with_ymd_and_hms(2027, 11, 18, 11, 59, 58).unwrap()
    );
}

#[tokio::test]
async fn a_base_with_a_trailing_slash_reaches_the_same_path() {
    let server = answering(ResponseTemplate::new(200).set_body_string(SUBSCRIPTION)).await;
    let found = client(&format!("{}/", server.uri()))
        .subscription(id())
        .await
        .unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn a_missing_subscription_is_none() {
    let body = r#"{"error":"ResourceNotFound","detail":"Not found"}"#;
    assert_eq!(
        fetch(ResponseTemplate::new(404).set_body_string(body)).await,
        Ok(None)
    );
}

#[tokio::test]
async fn an_unparsable_success_is_malformed() {
    for body in ["", "not json", r#"{"id":"s"}"#] {
        assert_eq!(
            fetch(ResponseTemplate::new(200).set_body_string(body)).await,
            Err(ApiError::Malformed),
            "{body:?}"
        );
    }
}

#[tokio::test]
async fn a_body_over_the_limit_is_malformed() {
    let body = format!("{SUBSCRIPTION}{}", " ".repeat(BODY_LIMIT));
    assert_eq!(
        fetch(ResponseTemplate::new(200).set_body_string(body)).await,
        Err(ApiError::Malformed)
    );
}

async fn raw(response: String) -> Result<Option<OrderSubscription>, ApiError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        if stream.write_all(response.as_bytes()).await.is_ok() {
            let _ = stream.shutdown().await;
        }
    });
    let fetched = client(&format!("http://{address}"))
        .subscription(id())
        .await;
    server.await.unwrap();
    fetched
}

#[tokio::test]
async fn a_streamed_body_over_the_limit_is_malformed() {
    let chunk = format!("{:x}\r\n{}\r\n", BODY_LIMIT, " ".repeat(BODY_LIMIT));
    let response =
        format!("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n{chunk}{chunk}0\r\n\r\n");
    assert_eq!(raw(response).await, Err(ApiError::Malformed));
}

#[tokio::test]
async fn a_body_cut_short_is_unavailable() {
    let response = "HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n{\"id\"".to_owned();
    assert_eq!(raw(response).await, Err(ApiError::Unavailable));
}

#[tokio::test]
async fn a_body_at_the_limit_is_read() {
    let body = format!(
        "{SUBSCRIPTION}{}",
        " ".repeat(BODY_LIMIT - SUBSCRIPTION.len())
    );
    assert!(
        fetch(ResponseTemplate::new(200).set_body_string(body))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn throttling_and_server_errors_are_unavailable() {
    for status in [408, 429, 500, 502, 503, 504] {
        assert_eq!(
            fetch(ResponseTemplate::new(status)).await,
            Err(ApiError::Unavailable),
            "{status}"
        );
    }
}

#[tokio::test]
async fn any_other_status_is_refused() {
    for status in [400, 401, 403, 409, 422] {
        assert_eq!(
            fetch(ResponseTemplate::new(status)).await,
            Err(ApiError::Refused { status }),
            "{status}"
        );
    }
}

#[tokio::test]
async fn a_redirect_is_refused_and_not_followed() {
    let server = MockServer::start().await;
    let elsewhere = format!("{}/elsewhere", server.uri());
    Mock::given(method("GET"))
        .and(path(endpoint()))
        .respond_with(ResponseTemplate::new(302).insert_header("location", elsewhere.as_str()))
        .mount(&server)
        .await;
    Mock::given(path("/elsewhere"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SUBSCRIPTION))
        .expect(0)
        .mount(&server)
        .await;

    assert_eq!(
        client(&server.uri()).subscription(id()).await,
        Err(ApiError::Refused { status: 302 })
    );
}

#[tokio::test]
async fn a_slow_answer_times_out_as_unavailable() {
    let server = answering(
        ResponseTemplate::new(200)
            .set_body_string(SUBSCRIPTION)
            .set_delay(DELAY),
    )
    .await;
    let client =
        Client::with_timeout(AccessToken::new(TOKEN), &server.uri(), Proxy::Direct, SHORT).unwrap();

    let started = Instant::now();
    let fetched = client.subscription(id()).await;

    assert_eq!(fetched, Err(ApiError::Unavailable));
    assert!(started.elapsed() < DELAY, "{:?}", started.elapsed());
}

#[tokio::test]
async fn an_unreachable_api_is_unavailable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    assert_eq!(
        client(&format!("http://{address}"))
            .subscription(id())
            .await,
        Err(ApiError::Unavailable)
    );
}

#[test]
fn refuses_a_base_that_is_not_a_url() {
    for base in ["", "api.polar.sh", "not a url"] {
        assert!(
            Client::new(AccessToken::new(TOKEN), base, Proxy::Direct).is_err(),
            "{base:?}"
        );
    }
}

#[test]
fn production_points_at_the_polar_api_with_the_documented_timeouts() {
    assert_eq!(API, "https://api.polar.sh");
    assert_eq!(TIMEOUT, Duration::from_secs(10));
    assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(5));
    assert_eq!(BODY_LIMIT, 1024 * 1024);
    assert!(Client::new(AccessToken::new(TOKEN), API, Proxy::System).is_ok());
}

#[test]
fn the_token_never_shows_in_debug_or_errors() {
    let client = client(API);
    let rendered = format!("{client:?} {client:#?}");
    assert!(!rendered.contains(TOKEN), "{rendered}");
    for error in [
        ApiError::Unavailable,
        ApiError::Refused { status: 401 },
        ApiError::Malformed,
    ] {
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains(TOKEN), "{rendered}");
    }
}
