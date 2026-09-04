//! Model routes integration tests with mock Ollama server
//!
//! Tests the /api/models endpoints using a mock Ollama server to simulate
//! the external dependency.

mod common;

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::{delete, get, post},
};
use http_body_util::BodyExt;
use serde_json::json;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::OnceCell;
use tower::ServiceExt;

// =============================================================================
// Mock Ollama Server
// =============================================================================

/// Create a mock Ollama server that simulates the Ollama API
fn create_mock_ollama_router() -> Router {
    Router::new()
        .route("/api/tags", get(mock_ollama_tags))
        .route("/api/show", post(mock_ollama_show))
        .route("/api/delete", delete(mock_ollama_delete))
}

/// Mock handler for GET /api/tags
async fn mock_ollama_tags() -> Json<serde_json::Value> {
    Json(json!({
        "models": [
            {
                "name": "llama2:latest",
                "size": 3825819519_u64,
                "digest": "sha256:abc123",
                "modified_at": "2024-01-15T10:30:00Z",
                "details": {
                    "format": "gguf",
                    "family": "llama",
                    "parameter_size": "7B",
                    "quantization_level": "Q4_0"
                }
            },
            {
                "name": "mistral:latest",
                "size": 4109854720_u64,
                "digest": "sha256:def456",
                "modified_at": "2024-01-14T09:00:00Z",
                "details": {
                    "format": "gguf",
                    "family": "mistral",
                    "parameter_size": "7B",
                    "quantization_level": "Q4_K_M"
                }
            }
        ]
    }))
}

/// Mock handler for POST /api/show
async fn mock_ollama_show(
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let name = payload["name"].as_str().unwrap_or("");

    match name {
        "llama2:latest" | "llama2" => Ok(Json(json!({
            "modelfile": "FROM llama2",
            "parameters": "temperature 0.7",
            "template": "{{ .Prompt }}",
            "details": {
                "format": "gguf",
                "family": "llama",
                "parameter_size": "7B",
                "quantization_level": "Q4_0"
            }
        }))),
        "mistral:latest" | "mistral" => Ok(Json(json!({
            "modelfile": "FROM mistral",
            "parameters": "temperature 0.8",
            "template": "{{ .Prompt }}",
            "details": {
                "format": "gguf",
                "family": "mistral",
                "parameter_size": "7B",
                "quantization_level": "Q4_K_M"
            }
        }))),
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("model '{}' not found", name)})),
        )),
    }
}

/// Mock handler for DELETE /api/delete
async fn mock_ollama_delete(
    Json(payload): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let name = payload["name"].as_str().unwrap_or("");

    match name {
        "llama2:latest" | "llama2" | "mistral:latest" | "mistral" => Ok(StatusCode::OK),
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("model '{}' not found", name)})),
        )),
    }
}

/// Start a mock Ollama server and return its address
async fn start_mock_ollama_server() -> SocketAddr {
    let router = create_mock_ollama_router();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    addr
}

/// Create a mock Ollama server that returns errors
fn create_error_ollama_router() -> Router {
    Router::new()
        .route("/api/tags", get(mock_ollama_tags_error))
        .route("/api/show", post(mock_ollama_show_error))
        .route("/api/delete", delete(mock_ollama_delete_error))
}

async fn mock_ollama_tags_error() -> (StatusCode, &'static str) {
    (StatusCode::INTERNAL_SERVER_ERROR, "Service unavailable")
}

async fn mock_ollama_show_error() -> (StatusCode, &'static str) {
    (StatusCode::INTERNAL_SERVER_ERROR, "Service unavailable")
}

async fn mock_ollama_delete_error() -> (StatusCode, &'static str) {
    (StatusCode::INTERNAL_SERVER_ERROR, "Service unavailable")
}

