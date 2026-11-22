use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::header::{ALLOW, AUTHORIZATION, CACHE_CONTROL};
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use http_body_util::BodyExt;
use rusty_s3::{Bucket, Credentials, UrlStyle};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{any, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::{Keyring, Plan, TrustedKey};

use crate::bundle::{BundleSettings, DEFAULT_PREFIX, Presigner};
use crate::gate::{Gate, Limits, Revoked, Throttle, TrustedProxies};
use crate::http::tests::Recording;
use crate::http::{Context, Distributor, router};
use crate::logging;
use crate::polar::{Proxy, WebhookSecret};
use crate::private_key::PrivateKey;
use crate::products::Products;
use crate::signer::Signer;
use crate::terms::Terms;

const SECRET: &str = "whsec_bundles-test-secret";
const KEY_ID: &str = "test-bundles-1";
const SEED: [u8; 32] = [11; 32];
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const CUSTOMER: &str = "Acme Bundles Ltd";
const LICENSE: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_2e01);
const BUCKET: &str = "zone-distribution";
const REGION: &str = "ap-southeast-2";
const ACCESS_KEY: &str = "AKIAZONEBUNDLEUNIT01";
const SECRET_KEY: &str = "bundle-unit-secret/never+logged";
const BUNDLE: &str = "/bundles/1.4.3-lts/amd64";
const MARKER: &str =
    "/zone-distribution/bundles/1.4.3-lts/zone-bundle-1.4.3-lts-amd64.tar.sha256.sigstore.json";
const PEER: &str = "203.0.113.9:41000";
const SHORT: Duration = Duration::from_millis(200);
const DELAY: Duration = Duration::from_secs(5);

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-23T00:00:00Z")
        .unwrap()
        .to_utc()
}

fn signer() -> Signer {
    Signer::new(PrivateKey::new(SEED), KEY_ID)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(KEY_ID, signer().public()).unwrap()]).unwrap()
}

fn key() -> String {
    signer()
        .sign(&Terms {
            id: LICENSE,
            customer: CUSTOMER.to_owned(),
            seats: 10,
            issued_at: now() - TimeDelta::days(30),
            expires_at: now() + TimeDelta::days(335),
            subscription: None,
            plan: Plan::Enterprise,
        })
        .unwrap()
}

fn context() -> Context {
    Context::new(
        signer(),
        WebhookSecret::new(SECRET),
        Products::parse(PRODUCT).unwrap(),
        std::sync::Arc::new(Recording::default()),
    )
}

fn presigner(endpoint: &str, timeout: Duration) -> Presigner {
    let settings = BundleSettings {
        bucket: Bucket::new(endpoint.parse().unwrap(), UrlStyle::Path, BUCKET, REGION).unwrap(),
        prefix: DEFAULT_PREFIX.to_owned(),
        credentials: Credentials::new(ACCESS_KEY, SECRET_KEY),
    };
    Presigner::with_timeout(settings, Proxy::Direct, timeout).unwrap()
}

fn distributed(storage: &MockServer, timeout: Duration) -> Router {
    let gate = Gate::new(keyring(), Revoked::default(), TrustedProxies::default()).with_clock(now);
    let distributor = Distributor::new(
        gate,
        presigner(&storage.uri(), timeout),
        Throttle::new(Limits::default()),
    );
    router(context().with_distributor(distributor))
}

fn peered(app: Router) -> Router {
    app.layer(MockConnectInfo(PEER.parse::<SocketAddr>().unwrap()))
}

fn request(method: Method, uri: &str) -> Request<Body> {
    let credentials = STANDARD.encode(format!("{LICENSE}:{}", key()));
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Basic {credentials}"))
        .body(Body::empty())
        .unwrap()
}

async fn storage(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(MARKER))
        .and(header("range", "bytes=0-0"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn untouched() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(206))
        .mount(&server)
        .await;
    server
}

async fn assert_untouched(server: &MockServer) {
    let received = server.received_requests().await.unwrap();
    assert!(received.is_empty(), "storage was called: {received:?}");
}

fn line<'a>(logged: &'a str, outcome: &str) -> &'a str {
    logged
        .lines()
        .find(|line| line.contains(&format!("outcome={outcome}")))
        .unwrap_or_else(|| panic!("no {outcome} line in {logged}"))
}

#[tokio::test]
async fn a_signer_without_bundles_answers_not_found_whatever_is_asked() {
    let app = peered(router(context()));

    let logged = logging::capture(tracing::Level::INFO, async {
        let response = app.oneshot(request(Method::GET, BUNDLE)).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    })
    .await;

    line(&logged, "disabled");
}

