//! Test utilities for zone_server integration tests
//!
//! Provides helpers for creating test configurations, database connections,
//! and HTTP test clients.

#![allow(dead_code)]

pub mod context;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;
use zone_context::adapters::{AdapterRegistry, TextAdapter};
use zone_context::context::ContextService;
use zone_context::embeddings::EmbeddingService;

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
        model_search_proxy_url: None,
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        github_api_url: zone_server::config::DEFAULT_GITHUB_API_URL.to_string(),
        web_search: Default::default(),
        comfyui: Default::default(),
        source_index: Default::default(),
        monitoring: Default::default(),
        chat: Default::default(),
        train_upload_limit_mb: 512,
    }
}

/// Require an explicitly selected disposable database for canonical chat tests.
/// CI already supplies TEST_DATABASE_URL; the context-specific override is optional.
pub fn context_database_url() -> String {
    select_context_database(
        std::env::var("ZONE_CONTEXT_TEST_DATABASE_URL").ok(),
        std::env::var("TEST_DATABASE_URL").ok(),
    ).expect("Set TEST_DATABASE_URL or ZONE_CONTEXT_TEST_DATABASE_URL to an isolated migrated test database")
}

pub fn select_context_database(
    explicit: Option<String>,
    configured: Option<String>,
) -> Result<String, &'static str> {
    explicit.or(configured).filter(|address| !address.trim().is_empty())
        .ok_or("Set TEST_DATABASE_URL or ZONE_CONTEXT_TEST_DATABASE_URL to an isolated migrated test database")
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
    let state = AppState::new(config, pool, None);
    state.disable_mcp();
    state
}

/// Create a test router with the given state
pub fn create_test_router(state: AppState) -> Router {
    create_router(state)
}

/// Test client for making HTTP requests to the test router
pub struct TestClient {
    router: Router,
    state: Option<AppState>,
}

impl TestClient {
    /// Create a new test client
    pub fn new(router: Router) -> Self {
        Self {
            router,
            state: None,
        }
    }

    /// Create a test client with a database connection
    pub async fn with_db() -> Self {
        Self::with_config(test_config()).await
    }

    /// Create a test client whose config has been adjusted, for tests that
    /// need to point an upstream URL at a local stub.
    pub async fn with_config(config: Config) -> Self {
        let pool = create_test_pool().await;
        let state = create_test_state(config, pool);
        let router = create_test_router(state.clone());
        Self {
            router,
            state: Some(state),
        }
    }

    /// Create a test client whose state embeds through the given service, for
    /// a test that has to see what the routes do when embedding fails.
    pub async fn with_embedding(embedding: Arc<dyn EmbeddingService>) -> Self {
        let pool = create_test_pool().await;
        let mut adapters = AdapterRegistry::new();
        adapters.register(TextAdapter::new());
        let adapters = Arc::new(adapters);
        let context = Arc::new(ContextService::new(
            pool.clone(),
            Arc::clone(&adapters),
            Arc::clone(&embedding),
        ));
        let state =
            AppState::new_with_services(test_config(), pool, None, adapters, embedding, context);
        state.disable_mcp();
        let router = create_test_router(state.clone());
        Self {
            router,
            state: Some(state),
        }
    }

