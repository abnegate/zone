mod support;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_enterprise::{Keyring, License, TrustedKey};
use zone_license::http::{Context, HEALTH_PATH, Server, WEBHOOK_PATH, router};
use zone_license::mail::Relay;
use zone_license::polar::{ID_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER, signature};
use zone_license::{Config, PrivateKey, ServeError, Variable};

use crate::support::smtp::{Delivered, SENDER, SmtpSink};

const CREATE: &[u8] = include_bytes!("fixtures/polar/order_paid_subscription_create.json");
const CYCLE: &[u8] = include_bytes!("fixtures/polar/order_paid_subscription_cycle.json");
const UPDATE: &[u8] = include_bytes!("fixtures/polar/order_paid_subscription_update.json");

const SECRET: &str = "whsec_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const SECRET_KEY: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];
const KEY_ID: &str = "test-serve-1";
const WEBHOOK: &str = "msg_2kWm9Q4tYpS3vB7xN1cR8dLe";
const EMAIL: &str = "billing@acme.test";
const CUSTOMER: &str = "Acme Ltd";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";

struct Service {
    address: SocketAddr,
    keyring: Keyring,
    client: reqwest::Client,
    stop: oneshot::Sender<()>,
    task: JoinHandle<Result<(), ServeError>>,
}

impl Service {
    async fn start(sink: &SmtpSink) -> Self {
        let mut seed = [0; 32];
        getrandom::fill(&mut seed).unwrap();
        let key = PrivateKey::new(seed);
        let keyring = Keyring::new([TrustedKey::new(KEY_ID, key.public()).unwrap()]).unwrap();
        let variables = HashMap::from([
            (Variable::WebhookSecret, SECRET.to_owned()),
            (Variable::Products, PRODUCT.to_owned()),
            (Variable::PrivateKey, key.encode()),
            (Variable::KeyId, KEY_ID.to_owned()),
            (Variable::Listen, "127.0.0.1:0".to_owned()),
            (Variable::SmtpHost, sink.address().ip().to_string()),
            (Variable::SmtpPort, sink.address().port().to_string()),
            (Variable::SmtpUser, "unused".to_owned()),
            (Variable::SmtpPassword, String::new()),
            (Variable::SmtpFrom, SENDER.to_owned()),
        ]);
        let config =
            Config::from_variables(|variable| variables.get(&variable).cloned(), &keyring).unwrap();
        let relay = Relay::new(&sink.config()).unwrap();
        let context = Context::new(
            config.signer,
            config.secret,
            config.products,
            Arc::new(relay),
        );
        let server = Server::bind(config.listen, router(context)).await.unwrap();
        let address = server.address().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(server.run(async {
            let _ = stopped.await;
        }));
        Self {
            address,
            keyring,
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            stop,
            task,
        }
    }

    async fn deliver(&self, body: &[u8], timestamp: i64) -> (StatusCode, Value) {
        let response = self
            .client
            .post(format!("http://{}{WEBHOOK_PATH}", self.address))
            .header(ID_HEADER, WEBHOOK)
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(
                SIGNATURE_HEADER,
                signature(&SECRET_KEY, WEBHOOK, timestamp, body),
            )
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap()
        };
        (status, body)
    }

    fn license(&self, message: &Delivered) -> (String, License) {
        let text = message.part("text/plain").expect("a text part");
        text.lines()
            .find_map(|line| {
                self.keyring
                    .verify(line.trim())
                    .ok()
                    .map(|license| (line.trim().to_owned(), license))
            })
            .expect("the text part carries a license that verifies")
    }

    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

fn now() -> i64 {
    Utc::now().timestamp()
}

fn subscription(body: &[u8]) -> Value {
    let event: Value = serde_json::from_slice(body).unwrap();
    event["data"]["subscription"].clone()
}

fn instant(value: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value.as_str().unwrap())
        .unwrap()
        .to_utc()
}

fn license_id(answer: &Value) -> Uuid {
    answer["license"].as_str().unwrap().parse().unwrap()
}

#[tokio::test]
async fn each_issuing_order_emails_a_license_for_its_period_and_a_retry_resends_the_same_key() {
    for (body, seats) in [(CREATE, 5), (CYCLE, 5), (UPDATE, 8)] {
        let sink = SmtpSink::start().await;
        let service = Service::start(&sink).await;

        let (status, answer) = service.deliver(body, now()).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        let messages = sink.messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].recipients, [EMAIL]);

        let (key, license) = service.license(&messages[0]);
        let payload = license.payload();
        let period = subscription(body);
        assert_eq!(payload.id, license_id(&answer));
        assert_eq!(payload.customer, CUSTOMER);
        assert_eq!(payload.seats, seats);
        assert_eq!(payload.issued_at, instant(&period["current_period_start"]));
        assert_eq!(payload.expires_at, instant(&period["current_period_end"]));
        assert_eq!(payload.key_id, KEY_ID);

        let (status, retried) = service.deliver(body, now() - 90).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{retried}");
        assert_eq!(retried, answer);
        let messages = sink.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(service.license(&messages[1]).0, key);

        service.stop().await;
    }
}

#[tokio::test]
async fn a_seat_change_in_the_same_period_gets_a_new_license() {
    let sink = SmtpSink::start().await;
    let service = Service::start(&sink).await;

    let (_, created) = service.deliver(CREATE, now()).await;
    let (_, updated) = service.deliver(UPDATE, now()).await;

    assert_eq!(
        subscription(CREATE)["current_period_end"],
        subscription(UPDATE)["current_period_end"]
    );
    assert_ne!(license_id(&created), license_id(&updated));
    let messages = sink.messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(service.license(&messages[0]).1.payload().seats, 5);
    assert_eq!(service.license(&messages[1]).1.payload().seats, 8);
    service.stop().await;
}

#[tokio::test]
async fn a_relay_that_refuses_the_message_gets_polar_to_retry() {
    for reply in ["451 4.3.0 try again later", "550 5.1.1 no such user"] {
        let sink = SmtpSink::rejecting(reply).await;
        let service = Service::start(&sink).await;

        let (status, answer) = service.deliver(CREATE, now()).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
        assert_eq!(answer, Value::Null);
        assert!(sink.messages().is_empty());
        service.stop().await;
    }
}

#[tokio::test]
async fn a_forged_delivery_sends_nothing() {
    let sink = SmtpSink::start().await;
    let service = Service::start(&sink).await;

    let response = service
        .client
        .post(format!("http://{}{WEBHOOK_PATH}", service.address))
        .header(ID_HEADER, WEBHOOK)
        .header(TIMESTAMP_HEADER, now().to_string())
        .header(
            SIGNATURE_HEADER,
            signature(SECRET.as_bytes(), "msg_other", now(), CREATE),
        )
        .body(CREATE.to_vec())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(sink.messages().is_empty());
    service.stop().await;
}

#[tokio::test]
async fn health_answers_over_http() {
    let sink = SmtpSink::start().await;
    let service = Service::start(&sink).await;

    let response = service
        .client
        .get(format!("http://{}{HEALTH_PATH}", service.address))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({ "status": "ok" })
    );
    service.stop().await;
}
