//! Integration tests for sync functionality

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::json;
use sha2::Sha256;
use sqlx::Executor;
use tower::ServiceExt;
use uuid::Uuid;

use zone_server::{
    config::Config,
    crypto,
    db::{
        DbPool, projects,
        sync_config::{self, SyncDirection, SyncEventDirection, SyncEventType},
        tasks,
    },
    routes::create_router,
    state::AppState,
    utils::crypto::generate_token,
};

type HmacSha256 = Hmac<Sha256>;

const GITHUB_SIGNATURE_HEADER: &str = "X-Hub-Signature-256";
const GITHUB_EVENT_HEADER: &str = "X-GitHub-Event";
const GITHUB_ISSUES_EVENT: &str = "issues";
const GITHUB_DELIVERY_HEADER: &str = "X-GitHub-Delivery";
const LINEAR_SIGNATURE_HEADER: &str = "Linear-Signature";
const LINEAR_DELIVERY_HEADER: &str = "Linear-Delivery";

fn now_milliseconds() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn setup_test_state() -> AppState {
    // Use test database
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/zone_test".to_string());

    let pool = DbPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database");

    let config = Config {
        host: "localhost".to_string(),
        port: 8000,
        database_url: database_url.clone(),
        redis_url: "redis://localhost:6379".to_string(),
        jwt_secret: "test-secret-key-with-at-least-32-chars".to_string(),
        jwt_access_lifetime: 900,
        jwt_refresh_lifetime: 604800,
        model_backend: Default::default(),
        agents: Default::default(),
        litellm_host: "http://localhost:4000".to_string(),
        litellm_key: "test-key".to_string(),
        ollama_host: "http://localhost:11434".to_string(),
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
        auto: Default::default(),
    };

    AppState::new(config, pool.inner().clone(), None)
}

/// Helper to cleanup test data using raw SQL (avoids sqlx! macro caching)
async fn cleanup_project(pool: &sqlx::PgPool, project_id: Uuid) {
    let _ = pool.execute(
        sqlx::query("DELETE FROM sync_events WHERE sync_config_id IN (SELECT id FROM sync_configs WHERE project_id = $1)")
            .bind(project_id)
    ).await;
    let _ = pool.execute(
        sqlx::query("DELETE FROM synced_items WHERE sync_config_id IN (SELECT id FROM sync_configs WHERE project_id = $1)")
            .bind(project_id)
    ).await;
    let _ = pool
        .execute(sqlx::query("DELETE FROM sync_configs WHERE project_id = $1").bind(project_id))
        .await;
    let _ = pool
        .execute(
            sqlx::query(
                "DELETE FROM tasks WHERE id IN (SELECT task_id FROM task_projects WHERE project_id = $1)",
            )
            .bind(project_id),
        )
        .await;
    let _ = pool
        .execute(sqlx::query("DELETE FROM projects WHERE id = $1").bind(project_id))
        .await;
}

#[tokio::test]
async fn test_sync_config_lifecycle() {
    let state = setup_test_state().await;

    // Create a test project using db function
    let project = projects::create_project(
        state.db(),
        "Sync Test Project",
        Some("Test Description"),
        None, // workspace_id
    )
    .await
    .expect("Failed to create project");

    // Test creating sync config
    let config_json = json!({
        "owner": "test-owner",
        "repo": "test-repo",
        "token": "ghp_test123"
    });

    let encryption_key =
        crypto::derive_key(state.config().encryption_key()).expect("Failed to derive key");
    let encrypted_secret =
        crypto::encrypt(&encryption_key, "webhook-secret").expect("Failed to encrypt");

    let sync_config_row = sync_config::create_sync_config(
        state.db(),
        project.id,
        "github",
        true,
        config_json.clone(),
        Some(&encrypted_secret),
    )
    .await
    .expect("Failed to create sync config");

    assert_eq!(sync_config_row.provider, "github");
    assert!(sync_config_row.enabled);
    assert_eq!(sync_config_row.config, config_json);

    // Test getting sync config
    let retrieved = sync_config::get_sync_config(state.db(), sync_config_row.id)
        .await
        .expect("Failed to get sync config");

    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, sync_config_row.id);
    assert_eq!(retrieved.provider, "github");

    // Test listing sync configs
    let configs = sync_config::list_sync_configs(state.db(), project.id)
        .await
        .expect("Failed to list sync configs");

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].id, sync_config_row.id);

    // Test updating sync config
    let updated = sync_config::update_sync_config(
        state.db(),
        sync_config_row.id,
        Some(false), // Disable it
        None,
        None,
    )
    .await
    .expect("Failed to update sync config");

    assert!(updated.is_some());
    let updated = updated.unwrap();
    assert!(!updated.enabled);

    // Test deleting sync config
    let deleted = sync_config::delete_sync_config(state.db(), sync_config_row.id)
        .await
        .expect("Failed to delete sync config");

    assert!(deleted);

    // Cleanup
    cleanup_project(state.db(), project.id).await;
}

#[tokio::test]
async fn test_synced_item_lifecycle() {
    let state = setup_test_state().await;

    // Setup test data (organization, workspace, user)
    let (_org_id, workspace_id, _user_id) = common::setup_test_data(state.db()).await;

    // Create test project using db function
    let project = projects::create_project(
        state.db(),
        "Synced Item Test Project",
        Some("Test Description"),
        Some(workspace_id),
    )
    .await
    .expect("Failed to create project");

    let task = tasks::create_task(
        state.db(),
        workspace_id,
        &[project.id],
        "Test Task",
        "Test Description",
        None,
        None,
        false,
        None,
    )
    .await
    .expect("Failed to create task");

    // Create sync config
    let config_json = json!({
        "owner": "test-owner",
        "repo": "test-repo",
        "token": "ghp_test123"
    });

    let sync_config_row =
        sync_config::create_sync_config(state.db(), project.id, "github", true, config_json, None)
            .await
            .expect("Failed to create sync config");

    // Create synced item
    let external_state = json!({
        "state": "open",
        "number": 123
    });

    let synced_item = sync_config::create_synced_item(
        state.db(),
        sync_config_row.id,
        task.id,
        "123",
        Some("https://github.com/test-owner/test-repo/issues/123"),
        SyncDirection::Bidirectional,
        Some(external_state.clone()),
    )
    .await
    .expect("Failed to create synced item");

    assert_eq!(synced_item.external_id, "123");
    assert_eq!(synced_item.task_id, task.id);
    assert_eq!(synced_item.sync_direction, SyncDirection::Bidirectional);

    // Test getting by task
    let by_task = sync_config::get_synced_item_by_task(state.db(), sync_config_row.id, task.id)
        .await
        .expect("Failed to get synced item by task");

    assert!(by_task.is_some());
    assert_eq!(by_task.unwrap().id, synced_item.id);

    // Test getting by external ID
    let by_external =
        sync_config::get_synced_item_by_external_id(state.db(), sync_config_row.id, "123")
            .await
            .expect("Failed to get synced item by external ID");

    assert!(by_external.is_some());
    assert_eq!(by_external.unwrap().id, synced_item.id);

    // Test updating synced item
    let new_state = json!({"state": "closed"});
    let updated =
        sync_config::update_synced_item(state.db(), synced_item.id, Some(new_state.clone()))
            .await
            .expect("Failed to update synced item");

    assert!(updated.is_some());
    assert_eq!(updated.unwrap().last_external_state, Some(new_state));

    // Test deleting synced item
    let deleted = sync_config::delete_synced_item(state.db(), synced_item.id)
        .await
        .expect("Failed to delete synced item");

    assert!(deleted);

    // Cleanup
    cleanup_project(state.db(), project.id).await;
}

#[tokio::test]
async fn test_sync_event_logging() {
    let state = setup_test_state().await;

    // Create test project
    let project = projects::create_project(
        state.db(),
        "Sync Event Test Project",
        Some("Test Description"),
        None,
    )
    .await
    .expect("Failed to create project");

    // Create sync config
    let config_json = json!({
        "owner": "test-owner",
        "repo": "test-repo",
        "token": "ghp_test123"
    });

    let sync_config_row =
        sync_config::create_sync_config(state.db(), project.id, "github", true, config_json, None)
            .await
            .expect("Failed to create sync config");

    // Create sync event
    let payload = json!({
        "action": "opened",
        "issue": {
            "number": 123,
            "title": "Test Issue"
        }
    });

    let event = sync_config::create_sync_event(
        state.db(),
        sync_config_row.id,
        None,
        SyncEventType::WebhookReceived,
        SyncEventDirection::Inbound,
        Some(payload.clone()),
        None,
    )
    .await
    .expect("Failed to create sync event");

    assert_eq!(event.event_type, SyncEventType::WebhookReceived);
    assert_eq!(event.direction, SyncEventDirection::Inbound);
    assert_eq!(event.payload, Some(payload));
    assert!(event.error_message.is_none());

    // List events
    let events = sync_config::list_sync_events(state.db(), sync_config_row.id, 10)
        .await
        .expect("Failed to list sync events");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, event.id);

    // Cleanup
    cleanup_project(state.db(), project.id).await;
}

#[tokio::test]
async fn every_sync_event_type_and_direction_satisfies_the_sync_events_constraints() {
    let state = setup_test_state().await;
    let project = projects::create_project(state.db(), "Sync Event Drift Project", None, None)
        .await
        .expect("Failed to create project");
    let sync_config_row = sync_config::create_sync_config(
        state.db(),
        project.id,
        "github",
        true,
        json!({ "owner": "test-owner", "repo": "test-repo", "token": "ghp_test123" }),
        None,
    )
    .await
    .expect("Failed to create sync config");

    for event_type in SyncEventType::ALL {
        for direction in SyncEventDirection::ALL {
            let event = sync_config::create_sync_event(
                state.db(),
                sync_config_row.id,
                None,
                event_type,
                direction,
                None,
                None,
            )
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "sync_events rejected {} {}: {error}",
                    event_type.as_str(),
                    direction.as_str()
                )
            });
            assert_eq!(event.event_type, event_type);
            assert_eq!(event.direction, direction);
        }
    }

    let listed = sync_config::list_sync_events(state.db(), sync_config_row.id, 100)
        .await
        .expect("every logged sync event reads back into its enums");
    assert_eq!(
        listed.len(),
        SyncEventType::ALL.len() * SyncEventDirection::ALL.len()
    );

    cleanup_project(state.db(), project.id).await;
}

