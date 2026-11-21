use std::sync::Mutex;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zone_enterprise::{Keyring, License, Plan, TrustedKey};

use super::*;
use crate::license_id;
use crate::logging;
use crate::mail::{Letter, MailError, Outbox};
use crate::polar::{ID_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER, WebhookSecret, signature};
use crate::private_key::PrivateKey;
use crate::products::Products;
use crate::signer::Signer;

const CREATE: &[u8] =
    include_bytes!("../../../tests/fixtures/polar/order_paid_subscription_create.json");
const PURCHASE: &[u8] = include_bytes!("../../../tests/fixtures/polar/order_paid_purchase.json");
const UNPAID: &[u8] = include_bytes!("../../../tests/fixtures/polar/order_paid_unpaid.json");
const NO_EMAIL: &[u8] = include_bytes!("../../../tests/fixtures/polar/order_paid_no_email.json");
const NO_SEATS: &[u8] = include_bytes!("../../../tests/fixtures/polar/order_paid_no_seats.json");
const SUBSCRIPTION_UPDATED: &[u8] =
    include_bytes!("../../../tests/fixtures/polar/subscription_updated.json");

const SECRET: &str = "whsec_router-test-secret";
const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [5; 32];
const WEBHOOK: &str = "msg_router";
const SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
const EMAIL: &str = "billing@acme.test";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const TEAM_PRODUCT: &str = "8a2b6c1d-4e3f-4a5b-9c6d-7e8f9a0b1c2d";

#[derive(Default)]
pub(super) struct Recording {
    sent: Mutex<Vec<(String, Letter)>>,
}

#[async_trait]
impl Outbox for Recording {
    async fn send(&self, recipient: &str, letter: &Letter) -> Result<(), MailError> {
        self.sent
            .lock()
            .unwrap()
            .push((recipient.to_owned(), letter.clone()));
        Ok(())
    }
}

impl Recording {
    pub(super) fn sent(&self) -> Vec<(String, Letter)> {
        self.sent.lock().unwrap().clone()
    }
}

struct Failing;

#[async_trait]
impl Outbox for Failing {
    async fn send(&self, _: &str, _: &Letter) -> Result<(), MailError> {
        Err(MailError::Timeout {
            host: "smtp.example.test".to_owned(),
        })
    }
}

struct Unsendable;

#[async_trait]
impl Outbox for Unsendable {
    async fn send(&self, _: &str, _: &Letter) -> Result<(), MailError> {
        Err(MailError::Recipient)
    }
}

fn products() -> Products {
    Products::parse(PRODUCT).unwrap()
}

fn listing(list: &str) -> Products {
    Products::parse(list).unwrap()
}

fn verified(letter: &Letter) -> License {
    letter
        .text()
        .lines()
        .find_map(|line| keyring().verify(line).ok())
        .expect("the letter carries a license that verifies")
}

fn signer() -> Signer {
    Signer::new(PrivateKey::new(SEED), KEY_ID)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(KEY_ID, signer().public()).unwrap()]).unwrap()
}

fn app(outbox: Arc<dyn Outbox>, products: Products) -> Router {
    router(Context::new(
        signer(),
        WebhookSecret::new(SECRET),
        products,
        outbox,
    ))
}

fn delivery(body: &[u8], timestamp: i64, key: &[u8]) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(WEBHOOK_PATH)
        .header(ID_HEADER, WEBHOOK)
        .header(TIMESTAMP_HEADER, timestamp.to_string())
        .header(SIGNATURE_HEADER, signature(key, WEBHOOK, timestamp, body))
        .body(Body::from(body.to_vec()))
        .unwrap()
}

fn signed(body: &[u8]) -> Request<Body> {
    delivery(body, Utc::now().timestamp(), SECRET.as_bytes())
}

fn edited(body: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(body).unwrap();
    edit(&mut value);
    serde_json::to_vec(&value).unwrap()
}

async fn answer(app: Router, request: Request<Body>) -> (StatusCode, Option<Value>) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = (!bytes.is_empty()).then(|| serde_json::from_slice(&bytes).unwrap());
    (status, body)
}

async fn refused(request: Request<Body>) {
    let outbox = Arc::new(Recording::default());
    let (status, body) = answer(app(outbox.clone(), products()), request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, None);
    assert!(outbox.sent().is_empty());
}

