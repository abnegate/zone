use std::net::TcpListener;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeDelta, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{any, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::{Keyring, Plan, TrustedKey};

use super::*;
use crate::http::tests::Recording;
use crate::http::{RENEW_BODY_LIMIT, RENEW_PATH, Renewer, router};
use crate::license_id;
use crate::logging;
use crate::polar::{self, AccessToken, Proxy, WebhookSecret};
use crate::private_key::PrivateKey;
use crate::products::Products;
use crate::renewal::SETTLEMENT;
use crate::signer::Signer;

const FIXTURE: &str = include_str!("../../../../tests/fixtures/polar/subscription.json");

const TOKEN: &str = "polar_oat_renew-test-token";
const SECRET: &str = "whsec_renew-test-secret";
const KEY_ID: &str = "test-1";
const PREVIOUS_KEY_ID: &str = "test-0";
const SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const CUSTOMER: &str = "Acme Ltd";
const EMAIL: &str = "billing@acme.test";
const PERIOD_START: &str = "2026-11-18T11:59:58Z";
const PERIOD_END: &str = "2027-11-18T11:59:58Z";
const SHORT: Duration = Duration::from_millis(200);
const DELAY: Duration = Duration::from_secs(5);

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn settled() -> DateTime<Utc> {
    at(PERIOD_START) + SETTLEMENT
}

fn settling() -> DateTime<Utc> {
    at(PERIOD_START) + TimeDelta::hours(1)
}

fn signer() -> Signer {
    Signer::new(PrivateKey::new([5; 32]), KEY_ID)
}

fn previous() -> Signer {
    Signer::new(PrivateKey::new([6; 32]), PREVIOUS_KEY_ID)
}

fn keyring() -> Keyring {
    Keyring::new([
        TrustedKey::new(KEY_ID, signer().public()).unwrap(),
        TrustedKey::new(PREVIOUS_KEY_ID, previous().public()).unwrap(),
    ])
    .unwrap()
}

fn signed_by(
    signer: &Signer,
    expires_at: DateTime<Utc>,
    seats: u32,
    subscription: Option<&str>,
) -> String {
    signed_for(signer, expires_at, seats, subscription, Plan::Enterprise)
}

fn signed_for(
    signer: &Signer,
    expires_at: DateTime<Utc>,
    seats: u32,
    subscription: Option<&str>,
    plan: Plan,
) -> String {
    signer
        .sign(&Terms {
            id: Uuid::from_u128(1),
            customer: CUSTOMER.to_owned(),
            seats,
            issued_at: expires_at - TimeDelta::days(365),
            expires_at,
            subscription: subscription.map(str::to_owned),
            plan,
        })
        .unwrap()
}

fn key(expires_at: DateTime<Utc>, seats: u32) -> String {
    signed_by(&previous(), expires_at, seats, Some(SUBSCRIPTION))
}

fn planned(expires_at: DateTime<Utc>, seats: u32, plan: Plan) -> String {
    signed_for(&previous(), expires_at, seats, Some(SUBSCRIPTION), plan)
}

fn expiring() -> String {
    key(at(PERIOD_START), 5)
}

fn subscription(edit: impl FnOnce(&mut Value)) -> ResponseTemplate {
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    edit(&mut value);
    ResponseTemplate::new(200).set_body_json(value)
}

fn active() -> ResponseTemplate {
    subscription(|_| {})
}

async fn polar(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/subscriptions/{SUBSCRIPTION}")))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;
    server
}

async fn untouched() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    server
}

fn client(server: &MockServer) -> polar::Client {
    polar::Client::new(AccessToken::new(TOKEN), &server.uri(), Proxy::Direct).unwrap()
}

fn context(outbox: Arc<Recording>) -> Context {
    listing(outbox, PRODUCT)
}

fn listing(outbox: Arc<Recording>, products: &str) -> Context {
    Context::new(
        signer(),
        WebhookSecret::new(SECRET),
        Products::parse(products).unwrap(),
        outbox,
    )
}