#[tokio::test]
async fn every_sync_direction_satisfies_the_synced_items_constraint() {
    let state = setup_test_state().await;
    let (_organization_id, workspace_id, _user_id) = common::setup_test_data(state.db()).await;
    let project = projects::create_project(
        state.db(),
        "Synced Item Drift Project",
        None,
        Some(workspace_id),
    )
    .await
    .expect("Failed to create project");
    let task = tasks::create_task(
        state.db(),
        workspace_id,
        &[project.id],
        "Drift Task",
        "Drift description",
        None,
        None,
        false,
        None,
    )
    .await
    .expect("Failed to create task");
    let sync_config_row = sync_config::create_sync_config(
        state.db(),
        project.id,
        "github",
        true,
        json!({ "owner": "test-owner", "repo": "test-repo", "token": "ghp_test123" }),
        None,
    )
    .await
    .expect("Failed to create sync config");

    for direction in SyncDirection::ALL {
        let created = sync_config::create_synced_item(
            state.db(),
            sync_config_row.id,
            task.id,
            "123",
            None,
            direction,
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("synced_items rejected {}: {error}", direction.as_str()));
        assert_eq!(created.sync_direction, direction);

        let found =
            sync_config::get_synced_item_by_external_id(state.db(), sync_config_row.id, "123")
                .await
                .expect("a synced item reads back into its direction")
                .expect("the synced item exists");
        assert_eq!(found.sync_direction, direction);

        sync_config::delete_synced_item(state.db(), created.id)
            .await
            .expect("Failed to delete synced item");
    }

    let _ = state
        .db()
        .execute(sqlx::query("DELETE FROM tasks WHERE id = $1").bind(task.id))
        .await;
    cleanup_project(state.db(), project.id).await;
}