async fn ignored(body: &[u8], products: Products, reason: &str) {
    let outbox = Arc::new(Recording::default());
    let (status, answered) = answer(app(outbox.clone(), products), signed(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(answered, Some(json!({ "ignored": reason })));
    assert!(outbox.sent().is_empty());
}

async fn unprocessable(body: &[u8]) {
    let outbox = Arc::new(Recording::default());
    let (status, answered) = answer(app(outbox.clone(), products()), signed(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answered, None);
    assert!(outbox.sent().is_empty());
}

#[tokio::test]
async fn a_paid_order_sends_a_license_and_names_it() {
    let outbox = Arc::new(Recording::default());
    let (status, body) = answer(app(outbox.clone(), products()), signed(CREATE)).await;

    let period_end = "2027-11-18T11:59:58Z".parse().unwrap();
    let id = license_id::derive(SUBSCRIPTION, period_end, 5, Plan::Enterprise);
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, Some(json!({ "license": id.to_string() })));

    let sent = outbox.sent();
    assert_eq!(sent.len(), 1);
    let (recipient, letter) = &sent[0];
    assert_eq!(recipient, EMAIL);
    assert_eq!(letter.subject(), "Your Zone Enterprise license key");
    let license = verified(letter);
    assert_eq!(license.payload().id, id);
    assert_eq!(license.payload().customer, "Acme Ltd");
    assert_eq!(license.payload().seats, 5);
    assert_eq!(license.payload().plan, Some(Plan::Enterprise));
    assert_eq!(license.subscription(), Some(SUBSCRIPTION));
}

#[tokio::test]
async fn a_team_product_sends_a_team_license() {
    let outbox = Arc::new(Recording::default());
    let (status, body) = answer(
        app(outbox.clone(), listing(&format!("{PRODUCT}=team"))),
        signed(CREATE),
    )
    .await;

    let period_end = "2027-11-18T11:59:58Z".parse().unwrap();
    let id = license_id::derive(SUBSCRIPTION, period_end, 5, Plan::Team);
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, Some(json!({ "license": id.to_string() })));

    let sent = outbox.sent();
    assert_eq!(sent.len(), 1);
    let (_, letter) = &sent[0];
    assert_eq!(letter.subject(), "Your Zone Team license key");
    let license = verified(letter);
    assert_eq!(license.plan(), Plan::Team);
    assert_eq!(license.payload().id, id);
}

#[tokio::test]
async fn the_subscriptions_product_sets_the_plan_over_the_orders() {
    let outbox = Arc::new(Recording::default());
    let body = edited(CREATE, |value| {
        value["data"]["product_id"] = json!(TEAM_PRODUCT);
    });
    let products = listing(&format!("{PRODUCT},{TEAM_PRODUCT}=team"));
    let (status, _) = answer(app(outbox.clone(), products), signed(&body)).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, letter) = &outbox.sent()[0];
    assert_eq!(letter.subject(), "Your Zone Enterprise license key");
    assert_eq!(verified(letter).plan(), Plan::Enterprise);
}

#[tokio::test]
async fn an_unlisted_subscription_product_falls_back_to_the_orders_plan() {
    let outbox = Arc::new(Recording::default());
    let body = edited(CREATE, |value| {
        value["data"]["product_id"] = json!(TEAM_PRODUCT);
    });
    let (status, _) = answer(
        app(outbox.clone(), listing(&format!("{TEAM_PRODUCT}=team"))),
        signed(&body),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, letter) = &outbox.sent()[0];
    assert_eq!(letter.subject(), "Your Zone Team license key");
    assert_eq!(verified(letter).plan(), Plan::Team);
}

#[tokio::test]
async fn the_license_and_the_letter_carry_the_same_normalised_name() {
    let outbox = Arc::new(Recording::default());
    let body = edited(CREATE, |value| {
        value["data"]["billing_name"] = json!(" Acme\u{1b}[31m\u{200b} \r\n Ltd ");
    });
    let (status, _) = answer(app(outbox.clone(), products()), signed(&body)).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (_, letter) = &outbox.sent()[0];
    let license = verified(letter);
    assert_eq!(license.payload().customer, "Acme Ltd");
    assert!(
        letter.text().contains("Customer: Acme Ltd\n"),
        "{}",
        letter.text()
    );
}

