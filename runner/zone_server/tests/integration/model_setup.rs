//! GET/POST /api/models/setup
use crate::common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn token(router: &axum::Router) -> String {
    let email = common::test_email();
    let body = serde_json::to_string(&json!({
        "email": email,
        "password": common::test_password(),
        "display_name": "Setup Tester"
    }))
    .unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    json["access_token"].as_str().unwrap().to_string()
}

async fn router() -> axum::Router {
    let config = common::test_config_with_ollama_host("http://127.0.0.1:1");
    let pool = common::create_test_pool().await;
    common::create_test_router(common::create_test_state(config, pool))
}

#[tokio::test]
async fn setup_requires_auth() {
    let router = router().await;
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/setup")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_plan_lists_chat_and_disk_totals() {
    let router = router().await;
    let token = token(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/setup?features=chat&chat_preset=8gb")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["chat_preset"], "8gb");
    assert_eq!(body["wants_all"], false);
    let features = body["features"].as_array().unwrap();
    let chat = features
        .iter()
        .find(|feature| feature["id"] == "chat")
        .unwrap();
    assert_eq!(chat["required"], true);
    assert_eq!(chat["selected"], true);
    assert!(
        body["totals"]["required_free_label"]
            .as_str()
            .unwrap()
            .contains("GB")
    );
    assert!(
        body["pulls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|pull| pull["runtime"] == "ollama")
    );
}

#[tokio::test]
async fn setup_rejects_unknown_features() {
    let router = router().await;
    let token = token(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/setup?features=nope")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn setup_post_chat_on_8gb_is_allowed() {
    let router = router().await;
    let token = token(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/setup")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    json!({
                        "features": ["chat"],
                        "chat_preset": "8gb"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    if status == StatusCode::CONFLICT {
        assert_eq!(body["code"], "disk");
        return;
    }
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["gate"], Value::Null);
    assert_eq!(body["chat_preset"], "8gb");
    assert!(
        body["pulls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pull| pull["model"] == "llama3.2:3b")
    );
}

#[tokio::test]
async fn setup_post_all_on_tiny_ram_is_conflict() {
    unsafe {
        std::env::set_var("ZONE_SETUP_RAM_BYTES", "8589934592");
        std::env::set_var("ZONE_SETUP_DISK_FREE_BYTES", "500000000000");
    }
    let router = router().await;
    let token = token(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/setup")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({ "features": ["all"] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    unsafe {
        std::env::remove_var("ZONE_SETUP_RAM_BYTES");
        std::env::remove_var("ZONE_SETUP_DISK_FREE_BYTES");
    }
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["code"], "all-ram");
    assert!(body["error"].as_str().unwrap().contains("16 GB"));
}