fn posting(body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(RENEW_PATH)
        .header(CONTENT_TYPE, "application/json")
        .body(body.into())
        .unwrap()
}

fn asking(key: &str) -> Request<Body> {
    posting(json!({ "license": key }).to_string())
}

fn oversize() -> Request<Body> {
    posting(json!({ "license": "a".repeat(RENEW_BODY_LIMIT) }).to_string())
}

async fn answer(app: Router, request: Request<Body>) -> (StatusCode, Option<Value>) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = (!bytes.is_empty()).then(|| serde_json::from_slice(&bytes).unwrap());
    (status, body)
}

async fn renewing(client: polar::Client, request: Request<Body>) -> (StatusCode, Option<Value>) {
    renewing_at(client, request, settled).await
}

async fn renewing_at(
    client: polar::Client,
    request: Request<Body>,
    clock: fn() -> DateTime<Utc>,
) -> (StatusCode, Option<Value>) {
    renewing_listed(client, request, clock, PRODUCT).await
}

async fn renewing_listed(
    client: polar::Client,
    request: Request<Body>,
    clock: fn() -> DateTime<Utc>,
    products: &str,
) -> (StatusCode, Option<Value>) {
    let outbox = Arc::new(Recording::default());
    let renewer = Renewer::new(keyring(), client).with_clock(clock);
    let app = router(listing(outbox.clone(), products).with_renewer(renewer));
    let answered = answer(app, request).await;
    assert!(outbox.sent().is_empty(), "the renew route never emails");
    answered
}

async fn renew(server: &MockServer, request: Request<Body>) -> (StatusCode, Option<Value>) {
    renewing(client(server), request).await
}

async fn status_for(response: ResponseTemplate) -> StatusCode {
    let server = polar(response).await;
    let (status, body) = renew(&server, asking(&expiring())).await;
    assert_eq!(body, None);
    status
}

async fn refused(request: Request<Body>, expected: StatusCode) {
    let server = untouched().await;
    let (status, body) = renew(&server, request).await;
    assert_eq!(status, expected);
    assert_eq!(body, None);
}

fn renewed(body: Option<Value>) -> zone_enterprise::License {
    let body = body.expect("a renewed key");
    let text = body["license"].as_str().expect("a license field");
    assert_eq!(body.as_object().unwrap().len(), 1, "{body}");
    keyring().verify(text).expect("the renewed key verifies")
}

#[tokio::test]
async fn an_expiring_key_renews_to_the_current_period() {
    let server = polar(active()).await;
    let (status, body) = renew(&server, asking(&expiring())).await;

    assert_eq!(status, StatusCode::OK);
    let license = renewed(body);
    let payload = license.payload();
    assert_eq!(
        payload.id,
        license_id::derive(SUBSCRIPTION, at(PERIOD_END), 5, Plan::Enterprise)
    );
    assert_eq!(payload.customer, CUSTOMER);
    assert_eq!(payload.seats, 5);
    assert_eq!(payload.issued_at, at(PERIOD_START));
    assert_eq!(payload.expires_at, at(PERIOD_END));
    assert_eq!(payload.plan, Some(Plan::Enterprise));
    assert_eq!(license.subscription(), Some(SUBSCRIPTION));
}