async fn start_error_ollama_server() -> SocketAddr {
    let router = create_error_ollama_router();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

/// Create a mock server that returns invalid JSON
fn create_invalid_json_ollama_router() -> Router {
    Router::new()
        .route("/api/tags", get(mock_ollama_tags_invalid_json))
        .route("/api/show", post(mock_ollama_show_invalid_json))
}

async fn mock_ollama_tags_invalid_json() -> &'static str {
    "not valid json {"
}

async fn mock_ollama_show_invalid_json() -> &'static str {
    "not valid json {"
}

async fn start_invalid_json_ollama_server() -> SocketAddr {
    let router = create_invalid_json_ollama_router();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

fn mock_gpt4all_catalog() -> Json<serde_json::Value> {
    Json(json!([{
        "name": "Llama 3 Instruct",
        "filename": "llama-3-8b-instruct.Q4_0.gguf",
        "filesize": "4000000000",
        "parameters": "8B",
        "type": "LLaMA",
        "description": "A compact Llama 3 chat model",
        "quant": "q4_0"
    }]))
}

async fn start_gpt4all_catalog_server() -> String {
    let router = Router::new().route("/models3.json", get(|| async { mock_gpt4all_catalog() }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/models3.json");

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    for _ in 0..50 {
        if client
            .get(&url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return url;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("GPT4All catalog mock did not become ready at {url}");
}

static GPT4ALL_MOCK_URL: OnceCell<String> = OnceCell::const_new();

async fn local_gpt4all_catalog_url() -> String {
    GPT4ALL_MOCK_URL
        .get_or_init(|| async { start_gpt4all_catalog_server().await })
        .await
        .clone()
}

// =============================================================================
// Test Helpers
// =============================================================================

/// Get an auth token for testing
async fn get_auth_token(router: &Router) -> String {
    let email = common::test_email();
    let password = common::test_password();

    // Register
    let body = serde_json::to_string(&json!({
        "email": &email,
        "password": &password,
        "display_name": "Model Tester"
    }))
    .unwrap();

    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["access_token"].as_str().unwrap().to_string()
}

/// Create a test router with custom Ollama host
async fn create_test_router_with_ollama(ollama_host: &str) -> Router {
    create_test_router_with_hosts(ollama_host, None).await
}

async fn create_test_router_with_hosts(
    ollama_host: &str,
    gpt4all_models_url: Option<&str>,
) -> Router {
    let mut config = common::test_config_with_ollama_host(ollama_host);
    if let Some(url) = gpt4all_models_url {
        config.gpt4all_models_url = url.to_string();
    }
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool);
    common::create_test_router(state)
}

// =============================================================================
// List Models Tests
// =============================================================================

#[tokio::test]
async fn test_list_models_ollama_success() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let models: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["name"], "llama2:latest");
    assert_eq!(models[1]["name"], "mistral:latest");
    assert!(models[0]["size"].as_u64().is_some());
    assert!(models[0]["digest"].as_str().is_some());
    assert!(models[0]["details"]["family"].as_str().is_some());
}

#[tokio::test]
async fn test_list_models_ollama_with_source_param() {
    // When source=ollama is explicitly provided, it enters "browse" mode
    // which uses the OllamaLibraryProvider and returns a BrowseResponse
    let router = create_test_router_with_ollama("http://localhost:9999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=ollama")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Browse mode returns a BrowseResponse with "models" array
    assert!(result["models"].is_array());
}

#[tokio::test]
async fn test_list_models_huggingface() {
    let router = create_test_router_with_ollama("http://localhost:9999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=huggingface")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Browse mode returns a BrowseResponse with "models" array
    let models = result["models"].as_array().expect("models should be array");

    // Should return HuggingFace models
    assert!(!models.is_empty());
}

#[tokio::test]
async fn test_list_models_huggingface_with_search() {
    let router = create_test_router_with_ollama("http://localhost:9999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=huggingface&search=llama")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_models_gpt4all() {
    let catalog = local_gpt4all_catalog_url().await;
    let router = create_test_router_with_hosts("http://localhost:9999", Some(&catalog)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=gpt4all")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Browse mode returns a BrowseResponse with "models" array
    let models = result["models"].as_array().expect("models should be array");

    // Should return GPT4All models
    assert!(!models.is_empty());
}

#[tokio::test]
async fn test_list_models_gpt4all_with_search() {
    let catalog = local_gpt4all_catalog_url().await;
    let router = create_test_router_with_hosts("http://localhost:9999", Some(&catalog)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=gpt4all&search=llama")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_models_accepts_sort_and_filter_params() {
    let catalog = local_gpt4all_catalog_url().await;
    let router = create_test_router_with_hosts("http://localhost:9999", Some(&catalog)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=gpt4all&sort=name_asc&family=llama&size=medium")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(result["models"].is_array());
}

#[tokio::test]
async fn test_list_models_unknown_source() {
    let router = create_test_router_with_ollama("http://localhost:9999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models?source=unknown")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"].as_str().unwrap().contains("Unknown source"));
}

#[tokio::test]
async fn test_list_models_unauthorized() {
    let router = create_test_router_with_ollama("http://localhost:9999").await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_list_models_ollama_connection_error() {
    // Use a port that's not listening - test without source param to use local Ollama
    let router = create_test_router_with_ollama("http://127.0.0.1:59999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("Failed to connect")
    );
}

#[tokio::test]
async fn test_list_models_ollama_service_unavailable() {
    // Test without source param to use local Ollama
    let ollama_addr = start_error_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_list_models_ollama_invalid_json() {
    // Test without source param to use local Ollama
    let ollama_addr = start_invalid_json_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"].as_str().unwrap().contains("Failed to parse"));
}

// =============================================================================
// Get Model Tests
// =============================================================================

#[tokio::test]
async fn test_get_model_success() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/llama2:latest")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let model: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(model["modelfile"].as_str().is_some());
    assert!(model["details"]["family"].as_str().is_some());
}

#[tokio::test]
async fn test_get_model_not_found() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/nonexistent-model")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_get_model_unauthorized() {
    let router = create_test_router_with_ollama("http://localhost:9999").await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/llama2")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_get_model_connection_error() {
    let router = create_test_router_with_ollama("http://127.0.0.1:59999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/llama2")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("Failed to connect")
    );
}

#[tokio::test]
async fn test_get_model_service_error() {
    let ollama_addr = start_error_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/llama2")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_get_model_invalid_json_response() {
    let ollama_addr = start_invalid_json_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/llama2")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"].as_str().unwrap().contains("Failed to parse"));
}

// =============================================================================
// Delete Model Tests
// =============================================================================

#[tokio::test]
async fn test_delete_model_success() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/models/llama2:latest")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_delete_model_not_found() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/models/nonexistent-model")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_delete_model_unauthorized() {
    let router = create_test_router_with_ollama("http://localhost:9999").await;

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/models/llama2")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_delete_model_connection_error() {
    let router = create_test_router_with_ollama("http://127.0.0.1:59999").await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/models/llama2")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("Failed to connect")
    );
}

#[tokio::test]
async fn test_delete_model_service_error() {
    let ollama_addr = start_error_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/models/llama2")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

// =============================================================================
// Model Details Tests
// =============================================================================

#[tokio::test]
async fn test_model_details_populated() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let models: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    // Check that details are populated
    let model = &models[0];
    assert_eq!(model["details"]["format"], "gguf");
    assert_eq!(model["details"]["family"], "llama");
    assert_eq!(model["details"]["parameter_size"], "7B");
    assert_eq!(model["details"]["quantization_level"], "Q4_0");
}

#[tokio::test]
async fn test_get_model_details() {
    let ollama_addr = start_mock_ollama_server().await;
    let router = create_test_router_with_ollama(&format!("http://{}", ollama_addr)).await;
    let token = get_auth_token(&router).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/models/mistral")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let model: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(model["details"]["family"], "mistral");
    assert_eq!(model["details"]["quantization_level"], "Q4_K_M");
}