#[tokio::test]
async fn post_and_head_answer_method_not_allowed_with_only_get_allowed() {
    let storage = untouched().await;
    for method in [Method::POST, Method::HEAD, Method::PUT, Method::OPTIONS] {
        let app = peered(distributed(&storage, Duration::from_secs(10)));

        let logged = logging::capture(tracing::Level::TRACE, async {
            let response = app.oneshot(request(method.clone(), BUNDLE)).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method}"
            );
            let allowed: Vec<_> = response.headers().get_all(ALLOW).iter().collect();
            assert_eq!(allowed, ["GET"], "{method}");
        })
        .await;

        assert!(!logged.contains("outcome="), "{method}: {logged}");
    }
    assert_untouched(&storage).await;
}

#[tokio::test]
async fn a_path_segment_that_does_not_decode_is_unknown_without_calling_storage() {
    let storage = untouched().await;
    for uri in [
        "/bundles/%FF/amd64",
        "/bundles/1.4.3-lts/%C0%AF",
        "/bundles/..%2F1.4.3-lts/amd64",
        "/bundles/1.4.3-lts%2F..%2Fx/amd64",
        "/bundles/1.4.3-lts/amd64%00",
    ] {
        let app = peered(distributed(&storage, Duration::from_secs(10)));

        let logged = logging::capture(tracing::Level::INFO, async {
            let response = app.oneshot(request(Method::GET, uri)).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        })
        .await;

        let line = line(&logged, "unknown");
        assert!(line.contains(&format!("license={LICENSE}")), "{line}");
        assert!(!line.contains("%"), "{uri}: {line}");
        assert!(!line.contains(".."), "{uri}: {line}");
    }
    assert_untouched(&storage).await;
}

#[tokio::test]
async fn only_the_parsed_half_of_an_unknown_bundle_is_logged() {
    let storage = untouched().await;
    let app = peered(distributed(&storage, Duration::from_secs(10)));

    let logged = logging::capture(tracing::Level::INFO, async {
        let response = app
            .oneshot(request(Method::GET, "/bundles/1.4.3-lts/riscv64"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    })
    .await;

    let line = line(&logged, "unknown");
    assert!(line.contains("version=1.4.3-lts"), "{line}");
    assert!(!line.contains("architecture="), "{line}");
    assert!(!line.contains("riscv64"), "{line}");
    assert_untouched(&storage).await;
}

#[tokio::test]
async fn issued_links_are_never_cached_and_log_every_field_but_no_secret() {
    let storage = storage(ResponseTemplate::new(206)).await;
    let app = peered(distributed(&storage, Duration::from_secs(10)));

    let logged = logging::capture(tracing::Level::TRACE, async {
        let response = app.oneshot(request(Method::GET, BUNDLE)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["files"].as_array().unwrap().len(), 3);
    })
    .await;

    let line = line(&logged, "issued");
    for field in [
        format!("license={LICENSE}"),
        "address=203.0.113.9".to_owned(),
        "version=1.4.3-lts".to_owned(),
        "architecture=amd64".to_owned(),
    ] {
        assert!(line.contains(&field), "{field} missing from {line}");
    }
    for secret in [
        key().as_str(),
        SECRET_KEY,
        CUSTOMER,
        "X-Amz-Signature",
        "X-Amz-Credential",
    ] {
        assert!(!logged.contains(secret), "{secret} logged in {logged}");
    }
}

#[tokio::test]
async fn without_a_peer_address_it_fails_closed_and_presigns_nothing() {
    let storage = untouched().await;
    let app = distributed(&storage, Duration::from_secs(10));

    let logged = logging::capture(tracing::Level::INFO, async {
        let response = app.oneshot(request(Method::GET, BUNDLE)).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "errors": [{ "code": "UNKNOWN", "message": "The client address can't be read." }] })
        );
    })
    .await;

    assert!(line(&logged, "unaddressable").contains("ERROR"), "{logged}");
    assert!(!logged.contains("outcome=issued"), "{logged}");
    assert!(!logged.contains("X-Amz-Signature"), "{logged}");
    assert_untouched(&storage).await;
}

#[tokio::test]
async fn storage_that_times_out_is_unavailable_and_logged_as_a_warning() {
    let storage = storage(ResponseTemplate::new(206).set_delay(DELAY)).await;
    let app = peered(distributed(&storage, SHORT));

    let logged = logging::capture(tracing::Level::INFO, async {
        let response = app.oneshot(request(Method::GET, BUNDLE)).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    })
    .await;

    let line = line(&logged, "unavailable");
    assert!(line.contains(" WARN "), "{line}");
    assert!(!line.contains("code="), "{line}");
}