#[tokio::test]
async fn test_github_webhook_signature_verification() {
    let state = setup_test_state().await;

    // Create test project
    let project = projects::create_project(
        state.db(),
        "GitHub Webhook Test Project",
        Some("Test Description"),
        None,
    )
    .await
    .expect("Failed to create project");

    // Create sync config with webhook secret
    let config_json = json!({
        "owner": "test-owner",
        "repo": "test-repo",
        "token": "ghp_test123"
    });

    let webhook_secret = Uuid::new_v4().to_string();
    let encryption_key =
        crypto::derive_key(state.config().encryption_key()).expect("Failed to derive key");
    let encrypted_secret =
        crypto::encrypt(&encryption_key, &webhook_secret).expect("Failed to encrypt");

    let sync_config_row = sync_config::create_sync_config(
        state.db(),
        project.id,
        "github",
        true,
        config_json,
        Some(&encrypted_secret),
    )
    .await
    .expect("Failed to create sync config");

    // Create test webhook payload
    let payload = json!({
        "action": "opened",
        "issue": {
            "number": 123,
            "title": "Test Issue",
            "body": "Test body",
            "state": "open",
            "html_url": "https://github.com/test-owner/test-repo/issues/123"
        }
    });

    let payload_bytes = serde_json::to_vec(&payload).expect("Failed to serialize payload");

    // Compute valid signature
    let mut mac =
        HmacSha256::new_from_slice(webhook_secret.as_bytes()).expect("Failed to create HMAC");
    mac.update(&payload_bytes);
    let result = mac.finalize();
    let signature = format!("sha256={}", hex::encode(result.into_bytes()));

    // Create request with valid signature
    let app = create_router(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{}/github", sync_config_row.id))
        .header("Content-Type", "application/json")
        .header(GITHUB_SIGNATURE_HEADER, signature)
        .header(GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Test with invalid signature
    let app = create_router(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{}/github", sync_config_row.id))
        .header("Content-Type", "application/json")
        .header(GITHUB_SIGNATURE_HEADER, "sha256=invalid")
        .header(GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT)
        .body(Body::from(payload_bytes))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 401 Unauthorized
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Cleanup
    cleanup_project(state.db(), project.id).await;
}

#[tokio::test]
async fn test_linear_webhook_signature_verification() {
    let state = setup_test_state().await;

    // Create test project
    let project = projects::create_project(
        state.db(),
        "Linear Webhook Test Project",
        Some("Test Description"),
        None,
    )
    .await
    .expect("Failed to create project");

    // Create sync config with webhook secret
    let config_json = json!({
        "api_key": "lin_api_test123",
        "team_id": "TEAM-123"
    });

    let webhook_secret = Uuid::new_v4().to_string();
    let encryption_key =
        crypto::derive_key(state.config().encryption_key()).expect("Failed to derive key");
    let encrypted_secret =
        crypto::encrypt(&encryption_key, &webhook_secret).expect("Failed to encrypt");

    let sync_config_row = sync_config::create_sync_config(
        state.db(),
        project.id,
        "linear",
        true,
        config_json,
        Some(&encrypted_secret),
    )
    .await
    .expect("Failed to create sync config");

    // Create test webhook payload
    let payload = json!({
        "action": "create",
        "type": "Issue",
        "data": {
            "id": "issue-123",
            "title": "Test Issue",
            "description": "Test description",
            "state": {
                "type": "started",
                "name": "In Progress"
            }
        },
        "webhookTimestamp": now_milliseconds()
    });

    let payload_bytes = serde_json::to_vec(&payload).expect("Failed to serialize payload");

    // Compute valid signature (Linear uses raw hex, no prefix)
    let mut mac =
        HmacSha256::new_from_slice(webhook_secret.as_bytes()).expect("Failed to create HMAC");
    mac.update(&payload_bytes);
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());

    // Create request with valid signature
    let app = create_router(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{}/linear", sync_config_row.id))
        .header("Content-Type", "application/json")
        .header(LINEAR_SIGNATURE_HEADER, signature)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Test with invalid signature
    let app = create_router(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{}/linear", sync_config_row.id))
        .header("Content-Type", "application/json")
        .header(LINEAR_SIGNATURE_HEADER, "invalid")
        .body(Body::from(payload_bytes))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 401 Unauthorized
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Cleanup
    cleanup_project(state.db(), project.id).await;
}

async fn deliver_to(
    state: &AppState,
    sync_config_id: Uuid,
    provider: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{sync_config_id}/{provider}"))
        .header("Content-Type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = create_router(state.clone())
        .oneshot(request.body(Body::from(body.to_vec())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

struct SyncedTask {
    state: AppState,
    project_id: Uuid,
    task_id: Uuid,
    sync_config_id: Uuid,
    synced_item_id: Uuid,
    webhook_secret: String,
}

impl SyncedTask {
    async fn create(provider: &str, config: serde_json::Value, direction: SyncDirection) -> Self {
        let state = setup_test_state().await;
        let (_organization_id, workspace_id, _user_id) = common::setup_test_data(state.db()).await;
        let project = projects::create_project(
            state.db(),
            "Inbound Webhook Project",
            None,
            Some(workspace_id),
        )
        .await
        .expect("Failed to create project");
        let task = tasks::create_task(
            state.db(),
            workspace_id,
            &[project.id],
            "Original",
            "Original description",
            None,
            None,
            false,
            None,
        )
        .await
        .expect("Failed to create task");

        let webhook_secret = Uuid::new_v4().to_string();
        let encryption_key =
            crypto::derive_key(state.config().encryption_key()).expect("Failed to derive key");
        let encrypted_secret =
            crypto::encrypt(&encryption_key, &webhook_secret).expect("Failed to encrypt");
        let sync_config_row = sync_config::create_sync_config(
            state.db(),
            project.id,
            provider,
            true,
            config,
            Some(&encrypted_secret),
        )
        .await
        .expect("Failed to create sync config");
        let synced_item = sync_config::create_synced_item(
            state.db(),
            sync_config_row.id,
            task.id,
            "123",
            None,
            direction,
            None,
        )
        .await
        .expect("Failed to create synced item");

        Self {
            state,
            project_id: project.id,
            task_id: task.id,
            sync_config_id: sync_config_row.id,
            synced_item_id: synced_item.id,
            webhook_secret,
        }
    }

    fn sign(&self, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .expect("Failed to create HMAC");
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    async fn deliver(
        &self,
        provider: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> (StatusCode, serde_json::Value) {
        deliver_to(&self.state, self.sync_config_id, provider, headers, body).await
    }

    async fn post(&self, provider: &str, headers: &[(&str, &str)], body: &[u8]) -> StatusCode {
        self.deliver(provider, headers, body).await.0
    }

    async fn post_linear(&self, body: &serde_json::Value) -> StatusCode {
        self.send_linear(body, None).await.0
    }

    async fn send_linear(
        &self,
        body: &serde_json::Value,
        delivery: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::to_vec(body).unwrap();
        let signature = self.sign(&body);
        let mut headers = vec![(LINEAR_SIGNATURE_HEADER, signature.as_str())];
        if let Some(delivery) = delivery {
            headers.push((LINEAR_DELIVERY_HEADER, delivery));
        }
        self.deliver("linear", &headers, &body).await
    }

    async fn event_types(&self) -> Vec<SyncEventType> {
        sync_config::list_sync_events(self.state.db(), self.sync_config_id, 100)
            .await
            .expect("Failed to read sync events")
            .into_iter()
            .map(|event| event.event_type)
            .collect()
    }

    async fn item_event_types(&self) -> Vec<SyncEventType> {
        let mut events = sync_config::list_sync_events(self.state.db(), self.sync_config_id, 100)
            .await
            .expect("Failed to read sync events");
        events.reverse();
        events
            .into_iter()
            .filter(|event| event.synced_item_id == Some(self.synced_item_id))
            .map(|event| event.event_type)
            .collect()
    }

    async fn post_github(&self, body: &serde_json::Value) -> StatusCode {
        self.send_github(body, None).await.0
    }

    async fn send_github(
        &self,
        body: &serde_json::Value,
        delivery: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::to_vec(body).unwrap();
        let signature = format!("sha256={}", self.sign(&body));
        let mut headers = vec![
            (GITHUB_SIGNATURE_HEADER, signature.as_str()),
            (GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT),
        ];
        if let Some(delivery) = delivery {
            headers.push((GITHUB_DELIVERY_HEADER, delivery));
        }
        self.deliver("github", &headers, &body).await
    }

    async fn task(&self) -> tasks::TaskRow {
        tasks::get_task(self.state.db(), self.task_id)
            .await
            .expect("Failed to read task")
            .expect("Task disappeared")
    }

    async fn set_task(&self, title: &str, status: &str) {
        tasks::update_task(
            self.state.db(),
            self.task_id,
            Some(title),
            None,
            None,
            Some(status),
            None,
            None,
        )
        .await
        .expect("Failed to update task")
        .expect("Task disappeared");
    }

    async fn new_tasks(&self) -> Vec<tasks::TaskRow> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT t.id FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
             WHERE tp.project_id = $1 AND t.id <> $2 ORDER BY t.created_at",
        )
        .bind(self.project_id)
        .bind(self.task_id)
        .fetch_all(self.state.db())
        .await
        .expect("Failed to list the project's tasks");
        let mut found = Vec::new();
        for id in ids {
            found.push(
                tasks::get_task(self.state.db(), id)
                    .await
                    .expect("Failed to read task")
                    .expect("Task disappeared"),
            );
        }
        found
    }

    async fn new_task(&self) -> tasks::TaskRow {
        let mut created = self.new_tasks().await;
        assert_eq!(created.len(), 1, "exactly one task from the new issue");
        created.remove(0)
    }

    async fn linked(&self, external_id: &str) -> Option<sync_config::SyncedItemRow> {
        sync_config::get_synced_item_by_external_id(
            self.state.db(),
            self.sync_config_id,
            external_id,
        )
        .await
        .expect("Failed to read synced item")
    }

    async fn was_unlinked(&self, external_id: &str) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sync_unlinked_items \
             WHERE sync_config_id = $1 AND external_id = $2)",
        )
        .bind(self.sync_config_id)
        .bind(external_id)
        .fetch_one(self.state.db())
        .await
        .expect("Failed to read unlinked issues")
    }

    async fn link_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM synced_items WHERE sync_config_id = $1")
            .bind(self.sync_config_id)
            .fetch_one(self.state.db())
            .await
            .expect("Failed to count synced items")
    }

    async fn cleanup(self) {
        let _ = self
            .state
            .db()
            .execute(sqlx::query("DELETE FROM tasks WHERE id = $1").bind(self.task_id))
            .await;
        cleanup_project(self.state.db(), self.project_id).await;
    }
}

#[tokio::test]
async fn a_signed_github_edit_updates_the_task_and_answers_ok() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = github_issue("edited", "Renamed");

    let status = synced.post_github(&body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(synced.item_event_types().await, vec![SyncEventType::Update]);
    assert_eq!(synced.task().await.title, "Renamed");

    let status = synced
        .post(
            "github",
            &[
                (GITHUB_SIGNATURE_HEADER, "sha256=invalid"),
                (GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT),
            ],
            &serde_json::to_vec(&body).unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    synced.cleanup().await;
}

#[tokio::test]
async fn a_signed_github_delete_unlinks_the_issue_and_leaves_the_task_as_it_was() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    synced.set_task("Original", "in_progress").await;

    let status = synced
        .post_github(&github_issue("deleted", "Renamed"))
        .await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.task().await;
    assert_eq!(task.title, "Original");
    assert_eq!(task.status, "in_progress");
    assert!(
        synced.linked("123").await.is_none(),
        "a deleted issue is no longer linked to the task"
    );
    let logged: Vec<&str> = synced
        .event_types()
        .await
        .into_iter()
        .map(SyncEventType::as_str)
        .collect();
    assert!(logged.contains(&"unlink"), "{logged:?}");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_signed_linear_remove_unlinks_the_issue_and_leaves_the_task_as_it_was() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    synced.set_task("Original", "in_progress").await;
    let mut removed = linear_issue_at("Renamed", "completed", "2026-01-01T00:00:10.000Z");
    removed["action"] = json!("remove");

    let (status, response) = synced.send_linear(&removed, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["message"], "Issue unlinked from its task");
    let task = synced.task().await;
    assert_eq!(task.title, "Original");
    assert_eq!(task.status, "in_progress");
    assert!(
        synced.linked("123").await.is_none(),
        "a removed issue is no longer linked to the task"
    );
    assert!(
        synced.event_types().await.contains(&SyncEventType::Unlink),
        "the removal is logged as an unlink"
    );

    let mut recreated = linear_issue_update();
    recreated["action"] = json!("create");
    let (status, response) = synced.send_linear(&recreated, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        response["message"],
        "Issue was unlinked from its task and is not linked again"
    );
    assert!(synced.new_tasks().await.is_empty());
    assert!(synced.linked("123").await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn an_event_from_another_repository_leaves_the_task_linked_to_the_same_number_unchanged() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let mut foreign = github_issue("edited", "Renamed");
    foreign["repository"]["full_name"] = json!("someone-else/test-repo");
    foreign["issue"]["state"] = json!("closed");

    for action in ["edited", "closed", "deleted"] {
        foreign["action"] = json!(action);
        assert_eq!(
            synced.post_github(&foreign).await,
            StatusCode::OK,
            "{action}"
        );
    }
    let mut unplaced = github_issue("edited", "Renamed");
    unplaced.as_object_mut().unwrap().remove("repository");
    assert_eq!(synced.post_github(&unplaced).await, StatusCode::OK);

    let task = synced.task().await;
    assert_eq!(task.title, "Original");
    assert_eq!(task.status, "created");
    assert!(synced.linked("123").await.is_some());
    assert!(synced.item_event_types().await.is_empty());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_github_label_assignment_or_edit_leaves_the_task_status_alone() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    synced.set_task("Original", "in_progress").await;

    for action in ["labeled", "assigned", "edited"] {
        let status = synced.post_github(&github_issue(action, "Renamed")).await;
        assert_eq!(status, StatusCode::OK, "{action}");
    }

    let task = synced.task().await;
    assert_eq!(task.title, "Renamed");
    assert_eq!(task.status, "in_progress");

    synced.cleanup().await;
}

fn github_issue_at(action: &str, title: &str, state: &str, updated_at: &str) -> serde_json::Value {
    let mut issue = github_issue(action, title);
    issue["issue"]["state"] = json!(state);
    issue["issue"]["updated_at"] = json!(updated_at);
    issue
}

#[tokio::test]
async fn a_github_close_completes_the_task_and_a_reopen_brings_it_back() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    synced.set_task("Original", "in_progress").await;

    let closed = github_issue_at("closed", "Original", "closed", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&closed).await, StatusCode::OK);
    assert_eq!(synced.task().await.status, "complete");

    let reopened = github_issue_at("reopened", "Original", "open", "2026-01-01T00:00:20Z");
    assert_eq!(synced.post_github(&reopened).await, StatusCode::OK);
    assert_eq!(synced.task().await.status, "created");

    synced.cleanup().await;
}

#[tokio::test]
async fn an_edit_delivered_after_a_later_close_neither_reopens_nor_renames_the_task() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let closed = github_issue_at("closed", "Closed title", "closed", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&closed).await, StatusCode::OK);

    let late = github_issue_at("edited", "Stale title", "open", "2026-01-01T00:00:05Z");
    let (status, response) = synced.send_github(&late, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let repeated = github_issue_at("edited", "Closed title", "closed", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&repeated).await, StatusCode::OK);

    let task = synced.task().await;
    assert_eq!(task.title, "Closed title");
    assert_eq!(task.status, "complete");
    assert_eq!(synced.item_event_types().await, vec![SyncEventType::Close]);

    synced.cleanup().await;
}

#[tokio::test]
async fn an_edit_in_the_same_second_as_a_close_renames_the_task_without_reopening_it() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let closed = github_issue_at("closed", "Closed title", "closed", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&closed).await, StatusCode::OK);

    let simultaneous = github_issue_at(
        "edited",
        "Same-moment title",
        "open",
        "2026-01-01T00:00:10Z",
    );
    assert_eq!(synced.post_github(&simultaneous).await, StatusCode::OK);

    let task = synced.task().await;
    assert_eq!(task.title, "Same-moment title");
    assert_eq!(task.status, "complete");
    assert_eq!(
        synced.item_event_types().await,
        vec![SyncEventType::Close, SyncEventType::Update]
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_close_in_the_same_second_as_an_edit_completes_the_renamed_task() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    synced.set_task("Original", "in_progress").await;
    let edited = github_issue_at("edited", "Renamed", "open", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&edited).await, StatusCode::OK);

    let closed = github_issue_at("closed", "Renamed", "closed", "2026-01-01T00:00:10Z");
    let (status, response) = synced.send_github(&closed, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["message"], "Task updated from the issue");
    let task = synced.task().await;
    assert_eq!(task.title, "Renamed");
    assert_eq!(task.status, "complete");
    assert_eq!(
        synced.item_event_types().await,
        vec![SyncEventType::Update, SyncEventType::Close]
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_close_and_a_reopen_in_the_same_second_leave_the_task_created() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let closed = github_issue_at("closed", "Original", "closed", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&closed).await, StatusCode::OK);
    assert_eq!(synced.task().await.status, "complete");

    let reopened = github_issue_at("reopened", "Original", "open", "2026-01-01T00:00:10Z");
    let (status, response) = synced.send_github(&reopened, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(synced.task().await.status, "created", "{response}");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_deletion_older_than_the_last_change_applied_leaves_the_issue_linked() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let edited = github_issue_at("edited", "Renamed", "open", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&edited).await, StatusCode::OK);

    let deleted = github_issue_at("deleted", "Renamed", "open", "2026-01-01T00:00:05Z");
    let (status, response) = synced.send_github(&deleted, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(
        synced.linked("123").await.is_some(),
        "a deletion older than the edit applied is stale: {response}"
    );
    let current = github_issue_at("deleted", "Renamed", "open", "2026-01-01T00:00:10Z");
    assert_eq!(synced.post_github(&current).await, StatusCode::OK);
    assert!(synced.linked("123").await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_signed_comment_deletion_sent_as_an_issues_event_leaves_the_issue_linked() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let mut comment_deleted = github_issue("deleted", "Original");
    comment_deleted["comment"] = json!({ "id": 1, "body": "Never mind" });

    let (status, response) = synced.send_github(&comment_deleted, None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert!(
        synced.linked("123").await.is_some(),
        "a comment's deletion is not the issue's"
    );
    assert!(synced.item_event_types().await.is_empty());

    synced.cleanup().await;
}

async fn start_run(synced: &SyncedTask) -> Uuid {
    let run: Uuid = sqlx::query_scalar(
        "INSERT INTO task_runs(task_id,status) VALUES($1,'running') RETURNING id",
    )
    .bind(synced.task_id)
    .fetch_one(synced.state.db())
    .await
    .expect("Failed to start a run");
    sqlx::query("UPDATE tasks SET active_run_id = $2 WHERE id = $1")
        .bind(synced.task_id)
        .bind(run)
        .execute(synced.state.db())
        .await
        .expect("Failed to make the run active");
    run
}

async fn end_run(synced: &SyncedTask) {
    sqlx::query("UPDATE tasks SET active_run_id = NULL WHERE id = $1")
        .bind(synced.task_id)
        .execute(synced.state.db())
        .await
        .expect("Failed to end the run");
}

#[tokio::test]
async fn a_close_while_a_run_owns_the_status_renames_the_task_and_a_later_edit_completes_it() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let edited = github_issue_at("edited", "Original", "open", "2026-01-01T00:00:05Z");
    assert_eq!(synced.post_github(&edited).await, StatusCode::OK);
    synced.set_task("Original", "in_progress").await;
    start_run(&synced).await;

    let closed = github_issue_at("closed", "Closed title", "closed", "2026-01-01T00:00:10Z");
    let (status, response) = synced.send_github(&closed, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    let task = synced.task().await;
    assert_eq!(
        task.title, "Closed title",
        "the rest of the patch still applies"
    );
    assert_eq!(task.status, "in_progress", "the live run keeps its status");
    assert_eq!(
        synced
            .linked("123")
            .await
            .and_then(|item| item.last_external_state)
            .map(|state| state["state"].clone()),
        Some(json!("open")),
        "the stored state stays the one the task followed"
    );

    end_run(&synced).await;
    let labeled = github_issue_at("labeled", "Closed title", "closed", "2026-01-01T00:00:20Z");
    assert_eq!(synced.post_github(&labeled).await, StatusCode::OK);
    assert_eq!(synced.task().await.status, "complete");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_linear_move_while_a_run_owns_the_status_renames_the_task_and_catches_up_after() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let started = linear_issue_at("Original", "started", "2026-01-01T00:00:01.000Z");
    assert_eq!(synced.post_linear(&started).await, StatusCode::OK);
    synced.set_task("Original", "in_progress").await;
    start_run(&synced).await;

    let completed = linear_issue_at("Done title", "completed", "2026-01-01T00:00:02.000Z");
    assert_eq!(synced.post_linear(&completed).await, StatusCode::OK);
    let task = synced.task().await;
    assert_eq!(task.title, "Done title");
    assert_eq!(task.status, "in_progress");

    end_run(&synced).await;
    let again = linear_issue_at("Done title", "completed", "2026-01-01T00:00:03.000Z");
    assert_eq!(synced.post_linear(&again).await, StatusCode::OK);
    assert_eq!(synced.task().await.status, "complete");

    synced.cleanup().await;
}

const DELIVERY: &str = "72d3162e-cc78-11e3-81ab-4c9367dc0958";

fn says_already_processed(response: &serde_json::Value) -> bool {
    response["message"]
        .as_str()
        .is_some_and(|message| message.contains("already processed"))
}

#[tokio::test]
async fn a_repeated_github_delivery_is_answered_already_processed_and_applied_once() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = github_issue("edited", "Renamed");

    let (status, response) = synced.send_github(&body, Some(DELIVERY)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(synced.task().await.title, "Renamed");
    synced.set_task("Renamed in Zone", "created").await;

    let (status, response) = synced.send_github(&body, Some(DELIVERY)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(says_already_processed(&response), "{response}");
    let (first, second) = tokio::join!(
        synced.send_github(&body, Some(DELIVERY)),
        synced.send_github(&body, Some(DELIVERY))
    );
    assert_eq!((first.0, second.0), (StatusCode::OK, StatusCode::OK));

    assert_eq!(synced.task().await.title, "Renamed in Zone");
    assert_eq!(synced.item_event_types().await, vec![SyncEventType::Update]);

    synced.cleanup().await;
}

#[tokio::test]
async fn a_repeated_linear_delivery_is_answered_already_processed_and_applied_once() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let body = linear_issue_update();

    let (status, response) = synced.send_linear(&body, Some(DELIVERY)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    synced.set_task("Renamed in Zone", "created").await;

    let (status, response) = synced.send_linear(&body, Some(DELIVERY)).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(says_already_processed(&response), "{response}");
    assert_eq!(synced.task().await.title, "Renamed in Zone");
    assert_eq!(synced.item_event_types().await, vec![SyncEventType::Update]);

    synced.cleanup().await;
}

#[tokio::test]
async fn a_delivery_over_a_megabyte_is_refused_as_too_large() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = vec![b' '; 1024 * 1024 + 1];
    let signature = format!("sha256={}", synced.sign(&body));

    let status = synced
        .post(
            "github",
            &[
                (GITHUB_SIGNATURE_HEADER, &signature),
                (GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT),
            ],
            &body,
        )
        .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    synced.cleanup().await;
}

#[tokio::test]
async fn every_delivery_refused_before_its_signature_is_checked_gets_the_same_answer() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = serde_json::to_vec(&github_issue("edited", "Renamed")).unwrap();
    let signature = format!("sha256={}", synced.sign(&body));
    let github_headers = [
        (GITHUB_SIGNATURE_HEADER, signature.as_str()),
        (GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT),
    ];
    let linear_signature = synced.sign(&body);
    let linear_headers = [(LINEAR_SIGNATURE_HEADER, linear_signature.as_str())];
    let forged = synced
        .deliver(
            "github",
            &[
                (GITHUB_SIGNATURE_HEADER, "sha256=forged"),
                (GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT),
            ],
            &body,
        )
        .await;
    assert_eq!(forged.0, StatusCode::UNAUTHORIZED);

    let unknown = deliver_to(
        &synced.state,
        Uuid::new_v4(),
        "github",
        &github_headers,
        &body,
    )
    .await;
    let wrong_provider = synced.deliver("linear", &linear_headers, &body).await;
    let secretless = sync_config::create_sync_config(
        synced.state.db(),
        synced.project_id,
        "linear",
        true,
        linear_config(),
        None,
    )
    .await
    .expect("Failed to create sync config");
    let unsecured = deliver_to(
        &synced.state,
        secretless.id,
        "linear",
        &linear_headers,
        &body,
    )
    .await;
    sync_config::update_sync_config(
        synced.state.db(),
        synced.sync_config_id,
        Some(false),
        None,
        None,
    )
    .await
    .expect("Failed to disable sync config");
    let disabled = synced.deliver("github", &github_headers, &body).await;

    for (name, answer) in [
        ("unknown", unknown),
        ("wrong provider", wrong_provider),
        ("no secret", unsecured),
        ("disabled", disabled),
    ] {
        assert_eq!(answer, forged, "{name}");
    }
    assert_eq!(synced.task().await.title, "Original");

    synced.cleanup().await;
}

#[tokio::test]
async fn an_outbound_only_item_ignores_a_signed_inbound_edit() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Outbound).await;

    let status = synced.post_github(&github_issue("edited", "Renamed")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(synced.task().await.title, "Original");
    assert!(synced.item_event_types().await.is_empty());

    synced.cleanup().await;
}

