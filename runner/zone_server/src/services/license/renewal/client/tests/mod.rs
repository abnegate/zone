use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use reqwest::header::{CONTENT_TYPE, LOCATION, USER_AGENT as USER_AGENT_HEADER};
use serde_json::{Value, json};
use tracing::Level;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const RENEW_PATH: &str = "/licenses/renew";
const ELSEWHERE_PATH: &str = "/elsewhere";
const KEY: &str = "zone-license-v1.current";
const RENEWED: &str = "zone-license-v1.renewed";
const SECRET: &str = "zone-license-v1.secret-key-text";
const SHORT_TIMEOUT: Duration = Duration::from_millis(200);
const SLOW_ANSWER: Duration = Duration::from_secs(3);

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("the captured log")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn logged<T>(work: impl FnOnce() -> T) -> (T, String) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(Level::DEBUG)
        .with_ansi(false)
        .finish();
    let value = tracing::subscriber::with_default(subscriber, work);
    let log = String::from_utf8_lossy(&captured.0.lock().expect("the captured log")).into_owned();
    (value, log)
}

fn settings(server: &MockServer) -> Settings {
    Settings {
        enabled: true,
        url: Url::parse(&format!("{}{RENEW_PATH}", server.uri())).unwrap(),
    }
}

fn client(server: &MockServer) -> Client {
    Client::new(&settings(server), Proxy::Direct).unwrap()
}

async fn answering(template: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RENEW_PATH))
        .respond_with(template)
        .expect(1)
        .mount(&server)
        .await;
    server
}

async fn answer(template: ResponseTemplate) -> Answer {
    let server = answering(template).await;
    client(&server).renew(KEY).await
}

#[test]
fn production_timeouts_are_thirty_seconds_and_ten_to_connect() {
    assert_eq!(TIMEOUT, Duration::from_secs(30));
    assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
}

#[test]
fn the_body_limit_is_sixty_four_kibibytes() {
    assert_eq!(BODY_LIMIT, 64 * 1024);
}

#[test]
fn a_client_is_configured_only_while_renewal_is_on() {
    assert!(Client::configured(&Settings::default()).is_some());
    let off = Settings {
        enabled: false,
        ..Settings::default()
    };
    assert!(Client::configured(&off).is_none());
}

#[test]
fn a_body_that_does_not_parse_is_logged_without_its_contents() {
    for body in [
        format!("\"{SECRET}\""),
        format!("{{\"license\": [\"{SECRET}\"]}}"),
        format!("{{\"license\": \"{SECRET}"),
    ] {
        let (answer, log) = logged(|| parse(body.as_bytes()));

        assert_eq!(answer, Answer::Malformed, "{body}");
        assert!(log.contains("WARN"), "{log}");
        assert!(
            log.contains("The license server's answer is not a renewed key"),
            "{log}"
        );
        assert!(log.contains("category="), "{log}");
        assert!(!log.contains(SECRET), "the log quotes the body: {log}");
    }
}

#[tokio::test]
async fn a_new_key_is_renewed() {
    assert_eq!(
        answer(ResponseTemplate::new(200).set_body_json(json!({ "license": RENEWED }))).await,
        Answer::Renewed(RENEWED.to_string())
    );
}

#[tokio::test]
async fn the_request_carries_only_the_license_as_json_from_zone() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RENEW_PATH))
        .and(header(USER_AGENT_HEADER, "Zone"))
        .and(header(CONTENT_TYPE, "application/json"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(client(&server).renew(KEY).await, Answer::Current);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body, json!({ "license": KEY }), "Zone sends only the key");
    assert!(
        requests[0].url.query().is_none(),
        "nothing rides in the query"
    );
}

#[tokio::test]
async fn no_content_means_the_key_is_current() {
    assert_eq!(answer(ResponseTemplate::new(204)).await, Answer::Current);
}

#[tokio::test]
async fn gone_means_the_subscription_ended() {
    assert_eq!(answer(ResponseTemplate::new(410)).await, Answer::Ended);
}

#[tokio::test]
async fn client_errors_are_refusals() {
    for status in [400, 401, 403, 409, 413, 415, 422] {
        assert_eq!(
            answer(ResponseTemplate::new(status)).await,
            Answer::Refused,
            "{status}"
        );
    }
}

#[tokio::test]
async fn missing_busy_and_failing_signers_are_unreachable() {
    for status in [404, 408, 429, 500, 502, 503, 504] {
        assert_eq!(
            answer(ResponseTemplate::new(status)).await,
            Answer::Unreachable,
            "{status}"
        );
    }
}

#[tokio::test]
async fn a_body_that_is_not_a_renewed_key_is_malformed() {
    for template in [
        ResponseTemplate::new(200).set_body_string("not json"),
        ResponseTemplate::new(200).set_body_json(json!({ "key": RENEWED })),
        ResponseTemplate::new(200).set_body_json(json!({ "license": 7 })),
        ResponseTemplate::new(200),
    ] {
        assert_eq!(answer(template).await, Answer::Malformed);
    }
}

#[tokio::test]
async fn an_oversize_body_is_unreachable() {
    let license = "k".repeat(BODY_LIMIT);
    assert_eq!(
        answer(ResponseTemplate::new(200).set_body_json(json!({ "license": license }))).await,
        Answer::Unreachable
    );
}

#[tokio::test]
async fn a_body_at_the_limit_is_read() {
    let padding = BODY_LIMIT - json!({ "license": "" }).to_string().len();
    let license = "k".repeat(padding);
    assert_eq!(
        answer(ResponseTemplate::new(200).set_body_json(json!({ "license": license }))).await,
        Answer::Renewed(license)
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RENEW_PATH))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header(LOCATION, format!("{}{ELSEWHERE_PATH}", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(ELSEWHERE_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "license": RENEWED })))
        .expect(0)
        .mount(&server)
        .await;

    assert_eq!(client(&server).renew(KEY).await, Answer::Unreachable);
}

#[tokio::test]
async fn a_slow_signer_times_out_as_unreachable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RENEW_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "license": RENEWED }))
                .set_delay(SLOW_ANSWER),
        )
        .mount(&server)
        .await;
    let client = Client::with_timeouts(
        &settings(&server),
        Proxy::Direct,
        SHORT_TIMEOUT,
        SHORT_TIMEOUT,
    )
    .unwrap();

    let started = Instant::now();
    assert_eq!(client.renew(KEY).await, Answer::Unreachable);
    assert!(
        started.elapsed() < SLOW_ANSWER,
        "the request gave up after {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_refused_connection_is_unreachable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let settings = Settings {
        enabled: true,
        url: Url::parse(&format!("http://{address}{RENEW_PATH}")).unwrap(),
    };

    let client = Client::new(&settings, Proxy::Direct).unwrap();

    assert_eq!(client.renew(KEY).await, Answer::Unreachable);
}
