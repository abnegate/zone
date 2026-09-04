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
    db::{DbPool, projects, sync_config, tasks},
    routes::create_router,
    state::AppState,
};

type HmacSha256 = Hmac<Sha256>;

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
        litellm_host: "http://localhost:4000".to_string(),
        litellm_key: "test-key".to_string(),
        ollama_host: "http://localhost:11434".to_string(),
        gpt4all_models_url: zone_server::config::DEFAULT_GPT4ALL_MODELS_URL.to_string(),
        huggingface_models_url: zone_server::config::DEFAULT_HUGGINGFACE_MODELS_URL.to_string(),
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        web_search: Default::default(),
        comfyui: Default::default(),
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
        .execute(sqlx::query("DELETE FROM tasks WHERE project_id = $1").bind(project_id))
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
        "bidirectional",
        Some(external_state.clone()),
    )
    .await
    .expect("Failed to create synced item");

    assert_eq!(synced_item.external_id, "123");
    assert_eq!(synced_item.task_id, task.id);
    assert_eq!(synced_item.sync_direction, "bidirectional");

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
        "webhook_received",
        "inbound",
        Some(payload.clone()),
        None,
    )
    .await
    .expect("Failed to create sync event");

    assert_eq!(event.event_type, "webhook_received");
    assert_eq!(event.direction, "inbound");
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

    let webhook_secret = "test-webhook-secret";
    let encryption_key =
        crypto::derive_key(state.config().encryption_key()).expect("Failed to derive key");
    let encrypted_secret =
        crypto::encrypt(&encryption_key, webhook_secret).expect("Failed to encrypt");

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
        .header("X-Hub-Signature-256", signature)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 200 (or 500 if task processing fails, but not 401)
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);

    // Test with invalid signature
    let app = create_router(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{}/github", sync_config_row.id))
        .header("Content-Type", "application/json")
        .header("X-Hub-Signature-256", "sha256=invalid")
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

    let webhook_secret = "test-webhook-secret";
    let encryption_key =
        crypto::derive_key(state.config().encryption_key()).expect("Failed to derive key");
    let encrypted_secret =
        crypto::encrypt(&encryption_key, webhook_secret).expect("Failed to encrypt");

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
        }
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
        .header("Linear-Signature", signature)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 200 (or 500 if task processing fails, but not 401)
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);

    // Test with invalid signature
    let app = create_router(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/webhooks/sync/{}/linear", sync_config_row.id))
        .header("Content-Type", "application/json")
        .header("Linear-Signature", "invalid")
        .body(Body::from(payload_bytes))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 401 Unauthorized
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Cleanup
    cleanup_project(state.db(), project.id).await;
}