#[tokio::test]
async fn a_wrong_signature_is_refused() {
    refused(delivery(
        CREATE,
        Utc::now().timestamp(),
        b"whsec_another-secret",
    ))
    .await;
}

#[tokio::test]
async fn a_stale_timestamp_is_refused() {
    refused(delivery(
        CREATE,
        Utc::now().timestamp() - 600,
        SECRET.as_bytes(),
    ))
    .await;
}

#[tokio::test]
async fn a_body_changed_after_signing_is_refused() {
    let mut request = signed(CREATE);
    *request.body_mut() = Body::from(edited(CREATE, |value| value["data"]["seats"] = json!(500)));
    refused(request).await;
}

#[tokio::test]
async fn an_unsigned_delivery_is_not_a_warning() {
    let logged = logging::capture(
        tracing::Level::WARN,
        refused(delivery(
            CREATE,
            Utc::now().timestamp(),
            b"whsec_another-secret",
        )),
    )
    .await;
    assert_eq!(logged, "");
}

#[tokio::test]
async fn missing_signature_headers_are_refused() {
    let request = Request::builder()
        .method(Method::POST)
        .uri(WEBHOOK_PATH)
        .body(Body::from(CREATE.to_vec()))
        .unwrap();
    refused(request).await;
    for missing in [ID_HEADER, TIMESTAMP_HEADER, SIGNATURE_HEADER] {
        let mut request = signed(CREATE);
        request.headers_mut().remove(missing);
        refused(request).await;
    }
}

#[tokio::test]
async fn another_event_type_is_ignored() {
    ignored(SUBSCRIPTION_UPDATED, products(), "event_type").await;
}

#[tokio::test]
async fn a_one_off_purchase_is_ignored() {
    ignored(PURCHASE, products(), "billing_reason").await;
}

#[tokio::test]
async fn an_unpaid_order_is_ignored() {
    ignored(UNPAID, products(), "unpaid").await;
}

#[tokio::test]
async fn an_unlisted_product_is_ignored() {
    ignored(CREATE, listing("another-product"), "product").await;
}

#[tokio::test]
async fn a_listed_subscription_product_does_not_issue_for_an_unlisted_order() {
    let body = edited(CREATE, |value| {
        value["data"]["product_id"] = json!("another-product");
    });
    ignored(&body, products(), "product").await;
}

#[tokio::test]
async fn an_order_without_an_email_is_unprocessable() {
    unprocessable(NO_EMAIL).await;
}

#[tokio::test]
async fn a_listed_product_without_seats_is_unprocessable() {
    unprocessable(NO_SEATS).await;
}

#[tokio::test]
async fn malformed_json_is_unprocessable() {
    unprocessable(b"{\"type\":\"order.paid\",").await;
    unprocessable(b"not json").await;
}

#[tokio::test]
async fn an_order_the_signer_refuses_is_unprocessable() {
    let empty_period = edited(CREATE, |value| {
        let start = value["data"]["subscription"]["current_period_start"].clone();
        value["data"]["subscription"]["current_period_end"] = start;
    });
    unprocessable(&empty_period).await;
}

#[tokio::test]
async fn a_failed_send_is_unavailable_so_polar_retries() {
    let (status, body) = answer(app(Arc::new(Failing), products()), signed(CREATE)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, None);
}

#[tokio::test]
async fn a_send_that_can_never_succeed_is_unprocessable() {
    let (status, body) = answer(app(Arc::new(Unsendable), products()), signed(CREATE)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, None);
}

#[tokio::test]
async fn health_answers_ok() {
    let request = Request::builder()
        .uri(HEALTH_PATH)
        .body(Body::empty())
        .unwrap();
    let (status, body) = answer(app(Arc::new(Failing), products()), request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, Some(json!({ "status": "ok" })));
}

#[tokio::test]
async fn the_webhook_takes_only_post() {
    let request = Request::builder()
        .uri(WEBHOOK_PATH)
        .body(Body::empty())
        .unwrap();
    let (status, _) = answer(app(Arc::new(Failing), products()), request).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}
