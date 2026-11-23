//! The single sign-on throttle in a real `zone-server` process behind a trusted
//! proxy. The process must hand each request its TCP peer, or it cannot tell a
//! trusted proxy from a client and every forwarded client shares one count.

use reqwest::{Response, StatusCode};
use serde_json::{Value, json};
use sqlx::ConnectOptions;
use uuid::Uuid;
use zone_enterprise::sso::RATE_LIMITED;

use crate::common::Database;
use crate::common::replica::{Environment, Replica};
use crate::common::stub_litellm::StubLitellm;
use crate::shared_limiter::start_of_a_window;
use crate::sso_throttle::LIMIT;

const PREFIX: &str = "sso_throttle_forwarded";
const TRUSTED_PROXIES: &str = "ZONE_TRUSTED_PROXIES";
const REQUESTS_PER_MINUTE: &str = "ZONE_SSO_REQUESTS_PER_MINUTE";
const LOOPBACK: &str = "127.0.0.1/32";
const FORWARDED_FOR: &str = "x-forwarded-for";
const CLIENT: &str = "203.0.113.1";
const NEIGHBOUR: &str = "203.0.113.2";

async fn authorize(replica: &Replica, client: &str) -> Response {
    reqwest::Client::new()
        .post(replica.url(&format!(
            "/api/auth/sso/providers/{}/authorizations",
            Uuid::new_v4()
        )))
        .header(FORWARDED_FOR, client)
        .json(&json!({}))
        .send()
        .await
        .expect("the replica answers")
}

async fn allowed(replica: &Replica, client: &str) {
    let response = authorize(replica, client).await;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    assert_ne!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "{client} is limited before its own limit: {body}"
    );
}

async fn limited(replica: &Replica, client: &str) {
    let response = authorize(replica, client).await;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], RATE_LIMITED, "{body}");
}

#[tokio::test]
async fn a_server_behind_a_trusted_proxy_limits_each_forwarded_client_on_its_own() {
    let database = Database::new(PREFIX).await;
    let litellm = StubLitellm::start().await;
    let environment = Environment::new(
        database.pool.connect_options().to_url_lossy().to_string(),
        litellm.url(),
    )
    .with_variable(TRUSTED_PROXIES, LOOPBACK)
    .with_variable(REQUESTS_PER_MINUTE, LIMIT.to_string());
    let replica = Replica::start(&environment).await;
    start_of_a_window().await;

    for _ in 0..LIMIT {
        allowed(&replica, CLIENT).await;
    }
    limited(&replica, CLIENT).await;

    allowed(&replica, NEIGHBOUR).await;
}