#[tokio::test]
async fn an_overlong_multibyte_title_and_description_are_cut_on_a_character_boundary() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let title = format!("a{}", "é".repeat(250));
    let description = format!("a{}", "é".repeat(25_000));
    let mut body = github_issue("edited", &title);
    body["issue"]["body"] = json!(description);

    let status = synced.post_github(&body).await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.task().await;
    assert_eq!(task.title, format!("a{}", "é".repeat(249)));
    assert_eq!(task.description, format!("a{}", "é".repeat(24_999)));

    synced.cleanup().await;
}

fn github_config() -> serde_json::Value {
    json!({
        "owner": "test-owner",
        "repo": "test-repo",
        "token": "ghp_test123",
        "external_repo_url": "https://github.com/test-owner/test-repo"
    })
}

fn github_issue(action: &str, title: &str) -> serde_json::Value {
    json!({
        "action": action,
        "issue": {
            "number": 123,
            "title": title,
            "body": "Edited body",
            "state": "open",
            "html_url": "https://github.com/test-owner/test-repo/issues/123"
        },
        "repository": { "full_name": "test-owner/test-repo" }
    })
}

#[tokio::test]
async fn a_signed_linear_update_updates_the_task_and_answers_ok() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let body = linear_issue_update();

    let status = synced.post_linear(&body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(synced.item_event_types().await, vec![SyncEventType::Update]);
    assert_eq!(synced.task().await.title, "Renamed");

    let status = synced
        .post(
            "linear",
            &[(LINEAR_SIGNATURE_HEADER, "invalid")],
            &serde_json::to_vec(&body).unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    synced.cleanup().await;
}

#[tokio::test]
async fn a_signed_github_ping_is_acknowledged_without_touching_the_task() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = serde_json::to_vec(&json!({ "zen": "Design for failure.", "hook_id": 1 })).unwrap();
    let signature = format!("sha256={}", synced.sign(&body));

    let (status, response) = synced
        .deliver(
            "github",
            &[
                (GITHUB_SIGNATURE_HEADER, &signature),
                (GITHUB_EVENT_HEADER, "ping"),
            ],
            &body,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["success"], json!(true), "{response}");
    assert!(response["message"].is_string(), "{response}");
    assert_eq!(synced.task().await.title, "Original");
    assert!(synced.item_event_types().await.is_empty());
    assert_eq!(
        synced.event_types().await,
        vec![SyncEventType::WebhookReceived]
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn an_unsigned_github_ping_is_unauthorized() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = serde_json::to_vec(&json!({ "zen": "Design for failure.", "hook_id": 1 })).unwrap();

    let status = synced
        .post("github", &[(GITHUB_EVENT_HEADER, "ping")], &body)
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(synced.event_types().await.is_empty());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_signed_github_event_other_than_issues_leaves_the_task_unchanged() {
    let synced = SyncedTask::create("github", github_config(), SyncDirection::Bidirectional).await;
    let body = serde_json::to_vec(&github_issue("edited", "Renamed")).unwrap();
    let signature = format!("sha256={}", synced.sign(&body));

    let status = synced
        .post(
            "github",
            &[
                (GITHUB_SIGNATURE_HEADER, &signature),
                (GITHUB_EVENT_HEADER, "issue_comment"),
            ],
            &body,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(synced.task().await.title, "Original");
    assert!(synced.item_event_types().await.is_empty());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_signed_linear_comment_or_project_update_leaves_the_synced_task_unchanged() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;

    for entity in ["Comment", "Project"] {
        let status = synced
            .post_linear(&json!({
                "action": "update",
                "type": entity,
                "data": {
                    "id": "123",
                    "body": "A comment",
                    "name": "A project",
                    "description": "Not the task's description"
                },
                "webhookTimestamp": now_milliseconds()
            }))
            .await;

        assert_eq!(status, StatusCode::OK, "{entity}");
    }

    let task = synced.task().await;
    assert_eq!(task.title, "Original");
    assert_eq!(task.description, "Original description");
    assert!(synced.item_event_types().await.is_empty());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_linear_delivery_sent_over_a_minute_ago_is_unauthorized() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let mut body = linear_issue_update();
    body["webhookTimestamp"] = json!(now_milliseconds() - 120_000);

    let status = synced.post_linear(&body).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(synced.task().await.title, "Original");
    assert!(synced.event_types().await.is_empty());

    synced.cleanup().await;
}

fn linear_issue_at(title: &str, state: &str, updated_at: &str) -> serde_json::Value {
    let mut update = linear_issue_update();
    update["data"]["title"] = json!(title);
    update["data"]["state"] = json!({ "type": state, "name": state });
    update["data"]["updatedAt"] = json!(updated_at);
    update
}

#[tokio::test]
async fn a_linear_update_moves_the_task_only_when_the_issue_state_changes() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let first = linear_issue_at("Renamed", "started", "2026-01-01T00:00:01.000Z");
    assert_eq!(synced.post_linear(&first).await, StatusCode::OK);
    synced.set_task("Renamed", "complete").await;

    let same_state = linear_issue_at("Renamed again", "started", "2026-01-01T00:00:02.000Z");
    assert_eq!(synced.post_linear(&same_state).await, StatusCode::OK);
    let task = synced.task().await;
    assert_eq!(task.title, "Renamed again");
    assert_eq!(task.status, "complete");

    let moved = linear_issue_at("Renamed again", "backlog", "2026-01-01T00:00:03.000Z");
    assert_eq!(synced.post_linear(&moved).await, StatusCode::OK);
    assert_eq!(synced.task().await.status, "created");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_linear_update_older_than_the_last_one_applied_is_dropped() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let newer = linear_issue_at("Newer", "started", "2026-01-01T00:00:02.000Z");
    assert_eq!(synced.post_linear(&newer).await, StatusCode::OK);
    synced.set_task("Newer", "in_progress").await;

    let older = linear_issue_at("Older", "completed", "2026-01-01T00:00:01.000Z");
    assert_eq!(synced.post_linear(&older).await, StatusCode::OK);

    let task = synced.task().await;
    assert_eq!(task.title, "Newer");
    assert_eq!(task.status, "in_progress");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_linear_update_from_another_project_leaves_the_linked_task_unchanged() {
    let synced = SyncedTask::create("linear", linear_config(), SyncDirection::Bidirectional).await;
    let mut foreign = linear_issue_update();
    foreign["data"]["projectId"] = json!("7d1f2a9b-0000-4c3e-8f6a-1b2c3d4e5f60");

    assert_eq!(synced.post_linear(&foreign).await, StatusCode::OK);

    assert_eq!(synced.task().await.title, "Original");
    assert!(synced.item_event_types().await.is_empty());

    synced.cleanup().await;
}

fn linear_config() -> serde_json::Value {
    json!({
        "api_key": "lin_api_test123",
        "team_id": "TEAM-123",
        "external_project_id": LINEAR_PROJECT
    })
}

fn linear_issue_update() -> serde_json::Value {
    json!({
        "action": "update",
        "type": "Issue",
        "data": {
            "id": "123",
            "title": "Renamed",
            "description": "Edited description",
            "projectId": LINEAR_PROJECT,
            "state": { "type": "started", "name": "In Progress" }
        },
        "webhookTimestamp": now_milliseconds()
    })
}

const NEW_ISSUE: &str = "456";
const NEW_ISSUE_URL: &str = "https://github.com/test-owner/test-repo/issues/456";
const NEW_ISSUE_TITLE: &str = "Crash on save";
const NEW_ISSUE_BODY: &str = "Steps to reproduce";
const LINEAR_PROJECT: &str = "0b6f3c2e-6f1a-4d8e-9a57-2f8c1d7e4b10";

fn configured_github(direction: &str) -> serde_json::Value {
    json!({
        "direction": direction,
        "external_repo_url": "https://GitHub.com/Test-Owner/Test-Repo/",
        "external_project_id": null
    })
}

fn opened_issue(association: &str, repository: &str) -> serde_json::Value {
    json!({
        "action": "opened",
        "issue": {
            "number": 456,
            "title": NEW_ISSUE_TITLE,
            "body": NEW_ISSUE_BODY,
            "state": "open",
            "html_url": NEW_ISSUE_URL,
            "author_association": association,
            "user": { "login": "someone" }
        },
        "repository": {
            "full_name": repository,
            "html_url": format!("https://github.com/{repository}")
        },
        "sender": { "login": "someone" }
    })
}

fn configured_linear(direction: &str) -> serde_json::Value {
    json!({
        "direction": direction,
        "external_repo_url": null,
        "external_project_id": LINEAR_PROJECT
    })
}

fn created_linear_issue(entity: &str, project_id: &str) -> serde_json::Value {
    json!({
        "action": "create",
        "type": entity,
        "data": {
            "id": NEW_ISSUE,
            "title": NEW_ISSUE_TITLE,
            "description": NEW_ISSUE_BODY,
            "projectId": project_id,
            "url": "https://linear.app/acme/issue/ACME-456/crash-on-save",
            "state": { "type": "backlog", "name": "Backlog" }
        },
        "webhookTimestamp": now_milliseconds()
    })
}

#[tokio::test]
async fn a_signed_github_issue_opened_by_a_member_of_the_configured_repository_becomes_a_linked_non_agentic_task()
 {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    let status = synced
        .post_github(&opened_issue("MEMBER", "test-owner/test-repo"))
        .await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.new_task().await;
    assert_eq!(task.title, NEW_ISSUE_TITLE);
    assert_eq!(task.description, NEW_ISSUE_BODY);
    assert!(
        !task.is_agentic,
        "a task from a webhook never runs an agent"
    );
    assert_eq!(task.project_ids, vec![synced.project_id]);
    let item = synced
        .linked(NEW_ISSUE)
        .await
        .expect("the new issue is linked to its task");
    assert_eq!(item.task_id, task.id);
    assert_eq!(item.external_url.as_deref(), Some(NEW_ISSUE_URL));
    assert_eq!(item.sync_direction, SyncDirection::Inbound);
    let created: Vec<_> =
        sync_config::list_sync_events(synced.state.db(), synced.sync_config_id, 100)
            .await
            .expect("Failed to read sync events")
            .into_iter()
            .filter(|event| event.event_type == SyncEventType::Create)
            .collect();
    assert_eq!(created.len(), 1, "{created:?}");
    assert_eq!(created[0].synced_item_id, Some(item.id));
    assert_eq!(created[0].direction, SyncEventDirection::Inbound);

    synced.cleanup().await;
}

#[tokio::test]
async fn an_owner_or_collaborator_opening_an_issue_also_creates_a_task() {
    for association in ["OWNER", "COLLABORATOR"] {
        let synced = SyncedTask::create(
            "github",
            configured_github("bidirectional"),
            SyncDirection::Bidirectional,
        )
        .await;

        let status = synced
            .post_github(&opened_issue(association, "Test-Owner/Test-Repo"))
            .await;

        assert_eq!(status, StatusCode::OK, "{association}");
        assert_eq!(synced.new_tasks().await.len(), 1, "{association}");
        assert_eq!(
            synced
                .linked(NEW_ISSUE)
                .await
                .map(|item| item.sync_direction),
            Some(SyncDirection::Bidirectional),
            "{association}"
        );

        synced.cleanup().await;
    }
}

#[tokio::test]
async fn redelivering_an_opened_issue_keeps_one_task_and_one_link() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let body = opened_issue("MEMBER", "test-owner/test-repo");

    assert_eq!(synced.post_github(&body).await, StatusCode::OK);
    assert_eq!(synced.post_github(&body).await, StatusCode::OK);
    let (first, second) = tokio::join!(synced.post_github(&body), synced.post_github(&body));

    assert_eq!((first, second), (StatusCode::OK, StatusCode::OK));
    assert_eq!(synced.new_tasks().await.len(), 1);
    assert_eq!(
        synced.link_count().await,
        2,
        "the fixture's link and the new one"
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn issues_opened_at_the_same_moment_race_to_one_task_and_one_link() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let body = opened_issue("MEMBER", "test-owner/test-repo");

    let statuses = futures::future::join_all((0..4).map(|_| synced.post_github(&body))).await;

    assert!(
        statuses.iter().all(|status| *status == StatusCode::OK),
        "{statuses:?}"
    );
    assert_eq!(synced.new_tasks().await.len(), 1);
    assert_eq!(synced.link_count().await, 2);

    synced.cleanup().await;
}

fn opened_issue_at(updated_at: &str) -> serde_json::Value {
    let mut issue = opened_issue("MEMBER", "test-owner/test-repo");
    issue["issue"]["created_at"] = json!(chrono::Utc::now().to_rfc3339());
    issue["issue"]["updated_at"] = json!(updated_at);
    issue
}

fn with_action(issue: &serde_json::Value, action: &str) -> serde_json::Value {
    let mut changed = issue.clone();
    changed["action"] = json!(action);
    changed
}

#[tokio::test]
async fn an_issue_opened_and_closed_in_the_same_second_becomes_a_complete_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let opened = opened_issue_at("2026-01-01T00:00:10Z");
    let mut closed = with_action(&opened, "closed");
    closed["issue"]["state"] = json!("closed");

    assert_eq!(synced.post_github(&opened).await, StatusCode::OK);
    let (status, response) = synced.send_github(&closed, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(synced.new_task().await.status, "complete", "{response}");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_replayed_opened_body_after_the_issue_was_deleted_creates_no_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let opened = opened_issue_at("2026-01-01T00:00:10Z");
    let (status, response) = synced.send_github(&opened, Some("opened-delivery")).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let deleted = with_action(&opened, "deleted");
    let (status, response) = synced.send_github(&deleted, Some("deleted-delivery")).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(synced.linked(NEW_ISSUE).await.is_none(), "{response}");

    let (status, response) = synced.send_github(&opened, Some("replayed-delivery")).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let (status, response) = synced.send_github(&opened, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");

    assert_eq!(
        synced.new_tasks().await.len(),
        1,
        "only the first opening made a task: {response}"
    );
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn an_opened_body_for_an_issue_opened_over_a_day_ago_creates_no_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let mut opened = opened_issue_at("2026-01-01T00:00:10Z");
    let long_ago = chrono::Utc::now() - chrono::TimeDelta::hours(25);
    opened["issue"]["created_at"] = json!(long_ago.to_rfc3339());

    let (status, response) = synced.send_github(&opened, None).await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(synced.new_tasks().await.is_empty(), "{response}");
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

async fn until_waiting_on_an_issue_or_done<T>(
    pool: &sqlx::PgPool,
    delivery: &tokio::task::JoinHandle<T>,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if delivery.is_finished() {
            return;
        }
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity
             WHERE datname = current_database()
               AND pid <> pg_backend_pid()
               AND wait_event_type = 'Lock'
               AND wait_event = 'advisory'
               AND query ILIKE '%pg_advisory_xact_lock%'",
        )
        .fetch_one(pool)
        .await
        .expect("pg_stat_activity is readable");
        if waiting > 0 {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the edit neither finished nor waited on its issue"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn an_edit_racing_the_delivery_that_opens_its_issue_waits_for_the_link_and_applies() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let workspace_id = synced.task().await.workspace_id;
    let mut opening = synced
        .state
        .db()
        .begin()
        .await
        .expect("Failed to begin the opening delivery");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2, 0))")
        .bind(synced.sync_config_id)
        .bind(NEW_ISSUE)
        .execute(&mut *opening)
        .await
        .expect("Failed to hold the issue as its opening delivery does");
    sync_config::create_synced_task(
        &mut opening,
        sync_config::NewSyncedTask {
            sync_config_id: synced.sync_config_id,
            workspace_id,
            project_id: synced.project_id,
            title: NEW_ISSUE_TITLE,
            description: NEW_ISSUE_BODY,
            external_id: NEW_ISSUE,
            external_url: Some(NEW_ISSUE_URL),
            sync_direction: SyncDirection::Inbound,
            last_external_state: Some(json!({
                "title": NEW_ISSUE_TITLE,
                "description": NEW_ISSUE_BODY,
                "state": "open",
                "updated_at": "2026-01-01T00:00:10Z"
            })),
        },
    )
    .await
    .expect("Failed to link the new issue")
    .expect("nothing else links the new issue");

    let mut edited = with_action(&opened_issue_at("2026-01-01T00:00:20Z"), "edited");
    edited["issue"]["title"] = json!("Renamed while opening");
    let body = serde_json::to_vec(&edited).unwrap();
    let signature = format!("sha256={}", synced.sign(&body));
    let state = synced.state.clone();
    let sync_config_id = synced.sync_config_id;
    let edit = tokio::spawn(async move {
        deliver_to(
            &state,
            sync_config_id,
            "github",
            &[
                (GITHUB_SIGNATURE_HEADER, signature.as_str()),
                (GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT),
                (GITHUB_DELIVERY_HEADER, "edit-delivery"),
            ],
            &body,
        )
        .await
    });
    until_waiting_on_an_issue_or_done(synced.state.db(), &edit).await;
    opening
        .commit()
        .await
        .expect("Failed to commit the opening delivery");
    let (status, response) = edit.await.expect("the edit delivery panicked");

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        synced.new_task().await.title,
        "Renamed while opening",
        "the edit waited for the link instead of finding none: {response}"
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_github_issue_deleted_before_its_opening_arrives_never_becomes_a_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let opened = opened_issue_at("2026-01-01T00:00:10Z");

    let (status, response) = synced
        .send_github(&with_action(&opened, "deleted"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        response["message"],
        "Issue was deleted before it was linked and never becomes a task"
    );
    assert!(
        synced.event_types().await.contains(&SyncEventType::Unlink),
        "the deletion is logged as an unlink"
    );

    let (status, response) = synced.send_github(&opened, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        response["message"],
        "Issue was unlinked from its task and is not linked again"
    );
    assert!(synced.new_tasks().await.is_empty());
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_deletion_from_another_repository_leaves_the_issue_free_to_become_a_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let foreign = with_action(&opened_issue("MEMBER", "someone-else/test-repo"), "deleted");
    let (status, response) = synced.send_github(&foreign, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(!synced.was_unlinked(NEW_ISSUE).await, "{response}");

    let (status, response) = synced
        .send_github(&opened_issue_at("2026-01-01T00:00:10Z"), None)
        .await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(synced.new_task().await.title, NEW_ISSUE_TITLE);

    synced.cleanup().await;
}

#[tokio::test]
async fn a_linear_issue_removed_before_its_creation_arrives_never_becomes_a_task() {
    let synced = SyncedTask::create(
        "linear",
        configured_linear("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let created = created_linear_issue("Issue", LINEAR_PROJECT);
    let mut foreign = created_linear_issue("Issue", "7d1f2a9b-0000-4c3e-8f6a-1b2c3d4e5f60");
    foreign["action"] = json!("remove");
    let (status, response) = synced.send_linear(&foreign, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(
        !synced.was_unlinked(NEW_ISSUE).await,
        "a removal from another project records nothing: {response}"
    );

    let mut removed = created.clone();
    removed["action"] = json!("remove");
    let (status, response) = synced.send_linear(&removed, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        response["message"],
        "Issue was deleted before it was linked and never becomes a task"
    );

    let (status, response) = synced.send_linear(&created, None).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(synced.new_tasks().await.is_empty(), "{response}");
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn linking_an_issue_another_delivery_already_linked_leaves_no_orphan_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let workspace_id = synced.task().await.workspace_id;
    let mut connection = synced
        .state
        .db()
        .acquire()
        .await
        .expect("Failed to acquire a connection");

    let item = sync_config::create_synced_task(
        &mut connection,
        sync_config::NewSyncedTask {
            sync_config_id: synced.sync_config_id,
            workspace_id,
            project_id: synced.project_id,
            title: "Duplicate",
            description: "",
            external_id: "123",
            external_url: None,
            sync_direction: SyncDirection::Inbound,
            last_external_state: None,
        },
    )
    .await
    .expect("losing the race is not an error");

    assert!(item.is_none());
    assert!(synced.new_tasks().await.is_empty());
    assert_eq!(
        synced.linked("123").await.map(|item| item.task_id),
        Some(synced.task_id)
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_new_issue_with_an_overlong_title_and_description_becomes_a_task_cut_to_the_limits() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let mut opened = opened_issue("MEMBER", "test-owner/test-repo");
    opened["issue"]["title"] = json!(format!("a{}", "é".repeat(250)));
    opened["issue"]["body"] = json!(format!("a{}", "é".repeat(25_000)));

    let status = synced.post_github(&opened).await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.new_task().await;
    assert_eq!(task.title, format!("a{}", "é".repeat(249)));
    assert_eq!(task.description, format!("a{}", "é".repeat(24_999)));

    synced.cleanup().await;
}

#[tokio::test]
async fn an_outbound_sync_creates_no_task_for_a_new_issue() {
    let synced = SyncedTask::create(
        "github",
        configured_github("outbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    let status = synced
        .post_github(&opened_issue("MEMBER", "test-owner/test-repo"))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(synced.new_tasks().await.is_empty());
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn an_issue_in_another_repository_creates_no_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    for repository in ["someone-else/test-repo", "test-owner/test-repo-fork"] {
        let status = synced
            .post_github(&opened_issue("MEMBER", repository))
            .await;

        assert_eq!(status, StatusCode::OK, "{repository}");
    }
    let mut without_repository = opened_issue("MEMBER", "test-owner/test-repo");
    without_repository
        .as_object_mut()
        .unwrap()
        .remove("repository");
    assert_eq!(
        synced.post_github(&without_repository).await,
        StatusCode::OK
    );

    assert!(synced.new_tasks().await.is_empty());
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn an_issue_opened_by_someone_without_write_access_creates_no_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    for association in [
        "NONE",
        "CONTRIBUTOR",
        "FIRST_TIME_CONTRIBUTOR",
        "FIRST_TIMER",
        "MANNEQUIN",
        "SOMETHING_NEW",
    ] {
        let status = synced
            .post_github(&opened_issue(association, "test-owner/test-repo"))
            .await;

        assert_eq!(status, StatusCode::OK, "{association}");
    }
    let mut unstated = opened_issue("MEMBER", "test-owner/test-repo");
    unstated["issue"]
        .as_object_mut()
        .unwrap()
        .remove("author_association");
    assert_eq!(synced.post_github(&unstated).await, StatusCode::OK);

    assert!(synced.new_tasks().await.is_empty());
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

#[tokio::test]
async fn an_edit_of_an_issue_that_was_never_linked_creates_no_task() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let mut edited = opened_issue("MEMBER", "test-owner/test-repo");
    edited["action"] = json!("edited");

    let status = synced.post_github(&edited).await;

    assert_eq!(status, StatusCode::OK);
    assert!(synced.new_tasks().await.is_empty());

    synced.cleanup().await;
}

#[tokio::test]
async fn a_later_edit_of_a_new_issue_updates_the_task_it_became() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let opened = opened_issue("MEMBER", "test-owner/test-repo");
    assert_eq!(synced.post_github(&opened).await, StatusCode::OK);
    let mut edited = opened.clone();
    edited["action"] = json!("edited");
    edited["issue"]["title"] = json!("Crash on save as");
    edited["issue"]["author_association"] = json!("NONE");

    let status = synced.post_github(&edited).await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.new_task().await;
    assert_eq!(task.title, "Crash on save as");
    assert_eq!(task.description, NEW_ISSUE_BODY);
    assert!(!task.is_agentic);

    synced.cleanup().await;
}

#[tokio::test]
async fn a_webhook_answer_never_names_the_task_it_touched() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    let (status, created) = synced
        .send_github(&opened_issue("MEMBER", "test-owner/test-repo"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let (status, updated) = synced
        .send_github(&github_issue("edited", "Renamed"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let new_task = synced.new_task().await.id.to_string();
    assert!(!created.to_string().contains(&new_task), "{created}");
    assert!(
        !updated.to_string().contains(&synced.task_id.to_string()),
        "{updated}"
    );
    assert_eq!(synced.task().await.title, "Renamed");

    synced.cleanup().await;
}

#[tokio::test]
async fn a_new_issue_whose_create_event_cannot_be_logged_leaves_no_task_behind() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;
    let function = format!("refuse_create_event_{}", synced.sync_config_id.simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
           IF NEW.event_type = 'create' AND NEW.sync_config_id = '{}' THEN \
             RAISE EXCEPTION 'create events are refused'; \
           END IF; RETURN NEW; END $$; \
         CREATE TRIGGER {function} BEFORE INSERT ON sync_events \
           FOR EACH ROW EXECUTE FUNCTION {function}();",
        synced.sync_config_id
    )))
    .execute(synced.state.db())
    .await
    .expect("Failed to install the refusing trigger");
    let opened = opened_issue("MEMBER", "test-owner/test-repo");

    let status = synced.post_github(&opened).await;

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {function} ON sync_events; DROP FUNCTION {function}();"
    )))
    .execute(synced.state.db())
    .await
    .expect("Failed to remove the refusing trigger");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        synced.new_tasks().await.is_empty(),
        "no task outlives its failed delivery"
    );
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    assert_eq!(synced.post_github(&opened).await, StatusCode::OK);
    assert_eq!(
        synced.new_tasks().await.len(),
        1,
        "a retry creates the task"
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_task_created_from_an_issue_is_never_picked_to_run() {
    let synced = SyncedTask::create(
        "github",
        configured_github("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    let status = synced
        .post_github(&opened_issue("OWNER", "test-owner/test-repo"))
        .await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.new_task().await;
    assert_eq!(task.status, "created");
    assert_eq!(
        zone_server::db::auto_projects::next_runnable(synced.state.db(), synced.project_id)
            .await
            .expect("Failed to pick the next runnable task"),
        None
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_linear_issue_becomes_a_task_only_in_the_configured_project() {
    let synced = SyncedTask::create(
        "linear",
        configured_linear("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    let status = synced
        .post_linear(&created_linear_issue(
            "Issue",
            "7d1f2a9b-0000-4c3e-8f6a-1b2c3d4e5f60",
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let mut without_project = created_linear_issue("Issue", LINEAR_PROJECT);
    without_project["data"]
        .as_object_mut()
        .unwrap()
        .remove("projectId");
    assert_eq!(synced.post_linear(&without_project).await, StatusCode::OK);
    assert!(synced.new_tasks().await.is_empty());

    let status = synced
        .post_linear(&created_linear_issue("Issue", LINEAR_PROJECT))
        .await;

    assert_eq!(status, StatusCode::OK);
    let task = synced.new_task().await;
    assert_eq!(task.title, NEW_ISSUE_TITLE);
    assert_eq!(task.description, NEW_ISSUE_BODY);
    assert!(!task.is_agentic);
    let item = synced
        .linked(NEW_ISSUE)
        .await
        .expect("the new issue is linked to its task");
    assert_eq!(item.task_id, task.id);
    assert_eq!(
        item.external_url.as_deref(),
        Some("https://linear.app/acme/issue/ACME-456/crash-on-save")
    );

    synced.cleanup().await;
}

#[tokio::test]
async fn a_created_linear_comment_creates_no_task() {
    let synced = SyncedTask::create(
        "linear",
        configured_linear("inbound"),
        SyncDirection::Bidirectional,
    )
    .await;

    let status = synced
        .post_linear(&created_linear_issue("Comment", LINEAR_PROJECT))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(synced.new_tasks().await.is_empty());
    assert!(synced.linked(NEW_ISSUE).await.is_none());

    synced.cleanup().await;
}

// The console's External Sync section: configuration only, no engine yet.

async fn signed_in(client: &common::TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": common::test_email(), "password": common::test_password() }),
        )
        .await;
    response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn workspace_project(client: &common::TestClient, token: &str) -> (String, String) {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Sync Org", "slug": format!("sync-{}", Uuid::new_v4()) }),
            token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", organization),
            &json!({ "name": "Sync Workspace", "slug": format!("sync-{}", Uuid::new_v4()) }),
            token,
        )
        .await;
    let workspace = response.json_value()["workspace"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({ "workspace_id": workspace, "name": "Synced project" }),
            token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let project = response.json_value()["project"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    (workspace, project)
}

#[tokio::test]
async fn a_sync_is_configured_listed_and_removed_through_the_project() {
    let client = common::TestClient::with_db().await;
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/projects/{}/sync", project), &token)
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["configs"], json!([]));

    let response = client
        .post_json_auth(
            &format!("/api/projects/{}/sync", project),
            &json!({
                "provider": "github",
                "direction": "bidirectional",
                "external_repo_url": "https://github.com/abnegate/zone-tests"
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let config = response.json_value()["config"].clone();
    assert_eq!(config["provider"], "github");
    assert_eq!(config["direction"], "bidirectional");
    assert_eq!(
        config["external_repo_url"],
        "https://github.com/abnegate/zone-tests"
    );
    assert_eq!(config["is_active"], true);
    assert_eq!(config["status"], "configured");
    assert!(config["last_synced_at"].is_null());
    assert!(
        config["created_at"].as_str().unwrap().ends_with('Z'),
        "the console parses created_at as a UTC datetime: {}",
        config["created_at"]
    );
    let config_id = config["id"].as_str().unwrap().to_string();
    assert_eq!(
        config["webhook_path"],
        format!("/api/webhooks/sync/{config_id}/github")
    );

    let response = client
        .get_auth(&format!("/api/projects/{}/sync", project), &token)
        .await;
    let listed = response.json_value();
    assert_eq!(listed["configs"].as_array().unwrap().len(), 1);
    assert_eq!(listed["configs"][0]["id"], config_id);

    let response = client
        .post_json_auth(
            &format!("/api/projects/{}/sync", project),
            &json!({
                "provider": "github",
                "direction": "inbound",
                "external_repo_url": "https://github.com/abnegate/other"
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        response.json_value()["error"],
        "A github sync is already configured for this project; remove it first"
    );

    let response = client
        .post_json_auth(
            &format!("/api/projects/{}/sync", project),
            &json!({ "provider": "linear", "direction": "inbound" }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json_value()["error"],
        "A Linear sync needs external_project_id"
    );

    let response = client
        .delete_auth(
            &format!("/api/projects/{}/sync/{}", project, config_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NO_CONTENT);

    let response = client
        .get_auth(&format!("/api/projects/{}/sync", project), &token)
        .await;
    assert_eq!(response.json_value()["configs"], json!([]));
}

#[tokio::test]
async fn a_strangers_project_has_no_sync_to_read_or_write() {
    let client = common::TestClient::with_db().await;
    let owner = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &owner).await;
    let stranger = signed_in(&client).await;

    let response = client
        .get_auth(&format!("/api/projects/{}/sync", project), &stranger)
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let response = client
        .post_json_auth(
            &format!("/api/projects/{}/sync", project),
            &json!({
                "provider": "github",
                "direction": "outbound",
                "external_repo_url": "https://github.com/abnegate/zone-tests"
            }),
            &stranger,
        )
        .await;
    response.assert_status(StatusCode::NOT_FOUND);

    let response = client
        .get_auth(&format!("/api/projects/{}/sync", project), &owner)
        .await;
    assert_eq!(response.json_value()["configs"], json!([]));
}

// Webhook secrets: issued for GitHub, set by hand for Linear, rotated on request.

async fn registered(client: &common::TestClient) -> (String, Uuid) {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": common::test_email(), "password": common::test_password() }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    let token = body["access_token"].as_str().unwrap().to_string();
    let user = Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap();
    (token, user)
}

fn github_sync() -> serde_json::Value {
    json!({
        "provider": "github",
        "direction": "inbound",
        "external_repo_url": "https://github.com/abnegate/zone-tests"
    })
}

fn linear_sync() -> serde_json::Value {
    json!({ "provider": "linear", "direction": "inbound", "external_project_id": "LIN-1" })
}

async fn configure(
    client: &common::TestClient,
    token: &str,
    project: &str,
    body: &serde_json::Value,
) -> serde_json::Value {
    let response = client
        .post_json_auth(&format!("/api/projects/{project}/sync"), body, token)
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()
}

async fn set_secret(
    client: &common::TestClient,
    token: &str,
    project: &str,
    config: &str,
    body: &serde_json::Value,
) -> common::TestResponse {
    client
        .put_json_auth(
            &format!("/api/projects/{project}/sync/{config}/webhook-secret"),
            body,
            token,
        )
        .await
}

async fn listed_config(
    client: &common::TestClient,
    token: &str,
    project: &str,
) -> common::TestResponse {
    let response = client
        .get_auth(&format!("/api/projects/{project}/sync"), token)
        .await;
    response.assert_status(StatusCode::OK);
    response
}

fn signature_for(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("Failed to create HMAC");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

async fn deliver_github(
    client: &common::TestClient,
    config: &str,
    secret: &str,
) -> common::TestResponse {
    let body = serde_json::to_vec(&json!({
        "action": "opened",
        "issue": {
            "number": 4242,
            "title": "Filed on GitHub",
            "body": null,
            "state": "open",
            "html_url": "https://github.com/abnegate/zone-tests/issues/4242"
        }
    }))
    .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{config}/github"))
        .header("Content-Type", "application/json")
        .header(GITHUB_EVENT_HEADER, GITHUB_ISSUES_EVENT)
        .header(
            GITHUB_SIGNATURE_HEADER,
            format!("sha256={}", signature_for(secret, &body)),
        )
        .body(Body::from(body))
        .unwrap();
    client.send_request(request).await
}

async fn deliver_linear(
    client: &common::TestClient,
    config: &str,
    secret: &str,
) -> common::TestResponse {
    let body = serde_json::to_vec(&json!({
        "action": "update",
        "type": "Issue",
        "data": {
            "id": "lin-issue-4242",
            "title": "Filed on Linear",
            "state": { "type": "started", "name": "In Progress" }
        },
        "webhookTimestamp": now_milliseconds()
    }))
    .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{config}/linear"))
        .header("Content-Type", "application/json")
        .header("Linear-Event", "Issue")
        .header(LINEAR_SIGNATURE_HEADER, signature_for(secret, &body))
        .body(Body::from(body))
        .unwrap();
    client.send_request(request).await
}

fn assert_verified(response: &common::TestResponse) {
    assert!(
        !response.text().contains("Webhook secret not configured"),
        "the sync has a secret to verify deliveries with: {}",
        response.text()
    );
    assert_eq!(
        response.status,
        StatusCode::OK,
        "a delivery signed with the configured secret is accepted: {}",
        response.text()
    );
}

fn is_generated_secret(secret: &str) -> bool {
    secret.len() == 64
        && secret
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

#[tokio::test]
async fn a_github_sync_is_issued_a_secret_its_deliveries_verify_against() {
    let client = common::TestClient::with_db().await;
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;

    let created = configure(&client, &token, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let secret = created["webhook_secret"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    assert_verified(&deliver_github(&client, &config, &secret).await);
    assert!(
        is_generated_secret(&secret),
        "the secret is 32 random bytes in hex: {secret:?}"
    );
    assert_eq!(created["config"]["webhook_secret_configured"], true);
    assert_eq!(created["config"]["webhook_secret_issued_by_zone"], true);

    let listed = listed_config(&client, &token, &project).await;
    assert_eq!(
        listed.json_value()["configs"][0]["webhook_secret_configured"],
        true
    );
    assert!(
        !listed.text().contains(&secret),
        "a listing never shows the secret: {}",
        listed.text()
    );
}

#[tokio::test]
async fn a_linear_sync_verifies_deliveries_with_the_signing_secret_linear_issued() {
    let client = common::TestClient::with_db().await;
    let signing_secret = generate_token();
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;

    let created = configure(&client, &token, &project, &linear_sync()).await;
    assert!(
        created["webhook_secret"].is_null(),
        "Linear issues its own signing secret, so Zone generates none: {created}"
    );
    assert_eq!(created["config"]["webhook_secret_configured"], false);
    assert_eq!(created["config"]["webhook_secret_issued_by_zone"], false);
    let config = created["config"]["id"].as_str().unwrap().to_string();

    let response = set_secret(
        &client,
        &token,
        &project,
        &config,
        &json!({ "secret": format!("  {signing_secret}  ") }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(
        body["webhook_secret"].is_null(),
        "a supplied secret is never echoed back: {body}"
    );
    assert!(!response.text().contains(&signing_secret));
    assert_eq!(body["config"]["id"], config);
    assert_eq!(body["config"]["webhook_secret_configured"], true);

    assert_verified(&deliver_linear(&client, &config, &signing_secret).await);
    let listed = listed_config(&client, &token, &project).await;
    assert_eq!(
        listed.json_value()["configs"][0]["webhook_secret_configured"],
        true
    );
}

#[tokio::test]
async fn rotating_a_webhook_secret_retires_the_old_one() {
    let client = common::TestClient::with_db().await;
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;
    let created = configure(&client, &token, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let old = created["webhook_secret"].as_str().unwrap().to_string();

    let response = set_secret(&client, &token, &project, &config, &json!({})).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let new = body["webhook_secret"].as_str().unwrap().to_string();
    assert!(
        is_generated_secret(&new),
        "a rotated secret is generated: {new:?}"
    );
    assert_ne!(new, old);
    assert_eq!(body["config"]["id"], config);
    assert_eq!(body["config"]["webhook_secret_configured"], true);

    deliver_github(&client, &config, &old)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    assert_verified(&deliver_github(&client, &config, &new).await);
}

#[tokio::test]
async fn a_blank_or_short_webhook_secret_is_refused_and_not_stored() {
    let client = common::TestClient::with_db().await;
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;
    let created = configure(&client, &token, &project, &linear_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();

    for secret in ["  ", "too-short"] {
        let response = set_secret(
            &client,
            &token,
            &project,
            &config,
            &json!({ "secret": secret }),
        )
        .await;
        response.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json_value()["error"],
            "A webhook secret must be 16 to 256 characters"
        );
    }

    let listed = listed_config(&client, &token, &project).await;
    assert_eq!(
        listed.json_value()["configs"][0]["webhook_secret_configured"],
        false
    );
}

#[tokio::test]
async fn another_projects_sync_has_no_webhook_secret_to_set() {
    let client = common::TestClient::with_db().await;
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;
    let (_other_workspace, other_project) = workspace_project(&client, &token).await;
    let created = configure(&client, &token, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let secret = created["webhook_secret"].as_str().unwrap().to_string();

    let response = set_secret(&client, &token, &other_project, &config, &json!({})).await;
    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(
        response.json_value()["error"],
        "Sync configuration not found"
    );

    assert_verified(&deliver_github(&client, &config, &secret).await);
}

#[tokio::test]
async fn a_read_only_member_cannot_set_a_webhook_secret() {
    use zone_server::db::workspace_members::{self, WorkspaceRole};

    let client = common::TestClient::with_db().await;
    let (owner, _owner_id) = registered(&client).await;
    let (workspace, project) = workspace_project(&client, &owner).await;
    let created = configure(&client, &owner, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let secret = created["webhook_secret"].as_str().unwrap().to_string();
    let (viewer, viewer_id) = registered(&client).await;
    workspace_members::add_member(
        client.state().db(),
        Uuid::parse_str(&workspace).unwrap(),
        viewer_id,
        WorkspaceRole::Viewer,
        None,
    )
    .await
    .expect("the workspace takes the viewer");

    listed_config(&client, &viewer, &project).await;
    for body in [json!({}), json!({ "secret": "viewer-chosen-secret-123" })] {
        set_secret(&client, &viewer, &project, &config, &body)
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }

    assert_verified(&deliver_github(&client, &config, &secret).await);
}

// Rotating or setting a secret: admins only, atomically, on the record.

async fn joined(
    client: &common::TestClient,
    workspace: &str,
    role: zone_server::db::workspace_members::WorkspaceRole,
) -> (String, Uuid) {
    let (token, user) = registered(client).await;
    zone_server::db::workspace_members::add_member(
        client.state().db(),
        Uuid::parse_str(workspace).unwrap(),
        user,
        role,
        None,
    )
    .await
    .expect("the workspace takes the member");
    (token, user)
}

/// Wait until another connection's UPDATE of `sync_configs` is queued behind
/// a row lock, so the transaction holding that lock can commit under it.
async fn until_an_update_waits_on_a_sync_config(pool: &sqlx::PgPool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity
             WHERE datname = current_database()
               AND pid <> pg_backend_pid()
               AND wait_event_type = 'Lock'
               AND query ILIKE '%UPDATE sync_configs%'",
        )
        .fetch_one(pool)
        .await
        .expect("pg_stat_activity is readable");
        if waiting > 0 {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the rotation never reached its UPDATE"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

async fn updated_at(pool: &sqlx::PgPool, config: &str) -> chrono::NaiveDateTime {
    sqlx::query_scalar("SELECT updated_at FROM sync_configs WHERE id = $1")
        .bind(Uuid::parse_str(config).unwrap())
        .fetch_one(pool)
        .await
        .expect("the configuration has an updated_at")
}

async fn audited(
    pool: &sqlx::PgPool,
    config: &str,
) -> Vec<(String, Option<Uuid>, serde_json::Value)> {
    sqlx::query_as(
        "SELECT action, actor_id, COALESCE(new_values, 'null'::jsonb)
         FROM audit_logs
         WHERE resource_id = $1 AND action IN ('sync.webhook_secret_rotated', 'sync.webhook_secret_set')
         ORDER BY created_at",
    )
    .bind(Uuid::parse_str(config).unwrap())
    .fetch_all(pool)
    .await
    .expect("audit_logs is readable")
}

#[tokio::test]
async fn a_member_who_is_not_an_admin_cannot_set_or_rotate_a_webhook_secret() {
    use zone_server::db::workspace_members::WorkspaceRole;

    let client = common::TestClient::with_db().await;
    let (owner, _owner_id) = registered(&client).await;
    let (workspace, project) = workspace_project(&client, &owner).await;
    let created = configure(&client, &owner, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let secret = created["webhook_secret"].as_str().unwrap().to_string();
    let (member, _member_id) = joined(&client, &workspace, WorkspaceRole::Member).await;

    listed_config(&client, &member, &project).await;
    for body in [json!({}), json!({ "secret": "member-chosen-secret-123" })] {
        let response = set_secret(&client, &member, &project, &config, &body).await;
        response.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(
            response.json_value()["error"],
            "Workspace admin access required"
        );
    }
    assert!(audited(client.state().db(), &config).await.is_empty());
    assert_verified(&deliver_github(&client, &config, &secret).await);

    let (admin, _admin_id) = joined(&client, &workspace, WorkspaceRole::Admin).await;
    set_secret(&client, &admin, &project, &config, &json!({}))
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn a_rotation_that_read_a_secret_replaced_meanwhile_answers_conflict_and_shows_nothing() {
    let client = common::TestClient::with_db().await;
    let intervening_secret = generate_token();
    let token = signed_in(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;
    let created = configure(&client, &token, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let pool = client.state().db().clone();
    let intervening = crypto::encrypt(client.state().encryption_key(), &intervening_secret)
        .expect("the secret encrypts");

    let mut transaction = pool.begin().await.expect("a transaction begins");
    sqlx::query("UPDATE sync_configs SET webhook_secret_encrypted = $2 WHERE id = $1")
        .bind(Uuid::parse_str(&config).unwrap())
        .bind(&intervening)
        .execute(&mut *transaction)
        .await
        .expect("the intervening secret is written");
    let generate = json!({});
    let rotation = set_secret(&client, &token, &project, &config, &generate);
    let intervention = async {
        until_an_update_waits_on_a_sync_config(&pool).await;
        transaction
            .commit()
            .await
            .expect("the intervening secret commits");
    };
    let (response, ()) = tokio::join!(rotation, intervention);

    response.assert_status(StatusCode::CONFLICT);
    let body = response.json_value();
    assert!(
        body.get("webhook_secret").is_none() && body.get("config").is_none(),
        "a refused rotation shows no secret: {body}"
    );
    assert_eq!(
        body["error"],
        "The sync configuration changed while this request was replacing its webhook secret; reload and try again"
    );
    assert_verified(&deliver_github(&client, &config, &intervening_secret).await);
    assert!(audited(&pool, &config).await.is_empty());
}

#[tokio::test]
async fn setting_or_rotating_a_webhook_secret_moves_updated_at_and_records_who_did_it() {
    let client = common::TestClient::with_db().await;
    let signing_secret = generate_token();
    let (token, user) = registered(&client).await;
    let (_workspace, project) = workspace_project(&client, &token).await;
    let pool = client.state().db().clone();

    let github = configure(&client, &token, &project, &github_sync()).await;
    let github = github["config"]["id"].as_str().unwrap().to_string();
    let before = updated_at(&pool, &github).await;
    let response = set_secret(&client, &token, &project, &github, &json!({})).await;
    response.assert_status(StatusCode::OK);
    let rotated = response.json_value()["webhook_secret"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(updated_at(&pool, &github).await > before);
    let entries = audited(&pool, &github).await;
    assert_eq!(entries.len(), 1, "one rotation, one entry: {entries:?}");
    let (action, actor, values) = &entries[0];
    assert_eq!(action, "sync.webhook_secret_rotated");
    assert_eq!(*actor, Some(user));
    assert_eq!(values["provider"], "github");
    assert!(
        !values.to_string().contains(&rotated),
        "the audit log never holds the secret: {values}"
    );

    let linear = configure(&client, &token, &project, &linear_sync()).await;
    let linear = linear["config"]["id"].as_str().unwrap().to_string();
    let before = updated_at(&pool, &linear).await;
    set_secret(
        &client,
        &token,
        &project,
        &linear,
        &json!({ "secret": signing_secret }),
    )
    .await
    .assert_status(StatusCode::OK);
    assert!(updated_at(&pool, &linear).await > before);
    let entries = audited(&pool, &linear).await;
    assert_eq!(entries.len(), 1, "one change, one entry: {entries:?}");
    let (action, actor, values) = &entries[0];
    assert_eq!(action, "sync.webhook_secret_set");
    assert_eq!(*actor, Some(user));
    assert_eq!(values["provider"], "linear");
    assert!(!values.to_string().contains(&signing_secret));
}

// Adding or removing a sync: admins only, on the record.

async fn remove(
    client: &common::TestClient,
    token: &str,
    project: &str,
    config: &str,
) -> common::TestResponse {
    client
        .delete_auth(&format!("/api/projects/{project}/sync/{config}"), token)
        .await
}

async fn recorded(
    pool: &sqlx::PgPool,
    config: &str,
) -> Vec<(String, Option<Uuid>, serde_json::Value)> {
    sqlx::query_as(
        "SELECT action, actor_id, COALESCE(new_values, old_values, 'null'::jsonb)
         FROM audit_logs WHERE resource_id = $1 ORDER BY created_at",
    )
    .bind(Uuid::parse_str(config).unwrap())
    .fetch_all(pool)
    .await
    .expect("audit_logs is readable")
}

#[tokio::test]
async fn a_member_who_is_not_an_admin_cannot_add_or_remove_a_sync() {
    use zone_server::db::workspace_members::WorkspaceRole;

    let client = common::TestClient::with_db().await;
    let (owner, _owner_id) = registered(&client).await;
    let (workspace, project) = workspace_project(&client, &owner).await;
    let created = configure(&client, &owner, &project, &github_sync()).await;
    let config = created["config"]["id"].as_str().unwrap().to_string();
    let secret = created["webhook_secret"].as_str().unwrap().to_string();
    let (member, _member_id) = joined(&client, &workspace, WorkspaceRole::Member).await;
    let pool = client.state().db().clone();
    let before = recorded(&pool, &config).await;

    let response = remove(&client, &member, &project, &config).await;
    response.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(
        response.json_value()["error"],
        "Workspace admin access required"
    );
    for body in [github_sync(), linear_sync()] {
        let response = client
            .post_json_auth(&format!("/api/projects/{project}/sync"), &body, &member)
            .await;
        response.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(
            response.json_value()["error"],
            "Workspace admin access required"
        );
    }

    let listed = listed_config(&client, &member, &project).await.json_value();
    let configs = listed["configs"].as_array().unwrap();
    assert_eq!(configs.len(), 1, "the member added nothing: {listed}");
    assert_eq!(configs[0]["id"], config);
    assert_eq!(recorded(&pool, &config).await, before);
    assert_verified(&deliver_github(&client, &config, &secret).await);
}

#[tokio::test]
async fn an_admin_adds_and_removes_a_sync_and_each_is_recorded_without_the_secret() {
    use zone_server::db::workspace_members::WorkspaceRole;

    let client = common::TestClient::with_db().await;
    let (owner, _owner_id) = registered(&client).await;
    let (workspace, project) = workspace_project(&client, &owner).await;
    let (admin, admin_id) = joined(&client, &workspace, WorkspaceRole::Admin).await;
    let pool = client.state().db().clone();

    for (body, provider) in [(github_sync(), "github"), (linear_sync(), "linear")] {
        let created = configure(&client, &admin, &project, &body).await;
        let config = created["config"]["id"].as_str().unwrap().to_string();
        let issued = created["webhook_secret"].as_str().map(str::to_string);

        remove(&client, &admin, &project, &config)
            .await
            .assert_status(StatusCode::NO_CONTENT);

        let entries = recorded(&pool, &config).await;
        let actions: Vec<&str> = entries
            .iter()
            .map(|(action, _, _)| action.as_str())
            .collect();
        assert_eq!(actions, ["sync.created", "sync.deleted"], "{entries:?}");
        for (_action, actor, values) in &entries {
            assert_eq!(*actor, Some(admin_id));
            assert_eq!(values["project_id"], project);
            assert_eq!(values["provider"], provider);
            if let Some(issued) = &issued {
                assert!(
                    !values.to_string().contains(issued.as_str()),
                    "the audit log never holds the secret: {values}"
                );
            }
        }
    }
    let listed = listed_config(&client, &admin, &project).await.json_value();
    assert_eq!(listed["configs"], json!([]));
}
