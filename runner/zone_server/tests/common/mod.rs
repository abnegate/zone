//! Test utilities for zone_server integration tests
//!
//! Provides helpers for creating test configurations, database connections,
//! and HTTP test clients.

#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tower::ServiceExt;

use zone_server::config::Config;
use zone_server::routes::create_router;
use zone_server::state::AppState;

/// Send server tracing to the test's stdout. Without this the server's own
/// logs vanish and a failing integration test says nothing about why.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

/// Test configuration with sensible defaults
pub fn test_config() -> Config {
    Config {
        host: "127.0.0.1".to_string(),
        port: 0, // Random port
        database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/zone_test".to_string()
        }),
        redis_url: std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        jwt_secret: "test-secret-key-must-be-at-least-32-chars-long".to_string(),
        jwt_access_lifetime: 900,
        jwt_refresh_lifetime: 604800,
        litellm_host: std::env::var("LITELLM_HOST")
            .unwrap_or_else(|_| "http://localhost:4000".to_string()),
        litellm_key: std::env::var("LITELLM_KEY").unwrap_or_else(|_| "test-key".to_string()),
        ollama_host: std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| "http://localhost:11434".to_string()),
        gpt4all_models_url: zone_server::config::DEFAULT_GPT4ALL_MODELS_URL.to_string(),
        huggingface_models_url: zone_server::config::DEFAULT_HUGGINGFACE_MODELS_URL.to_string(),
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        web_search: Default::default(),
        comfyui: Default::default(),
    }
}

/// Create a database pool for testing
pub async fn create_test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string());

    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Create an AppState for testing (without cache)
pub fn create_test_state(config: Config, pool: PgPool) -> AppState {
    AppState::new(config, pool, None)
}

/// Create a test router with the given state
pub fn create_test_router(state: AppState) -> Router {
    create_router(state)
}

/// Test client for making HTTP requests to the test router
pub struct TestClient {
    router: Router,
}

impl TestClient {
    /// Create a new test client
    pub fn new(router: Router) -> Self {
        Self { router }
    }

    /// Create a test client with a database connection
    pub async fn with_db() -> Self {
        let config = test_config();
        let pool = create_test_pool().await;
        let state = create_test_state(config, pool);
        let router = create_test_router(state);
        Self::new(router)
    }

    /// Make a GET request
    pub async fn get(&self, uri: &str) -> TestResponse {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();

        self.send(request).await
    }

    /// Make a GET request with authorization
    pub async fn get_auth(&self, uri: &str, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        self.send(request).await
    }

    /// Make a POST request with JSON body
    pub async fn post_json(&self, uri: &str, body: &Value) -> TestResponse {
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(body).unwrap()))
            .unwrap();

        self.send(request).await
    }

    /// Make a POST request with JSON body and authorization
    pub async fn post_json_auth(&self, uri: &str, body: &Value, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(serde_json::to_string(body).unwrap()))
            .unwrap();

        self.send(request).await
    }

    /// Make a PUT request with JSON body and authorization
    pub async fn put_json_auth(&self, uri: &str, body: &Value, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("PUT")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(serde_json::to_string(body).unwrap()))
            .unwrap();

        self.send(request).await
    }

    /// Make a PATCH request with JSON body and authorization
    pub async fn patch_json_auth(&self, uri: &str, body: &Value, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("PATCH")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::from(serde_json::to_string(body).unwrap()))
            .unwrap();

        self.send(request).await
    }

    /// Make a DELETE request with authorization
    pub async fn delete_auth(&self, uri: &str, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("DELETE")
            .uri(uri)
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        self.send(request).await
    }

    /// Send a request and get a response
    async fn send(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Failed to send request");

        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to read body")
            .to_bytes();

        TestResponse { status, body }
    }
}

/// Response from a test request
pub struct TestResponse {
    pub status: StatusCode,
    body: bytes::Bytes,
}

impl TestResponse {
    /// Get the response body as a string
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// Parse the response body as JSON
    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).expect("Failed to parse JSON response")
    }

    /// Parse the response body as a generic JSON Value
    pub fn json_value(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Failed to parse JSON response")
    }

    /// Assert the status code
    pub fn assert_status(&self, expected: StatusCode) {
        assert_eq!(
            self.status,
            expected,
            "Expected status {}, got {}. Body: {}",
            expected,
            self.status,
            self.text()
        );
    }
}

/// Helper to generate a unique test email
pub fn test_email() -> String {
    format!("test-{}@example.com", uuid::Uuid::new_v4())
}

/// Helper to generate a valid test password
pub fn test_password() -> String {
    "SecurePassword123!".to_string()
}

/// Create a test config with a custom litellm_host for mocking external services
pub fn test_config_with_ollama_host(ollama_host: &str) -> Config {
    Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/zone_test".to_string()
        }),
        redis_url: std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        jwt_secret: "test-secret-key-must-be-at-least-32-chars-long".to_string(),
        jwt_access_lifetime: 900,
        jwt_refresh_lifetime: 604800,
        litellm_host: ollama_host.to_string(),
        litellm_key: "test-key".to_string(),
        ollama_host: ollama_host.to_string(),
        gpt4all_models_url: zone_server::config::DEFAULT_GPT4ALL_MODELS_URL.to_string(),
        huggingface_models_url: zone_server::config::DEFAULT_HUGGINGFACE_MODELS_URL.to_string(),
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        web_search: Default::default(),
        comfyui: Default::default(),
    }
}

/// Alias for test_config
pub fn create_test_config() -> Config {
    test_config()
}

/// Setup test data: organization, workspace, and user
/// Returns (org_id, workspace_id, user_id)
pub async fn setup_test_data(pool: &PgPool) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    use zone_server::db::{organizations, users, workspaces};

    // Create organization
    let org_id = organizations::create_organization(
        pool,
        &format!("Test Org {}", uuid::Uuid::new_v4()),
        &format!("test-org-{}", uuid::Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create organization")
    .id;

    // Create workspace
    let workspace_id = workspaces::create_workspace(
        pool,
        org_id,
        &format!("Test Workspace {}", uuid::Uuid::new_v4()),
        &format!("test-ws-{}", uuid::Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create workspace")
    .id;

    // Create user
    let user_email = format!("test-{}@example.com", uuid::Uuid::new_v4());
    let user_id = users::create_user(pool, &user_email, "password_hash", Some("Test User"), false)
        .await
        .expect("Failed to create user")
        .id;

    (org_id, workspace_id, user_id)
}
