mod context;
mod health;
mod issued;
mod outcome;
mod server;
mod skipped;
mod webhook;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

pub use context::Context;
pub use health::{Health, OK};
pub use issued::Issued;
pub use outcome::Outcome;
pub use server::Server;
pub use skipped::Skipped;

pub const WEBHOOK_PATH: &str = "/webhooks/polar";
pub const HEALTH_PATH: &str = "/health";

/// Axum's default body limit of 2 MB applies, far above any Polar event.
pub fn router(context: Context) -> Router {
    Router::new()
        .route(WEBHOOK_PATH, post(webhook::receive))
        .route(HEALTH_PATH, get(health::check))
        .with_state(Arc::new(context))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use chrono::Utc;
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;
    use zone_enterprise::{Keyring, TrustedKey};

    use super::*;
    use crate::license_id;
    use crate::mail::{Letter, MailError, Outbox};
    use crate::polar::{ID_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER, WebhookSecret, signature};
    use crate::private_key::PrivateKey;
    use crate::signer::Signer;

    const CREATE: &[u8] =
        include_bytes!("../../tests/fixtures/polar/order_paid_subscription_create.json");
    const PURCHASE: &[u8] = include_bytes!("../../tests/fixtures/polar/order_paid_purchase.json");
    const UNPAID: &[u8] = include_bytes!("../../tests/fixtures/polar/order_paid_unpaid.json");
    const NO_EMAIL: &[u8] = include_bytes!("../../tests/fixtures/polar/order_paid_no_email.json");
    const NO_SEATS: &[u8] = include_bytes!("../../tests/fixtures/polar/order_paid_no_seats.json");
    const SUBSCRIPTION_UPDATED: &[u8] =
        include_bytes!("../../tests/fixtures/polar/subscription_updated.json");

    const SECRET: &str = "whsec_router-test-secret";
    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [5; 32];
    const WEBHOOK: &str = "msg_router";
    const SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
    const EMAIL: &str = "billing@acme.test";
    const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";

    #[derive(Default)]
    struct Recording {
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
        fn sent(&self) -> Vec<(String, Letter)> {
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

    fn products() -> Vec<String> {
        vec![PRODUCT.to_owned()]
    }

    fn signer() -> Signer {
        Signer::new(PrivateKey::new(SEED), KEY_ID)
    }

    fn keyring() -> Keyring {
        Keyring::new([TrustedKey::new(KEY_ID, signer().public()).unwrap()]).unwrap()
    }

    fn app(outbox: Arc<dyn Outbox>, products: Vec<String>) -> Router {
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

    async fn ignored(body: &[u8], products: Vec<String>, reason: &str) {
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
        let id = license_id::derive(SUBSCRIPTION, period_end, 5);
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body, Some(json!({ "license": id.to_string() })));

        let sent = outbox.sent();
        assert_eq!(sent.len(), 1);
        let (recipient, letter) = &sent[0];
        assert_eq!(recipient, EMAIL);
        let license = letter
            .text()
            .lines()
            .find_map(|line| keyring().verify(line).ok())
            .expect("the letter carries a license that verifies");
        assert_eq!(license.payload().id, id);
        assert_eq!(license.payload().customer, "Acme Ltd");
        assert_eq!(license.payload().seats, 5);
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
        let license = letter
            .text()
            .lines()
            .find_map(|line| keyring().verify(line).ok())
            .expect("the letter carries a license that verifies");
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
        *request.body_mut() =
            Body::from(edited(CREATE, |value| value["data"]["seats"] = json!(500)));
        refused(request).await;
    }

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn an_unsigned_delivery_is_not_a_warning() {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        refused(delivery(
            CREATE,
            Utc::now().timestamp(),
            b"whsec_another-secret",
        ))
        .await;

        let logged = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
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
        ignored(CREATE, vec!["another-product".to_owned()], "product").await;
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
}