#[tokio::test]
async fn a_move_to_a_team_product_renews_to_a_team_key_in_the_same_period() {
    let server = polar(active()).await;
    let (status, body) = renewing_listed(
        client(&server),
        asking(&key(at(PERIOD_END), 5)),
        settled,
        &format!("{PRODUCT}=team"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let license = renewed(body);
    assert_eq!(license.plan(), Plan::Team);
    assert_eq!(license.seats(), 5);
    assert_eq!(license.expires_at(), at(PERIOD_END));
    assert_eq!(
        license.id(),
        license_id::derive(SUBSCRIPTION, at(PERIOD_END), 5, Plan::Team)
    );
}

#[tokio::test]
async fn a_move_to_an_enterprise_product_renews_to_an_enterprise_key_in_the_same_period() {
    let server = polar(active()).await;
    let (status, body) = renew(&server, asking(&planned(at(PERIOD_END), 5, Plan::Team))).await;

    assert_eq!(status, StatusCode::OK);
    let license = renewed(body);
    assert_eq!(license.plan(), Plan::Enterprise);
    assert_eq!(license.seats(), 5);
}

#[tokio::test]
async fn a_team_key_on_a_team_product_covering_the_period_is_current() {
    let server = polar(active()).await;
    let (status, body) = renewing_listed(
        client(&server),
        asking(&planned(at(PERIOD_END), 5, Plan::Team)),
        settled,
        &format!("{PRODUCT}=team"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, None);
}

#[tokio::test]
async fn the_renewed_key_is_signed_with_the_signers_key_id() {
    let server = polar(active()).await;
    let (_, body) = renew(&server, asking(&expiring())).await;

    let license = renewed(body);
    assert_eq!(license.key_id(), KEY_ID);
    assert_eq!(license.payload().key_id, KEY_ID);
}

#[tokio::test]
async fn a_key_with_surrounding_whitespace_still_renews() {
    let server = polar(active()).await;
    let (status, _) = renew(&server, asking(&format!("\n {} \n", expiring()))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_seat_change_in_the_same_period_renews() {
    let server = polar(active()).await;
    let (status, body) = renew(&server, asking(&key(at(PERIOD_END), 3))).await;

    assert_eq!(status, StatusCode::OK);
    let license = renewed(body);
    assert_eq!(license.seats(), 5);
    assert_eq!(license.expires_at(), at(PERIOD_END));
}

#[tokio::test]
async fn a_key_covering_the_current_period_is_current() {
    let server = polar(active()).await;
    let (status, body) = renew(&server, asking(&key(at(PERIOD_END), 5))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, None);
}

#[tokio::test]
async fn a_new_period_is_current_until_it_settles() {
    let server = polar(active()).await;
    let (status, body) = renewing_at(client(&server), asking(&expiring()), settling).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, None);
}

#[tokio::test]
async fn a_seat_change_in_the_same_period_renews_while_settling() {
    let server = polar(active()).await;
    let (status, body) =
        renewing_at(client(&server), asking(&key(at(PERIOD_END), 3)), settling).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renewed(body).seats(), 5);
}

#[tokio::test]
async fn a_trialing_subscription_is_current() {
    let trialing = subscription(|value| value["status"] = json!("trialing"));
    assert_eq!(status_for(trialing).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn without_a_renewer_the_route_is_not_found_whatever_is_posted() {
    let requests = [
        asking(&expiring()),
        oversize(),
        posting("not json"),
        Request::builder()
            .method(Method::POST)
            .uri(RENEW_PATH)
            .body(Body::from("{}"))
            .unwrap(),
    ];
    for request in requests {
        let outbox = Arc::new(Recording::default());
        let (status, body) = answer(router(context(outbox.clone())), request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, None);
        assert!(outbox.sent().is_empty());
    }
}

#[tokio::test]
async fn a_body_over_the_limit_is_too_large() {
    refused(oversize(), StatusCode::PAYLOAD_TOO_LARGE).await;
}

#[tokio::test]
async fn a_body_at_the_limit_is_read() {
    let filler = RENEW_BODY_LIMIT - json!({ "license": "" }).to_string().len();
    refused(
        posting(json!({ "license": "a".repeat(filler) }).to_string()),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
}

#[tokio::test]
async fn invalid_json_is_a_bad_request() {
    refused(posting("{\"license\":"), StatusCode::BAD_REQUEST).await;
    refused(posting("not json"), StatusCode::BAD_REQUEST).await;
}

#[tokio::test]
async fn a_missing_or_mistyped_license_is_a_bad_request() {
    refused(posting("{}"), StatusCode::BAD_REQUEST).await;
    refused(posting("{\"key\":\"value\"}"), StatusCode::BAD_REQUEST).await;
    refused(posting("{\"license\":5}"), StatusCode::BAD_REQUEST).await;
}

#[tokio::test]
async fn a_missing_or_other_content_type_is_a_bad_request() {
    let body = json!({ "license": expiring() }).to_string();
    let missing = Request::builder()
        .method(Method::POST)
        .uri(RENEW_PATH)
        .body(Body::from(body.clone()))
        .unwrap();
    refused(missing, StatusCode::BAD_REQUEST).await;

    let text = Request::builder()
        .method(Method::POST)
        .uri(RENEW_PATH)
        .header(CONTENT_TYPE, "text/plain")
        .body(Body::from(body))
        .unwrap();
    refused(text, StatusCode::BAD_REQUEST).await;
}

#[tokio::test]
async fn a_key_that_does_not_verify_is_unprocessable() {
    refused(asking("not a key"), StatusCode::UNPROCESSABLE_ENTITY).await;
    let stranger = Signer::new(PrivateKey::new([9; 32]), "test-9");
    let untrusted = signed_by(&stranger, at(PERIOD_START), 5, Some(SUBSCRIPTION));
    refused(asking(&untrusted), StatusCode::UNPROCESSABLE_ENTITY).await;
}

#[tokio::test]
async fn a_key_without_a_subscription_is_unprocessable() {
    let unsubscribed = signed_by(&previous(), at(PERIOD_START), 5, None);
    refused(asking(&unsubscribed), StatusCode::UNPROCESSABLE_ENTITY).await;
}

#[tokio::test]
async fn a_subscription_id_that_is_not_a_uuid_is_unprocessable() {
    let malformed = signed_by(&previous(), at(PERIOD_START), 5, Some("sub_01HZX0Q7M8"));
    refused(asking(&malformed), StatusCode::UNPROCESSABLE_ENTITY).await;
}

#[tokio::test]
async fn a_subscription_polar_does_not_know_is_gone() {
    let missing = ResponseTemplate::new(404)
        .set_body_json(json!({ "error": "ResourceNotFound", "detail": "Not found" }));
    assert_eq!(status_for(missing).await, StatusCode::GONE);
}

#[tokio::test]
async fn a_subscription_that_will_not_renew_is_gone() {
    let responses = [
        subscription(|value| value["status"] = json!("canceled")),
        subscription(|value| value["status"] = json!("past_due")),
        subscription(|value| value["ended_at"] = json!("2027-01-02T03:04:05Z")),
        subscription(|value| value["product_id"] = json!("another-product")),
    ];
    for response in responses {
        assert_eq!(status_for(response).await, StatusCode::GONE);
    }
}

#[tokio::test]
async fn a_scheduled_cancellation_still_renews() {
    let server = polar(subscription(|value| {
        value["cancel_at_period_end"] = json!(true)
    }))
    .await;
    let (status, body) = renew(&server, asking(&expiring())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renewed(body).expires_at(), at(PERIOD_END));
}

#[tokio::test]
async fn polar_refusing_the_request_is_a_bad_gateway() {
    for refusal in [401, 403, 400] {
        assert_eq!(
            status_for(ResponseTemplate::new(refusal)).await,
            StatusCode::BAD_GATEWAY,
            "{refusal}"
        );
    }
}

#[tokio::test]
async fn an_unreadable_subscription_is_a_bad_gateway() {
    let garbled = ResponseTemplate::new(200).set_body_string("not json");
    assert_eq!(status_for(garbled).await, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn a_subscription_without_seats_is_a_bad_gateway() {
    let seatless = subscription(|value| value["seats"] = Value::Null);
    assert_eq!(status_for(seatless).await, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn polar_being_unavailable_is_unavailable() {
    for unavailable in [500, 503, 429, 408] {
        assert_eq!(
            status_for(ResponseTemplate::new(unavailable)).await,
            StatusCode::SERVICE_UNAVAILABLE,
            "{unavailable}"
        );
    }
}

#[tokio::test]
async fn polar_timing_out_is_unavailable() {
    let server = polar(active().set_delay(DELAY)).await;
    let client =
        polar::Client::with_timeout(AccessToken::new(TOKEN), &server.uri(), Proxy::Direct, SHORT)
            .unwrap();
    let (status, body) = renewing(client, asking(&expiring())).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, None);
}

#[tokio::test]
async fn polar_being_unreachable_is_unavailable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let client = polar::Client::new(
        AccessToken::new(TOKEN),
        &format!("http://{address}"),
        Proxy::Direct,
    )
    .unwrap();
    let (status, body) = renewing(client, asking(&expiring())).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, None);
}

#[tokio::test]
async fn terms_the_signer_refuses_are_an_internal_error() {
    let empty = subscription(|value| value["current_period_start"] = json!(PERIOD_END));
    let server = polar(empty).await;
    let (status, body) = renewing_at(client(&server), asking(&expiring()), || {
        at(PERIOD_END) + SETTLEMENT
    })
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body, None);
}

#[tokio::test]
async fn the_route_takes_only_post() {
    let server = untouched().await;
    let request = Request::builder()
        .uri(RENEW_PATH)
        .body(Body::empty())
        .unwrap();
    let (status, _) = renew(&server, request).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

async fn logged(level: tracing::Level, response: ResponseTemplate, key: &str) -> String {
    logged_at(level, response, key, settled).await
}

async fn logged_at(
    level: tracing::Level,
    response: ResponseTemplate,
    key: &str,
    clock: fn() -> DateTime<Utc>,
) -> String {
    let server = polar(response).await;
    logging::capture(level, async {
        renewing_at(client(&server), asking(key), clock).await;
    })
    .await
}

#[tokio::test]
async fn a_renewal_logs_its_ids_and_never_the_key_or_the_customer() {
    let key = expiring();
    let logged = logged(tracing::Level::INFO, active(), &key).await;

    let old = keyring().verify(&key).unwrap().id();
    let new = license_id::derive(SUBSCRIPTION, at(PERIOD_END), 5, Plan::Enterprise);
    for expected in [
        "outcome=renewed".to_owned(),
        format!("subscription=\"{SUBSCRIPTION}\""),
        format!("license={old}"),
        format!("renewed={new}"),
        "status=\"active\"".to_owned(),
    ] {
        assert!(
            logged.contains(&expected),
            "{expected} missing from {logged}"
        );
    }
    for secret in [key.as_str(), CUSTOMER, EMAIL, TOKEN] {
        assert!(!logged.contains(secret), "{secret} leaked into {logged}");
    }
}

#[tokio::test]
async fn a_subscription_that_will_not_renew_logs_the_reason() {
    let response = subscription(|value| value["product_id"] = json!("another-product"));
    let logged = logged(tracing::Level::INFO, response, &expiring()).await;
    assert!(logged.contains("outcome=ended"), "{logged}");
    assert!(logged.contains("reason=product"), "{logged}");
}

#[tokio::test]
async fn a_settling_period_logs_why_it_was_held() {
    let logged = logged_at(tracing::Level::INFO, active(), &expiring(), settling).await;
    assert!(logged.contains("outcome=current"), "{logged}");
    assert!(logged.contains("reason=\"settling\""), "{logged}");
    assert!(logged.contains("status=\"active\""), "{logged}");
}

#[tokio::test]
async fn polar_refusing_the_token_is_logged_as_an_error() {
    let logged = logged(
        tracing::Level::ERROR,
        ResponseTemplate::new(401),
        &expiring(),
    )
    .await;
    assert!(logged.contains("outcome=unauthorized"), "{logged}");
    assert!(!logged.contains(TOKEN), "{logged}");
}

#[tokio::test]
async fn a_subscription_without_seats_is_logged_as_an_error() {
    let response = subscription(|value| value["seats"] = Value::Null);
    let logged = logged(tracing::Level::ERROR, response, &expiring()).await;
    assert!(logged.contains("outcome=misconfigured"), "{logged}");
}

#[tokio::test]
async fn polar_being_unavailable_is_not_an_error() {
    let logged = logged(
        tracing::Level::ERROR,
        ResponseTemplate::new(503),
        &expiring(),
    )
    .await;
    assert_eq!(logged, "");
}
