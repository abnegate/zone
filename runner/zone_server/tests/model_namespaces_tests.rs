mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

#[tokio::test]
async fn encoded_model_names_reach_ollama_unchanged() {
    let provider = MockServer::start().await;
    let name = "hf.co/owner/repository:Q4_K_M";
    Mock::given(method("POST"))
        .and(path("/api/show"))
        .and(body_json(json!({"name": name})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"content": name})))
        .expect(1)
        .mount(&provider)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/delete"))
        .and(body_json(json!({"name": name})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&provider)
        .await;
    let router = common::create_test_router(common::create_test_state(
        common::test_config_with_ollama_host(&provider.uri()),
        common::create_test_pool().await,
    ));
    let registration = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"email": common::test_email(), "password": common::test_password()}).to_string(),
        ))
        .unwrap();
    let response = router.clone().oneshot(registration).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let registration: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = registration["access_token"].as_str().unwrap();

    for (operation, endpoint, expected) in [
        (
            "GET",
            "/api/models/hf.co/owner/repository:Q4_K_M",
            StatusCode::NOT_FOUND,
        ),
        (
            "GET",
            "/api/models/hf.co%2Fowner%2Frepository%3AQ4_K_M",
            StatusCode::OK,
        ),
        (
            "DELETE",
            "/api/models/hf.co%2Fowner%2Frepository%3AQ4_K_M",
            StatusCode::NO_CONTENT,
        ),
    ] {
        let request = Request::builder()
            .method(operation)
            .uri(endpoint)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "{operation} {endpoint}");
        if operation == "GET" && expected == StatusCode::OK {
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let details: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(details["content"], name);
        }
    }
    provider.verify().await;
}