    /// The state the router was built over, for a test that drives a worker as
    /// well as the routes.
    pub fn state(&self) -> &AppState {
        self.state
            .as_ref()
            .expect("this client was built from a bare router and has no state")
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

    /// Make a HEAD request
    pub async fn head(&self, uri: &str) -> TestResponse {
        let request = Request::builder()
            .method("HEAD")
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

    /// Make a GET request with authorization and a Range header
    pub async fn get_range_auth(&self, uri: &str, range: &str, token: &str) -> TestResponse {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .header("Authorization", format!("Bearer {}", token))
            .header("Range", range)
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

    /// Send a fully built request, for cases the typed helpers do not cover
    pub async fn send_request(&self, request: Request<Body>) -> TestResponse {
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
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to read body")
            .to_bytes();

        TestResponse {
            status,
            headers,
            body,
        }
    }
}

/// Response from a test request
pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    body: bytes::Bytes,
}

impl TestResponse {
    /// Get the response body as a string
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// Get the response body as raw bytes
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    /// Get a response header as a string
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
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
///
/// The value is generated once per process so register/login pairs that call
/// this helper more than once still share the same secret.
pub fn test_password() -> String {
    static PASSWORD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PASSWORD
        .get_or_init(|| {
            let id = uuid::Uuid::new_v4();
            let mut chars: Vec<char> = id.simple().to_string().chars().collect();
            if let Some(letter) = chars.iter_mut().find(|ch| ch.is_ascii_lowercase()) {
                *letter = letter.to_ascii_uppercase();
            }
            chars.into_iter().collect()
        })
        .clone()
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
        model_search_proxy_url: None,
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        github_api_url: zone_server::config::DEFAULT_GITHUB_API_URL.to_string(),
        web_search: Default::default(),
        comfyui: Default::default(),
        source_index: Default::default(),
        monitoring: Default::default(),
        chat: Default::default(),
        train_upload_limit_mb: 512,
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

/// A user who is actually a member of the workspace it will act in.
///
/// [`setup_test_data`] leaves the user outside the workspace, which is enough
/// for routes that only read ids but not for a task run: the worker checks the
/// trigger's role before it claims anything, and a task tool set reaches
/// workspace tools only for a member.
pub async fn setup_workspace_member(pool: &PgPool) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    use zone_server::db::workspace_members::{WorkspaceRole, add_member};

    let (organization, workspace, user) = setup_test_data(pool).await;
    add_member(pool, workspace, user, WorkspaceRole::Member, None)
        .await
        .expect("the user joins the workspace it will run tasks in");
    (organization, workspace, user)
}

/// Serve the real router on an ephemeral port and return its `host:port`.
///
/// The oneshot [`TestClient`] cannot upgrade a connection, so anything that
/// exercises a websocket needs a listening server rather than a router.
pub async fn serve(state: AppState) -> String {
    let router = create_test_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("an ephemeral port");
    let address = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("{}:{}", address.ip(), address.port())
}

/// Register a user and give it an organization, a workspace and a chat.
///
/// Returned in the order a caller needs them: the access token, the chat, and
/// the workspace, which is what scopes anything the chat's tools reach.
pub async fn seed_chat(client: &TestClient, model: &str) -> (String, String, String) {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let registered = client
        .post_json(
            "/api/auth/register",
            &serde_json::json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let token = registered["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("register must return an access token, got {registered}"))
        .to_string();

    let organization = client
        .post_json_auth(
            "/api/organizations",
            &serde_json::json!({"name": "Jobs Org", "slug": format!("jobs-org-{suffix}")}),
            &token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("an organization")
        .to_string();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &serde_json::json!({"name": "Jobs Workspace", "slug": format!("jobs-ws-{suffix}")}),
            &token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .to_string();
    let chat = client
        .post_json_auth(
            "/api/chats",
            // `agent_enabled` defaults to false on the route, and a chat with
            // no tool catalog runs its turn non-agentically: a scripted tool
            // call would be read as an empty reply.
            &serde_json::json!({
                "workspace_id": workspace,
                "title": "Jobs Chat",
                "model_name": model,
                "agent_enabled": true,
            }),
            &token,
        )
        .await
        .json_value()["chat"]["id"]
        .as_str()
        .expect("a chat")
        .to_string();

    (token, chat, workspace)
}

/// The next JSON frame a socket sends, or nothing if it closed or went quiet.
pub async fn next_frame<Socket>(socket: &mut Socket, within: Duration) -> Option<Value>
where
    Socket: futures_util::StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    use tokio_tungstenite::tungstenite::Message;

    loop {
        let frame = tokio::time::timeout(within, socket.next()).await.ok()??;
        match frame.ok()? {
            Message::Text(text) => return serde_json::from_str(&text).ok(),
            Message::Close(_) => return None,
            _ => continue,
        }
    }
}
