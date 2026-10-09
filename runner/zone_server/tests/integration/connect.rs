use crate::common::{TestClient, test_config, test_email, test_password};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;

async fn access_token(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["access_token"]
        .as_str()
        .expect("access_token")
        .to_string()
}

#[tokio::test]
async fn connect_requires_auth() {
    let client = TestClient::with_db().await;
    client
        .get("/api/connect")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn connect_lists_configured_urls() {
    let mut config = test_config();
    config.connect_urls = vec!["http://192.168.1.10".into(), "http://100.64.1.2".into()];
    let client = TestClient::with_config(config).await;
    let token = access_token(&client).await;
    let response = client.get_auth("/api/connect", &token).await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["urls"],
        json!(["http://192.168.1.10", "http://100.64.1.2"])
    );
}

#[tokio::test]
async fn connect_advertises_a_forwarded_lan_host() {
    let client = TestClient::with_db().await;
    let token = access_token(&client).await;
    let response = client
        .send_request(
            Request::builder()
                .method("GET")
                .uri("/api/connect")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-forwarded-proto", "http")
                .header("x-forwarded-host", "192.168.1.10")
                .header("host", "manager.localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["urls"],
        json!(["http://192.168.1.10"])
    );
}
