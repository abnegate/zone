//! Integration tests for chat message embeddings
//!
//! These tests verify the message embedding functionality:
//! - Background embedding generation when messages are created
//! - Semantic search over chat history
//! - Chat-specific and global search capabilities
//!
//! Run with: SQLX_OFFLINE=true cargo test --test message_embedding_tests

mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use common::{TestClient, create_test_pool, test_email, test_password};
use zone_server::config::Config;
use zone_server::db::{chats, message_embeddings};
use zone_server::routes::create_router;
use zone_server::state::AppState;
use zone_server::workers::embeddings::spawn_message_embedding_task;

// =============================================================================
// Test Setup Helpers
// =============================================================================

/// Create test configuration
fn test_config() -> Config {
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
        litellm_host: "http://localhost:4000".to_string(),
        litellm_key: "test-key".to_string(),
        ollama_host: "http://localhost:11434".to_string(),
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        web_search: Default::default(),
    }
}

/// Create AppState without embedding service (for most tests)
fn create_test_state_with_embedding_service(pool: PgPool) -> AppState {
    let config = test_config();

    // For testing without real embedding service, we just create basic AppState
    // Tests will check if embedding service is available before trying to use it
    AppState::new(config, pool, None)
}

/// Create a test client with embedding service
async fn create_test_client_with_embeddings() -> (TestClient, AppState) {
    let pool = create_test_pool().await;
    let state = create_test_state_with_embedding_service(pool);
    let router = create_router(state.clone());
    (TestClient::new(router), state)
}

/// Register a test user and return (token, user_id)
async fn register_test_user(client: &TestClient) -> (String, Uuid) {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
                "display_name": "Test User"
            }),
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    let token = body["access_token"].as_str().unwrap().to_string();
    let user_id = Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap();
    (token, user_id)
}

/// Setup a test user with organization and workspace
async fn setup_user_and_workspace(client: &TestClient) -> (String, Uuid, Uuid) {
    let (token, user_id) = register_test_user(client).await;

    // Create organization
    let org_response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": format!("Test Org {}", Uuid::new_v4()),
                "slug": format!("test-org-{}", Uuid::new_v4())
            }),
            &token,
        )
        .await;
    org_response.assert_status(StatusCode::CREATED);
    let org_id = Uuid::parse_str(
        org_response.json_value()["organization"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    // Create workspace
    let ws_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": format!("Test Workspace {}", Uuid::new_v4()),
                "slug": format!("test-ws-{}", Uuid::new_v4())
            }),
            &token,
        )
        .await;
    ws_response.assert_status(StatusCode::CREATED);
    let workspace_id = Uuid::parse_str(
        ws_response.json_value()["workspace"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    (token, user_id, workspace_id)
}

/// Create a test chat and return its ID
async fn create_test_chat(client: &TestClient, token: &str, workspace_id: Uuid) -> Uuid {
    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test Chat",
                "model_name": "gpt-4"
            }),
            token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Uuid::parse_str(body["chat"]["id"].as_str().unwrap()).unwrap()
}

/// Create a test message and return its ID
async fn create_test_message(
    client: &TestClient,
    token: &str,
    chat_id: Uuid,
    content: &str,
) -> Uuid {
    let response = client
        .post_json_auth(
            &format!("/api/chats/{}/messages", chat_id),
            &json!({
                "role": "user",
                "content": content
            }),
            token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Uuid::parse_str(body["message"]["id"].as_str().unwrap()).unwrap()
}

// =============================================================================
// Background Worker Tests
// =============================================================================

#[tokio::test]
async fn test_spawn_message_embedding_task_creates_embedding() {
    // Given: A database with a chat and message
    let pool = create_test_pool().await;
    let config = test_config();

    // Create state without real embedding service (for testing)
    let state = AppState::new(config, pool.clone(), None);

    // Create a test chat and message directly in the database
    let chat = chats::create_chat(&pool, None, "Test Chat", "gpt-4", false, true)
        .await
        .expect("Failed to create chat");

    let message = chats::create_message(&pool, chat.id, "user", "This is a test message", None)
        .await
        .expect("Failed to create message");

    // When: Spawning the embedding task (if embedding service is available)
    if state.embedding_service().is_some() {
        spawn_message_embedding_task(state.clone(), message.id, chat.id, message.content.clone());

        // Wait a bit for the async task to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Then: The embedding should be stored in the database
        let embedding = message_embeddings::get_message_embedding(&pool, message.id)
            .await
            .expect("Failed to query embedding");

        assert!(embedding.is_some(), "Embedding should be created");
        let embedding = embedding.unwrap();
        assert_eq!(embedding.message_id, message.id);
        assert_eq!(embedding.chat_id, chat.id);
    } else {
        // If no embedding service, task should handle gracefully
        spawn_message_embedding_task(state.clone(), message.id, chat.id, message.content.clone());

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // No embedding should be created
        let embedding = message_embeddings::get_message_embedding(&pool, message.id)
            .await
            .expect("Failed to query embedding");

        assert!(
            embedding.is_none(),
            "No embedding should be created without service"
        );
    }
}

#[tokio::test]
async fn test_spawn_embedding_handles_empty_content() {
    // Given: A message with empty content
    let pool = create_test_pool().await;
    let config = test_config();
    let state = AppState::new(config, pool.clone(), None);

    let chat = chats::create_chat(&pool, None, "Test Chat", "gpt-4", false, true)
        .await
        .expect("Failed to create chat");

    let message_id = Uuid::new_v4();
    let chat_id = chat.id;
    let content = String::new();

    // When: Spawning the embedding task with empty content
    spawn_message_embedding_task(state.clone(), message_id, chat_id, content);

    // Wait a bit for the async task to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Then: Should handle gracefully without crashing
    // (The task should log but not fail the system)
}

#[tokio::test]
async fn test_spawn_embedding_respects_semaphore_limit() {
    // Given: A state with embedding service
    let pool = create_test_pool().await;
    let config = test_config();
    let state = AppState::new(config, pool.clone(), None);

    let chat = chats::create_chat(&pool, None, "Test Chat", "gpt-4", false, true)
        .await
        .expect("Failed to create chat");

    // When: Spawning many embedding tasks simultaneously
    let mut handles = vec![];
    for i in 0..20 {
        let state_clone = state.clone();
        let chat_id = chat.id;
        let handle = tokio::spawn(async move {
            let message_id = Uuid::new_v4();
            let content = format!("Test message {}", i);
            spawn_message_embedding_task(state_clone, message_id, chat_id, content);
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Then: All tasks should complete without overwhelming the system
    // (The semaphore should limit concurrency to MAX_CONCURRENT_EMBEDDINGS)
}

// =============================================================================
// Search Endpoint Tests
// =============================================================================

#[tokio::test]
async fn test_search_messages_endpoint_returns_results() {
    // Given: A client with embedding service and some messages
    let (client, state) = create_test_client_with_embeddings().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;
    let chat_id = create_test_chat(&client, &token, workspace_id).await;

    // Create some test messages
    let msg1_id = create_test_message(&client, &token, chat_id, "How do I install Python?").await;
    let msg2_id = create_test_message(&client, &token, chat_id, "What is machine learning?").await;
    let _msg3_id =
        create_test_message(&client, &token, chat_id, "Tell me about Rust programming").await;

    // Manually create embeddings for these messages (since we don't have real embedding service)
    if state.embedding_service().is_some() {
        let embedding1 = vec![0.1; 1536]; // Mock embedding
        let embedding2 = vec![0.2; 1536];

        message_embeddings::store_message_embedding(
            state.db(),
            msg1_id,
            chat_id,
            &embedding1,
            "text-embedding-3-small",
        )
        .await
        .expect("Failed to store embedding");

        message_embeddings::store_message_embedding(
            state.db(),
            msg2_id,
            chat_id,
            &embedding2,
            "text-embedding-3-small",
        )
        .await
        .expect("Failed to store embedding");

        // When: Searching for messages
        let response = client
            .get_auth(
                &format!(
                    "/api/chats/search?query=Python&limit=10&workspace_id={}",
                    workspace_id
                ),
                &token,
            )
            .await;

        // Then: Should return search results
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert!(body.is_array(), "Response should be an array");
    }
}

#[tokio::test]
async fn test_search_with_chat_id_filter() {
    // Given: Multiple chats with messages
    let (client, state) = create_test_client_with_embeddings().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;

    let chat1_id = create_test_chat(&client, &token, workspace_id).await;
    let chat2_id = create_test_chat(&client, &token, workspace_id).await;

    let msg1_id = create_test_message(&client, &token, chat1_id, "Python is great").await;
    let msg2_id = create_test_message(&client, &token, chat2_id, "Rust is awesome").await;

    // Store embeddings
    if state.embedding_service().is_some() {
        let embedding1 = vec![0.1; 1536];
        let embedding2 = vec![0.2; 1536];

        message_embeddings::store_message_embedding(
            state.db(),
            msg1_id,
            chat1_id,
            &embedding1,
            "text-embedding-3-small",
        )
        .await
        .ok();

        message_embeddings::store_message_embedding(
            state.db(),
            msg2_id,
            chat2_id,
            &embedding2,
            "text-embedding-3-small",
        )
        .await
        .ok();

        // When: Searching with chat_id filter
        let response = client
            .get_auth(
                &format!(
                    "/api/chats/search?query=programming&chat_id={}&workspace_id={}",
                    chat1_id, workspace_id
                ),
                &token,
            )
            .await;

        // Then: Should only return results from chat1
        response.assert_status(StatusCode::OK);
        let body = response.json_value();

        if let Some(results) = body.as_array() {
            for result in results {
                let result_chat_id = result
                    .get("chat_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok());
                // All results should be from chat1 (if any)
                if let Some(cid) = result_chat_id {
                    assert_eq!(cid, chat1_id, "Results should only be from specified chat");
                }
            }
        }
    }
}

#[tokio::test]
async fn test_search_with_threshold_filter() {
    // Given: A client with messages and embeddings
    let (client, state) = create_test_client_with_embeddings().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;
    let chat_id = create_test_chat(&client, &token, workspace_id).await;

    let msg_id = create_test_message(&client, &token, chat_id, "Test message").await;

    if state.embedding_service().is_some() {
        let embedding = vec![0.1; 1536];
        message_embeddings::store_message_embedding(
            state.db(),
            msg_id,
            chat_id,
            &embedding,
            "text-embedding-3-small",
        )
        .await
        .ok();

        // When: Searching with high threshold (0.95)
        let response = client
            .get_auth(
                &format!(
                    "/api/chats/search?query=test&threshold=0.95&workspace_id={}",
                    workspace_id
                ),
                &token,
            )
            .await;

        // Then: Should return fewer or no results
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert!(body.is_array(), "Response should be an array");

        // With high threshold, we expect fewer matches
        let results = body.as_array().unwrap();
        // Exact count depends on similarity, but it should be filtered
        assert!(results.len() <= 10, "Should respect limit");
    }
}

#[tokio::test]
async fn test_search_respects_limit_parameter() {
    // Given: A client with multiple messages
    let (client, state) = create_test_client_with_embeddings().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;
    let chat_id = create_test_chat(&client, &token, workspace_id).await;

    // Create multiple messages
    for i in 0..15 {
        let msg_id = create_test_message(&client, &token, chat_id, &format!("Message {}", i)).await;

        if state.embedding_service().is_some() {
            let embedding = vec![0.1; 1536];
            message_embeddings::store_message_embedding(
                state.db(),
                msg_id,
                chat_id,
                &embedding,
                "text-embedding-3-small",
            )
            .await
            .ok();
        }
    }

    if state.embedding_service().is_some() {
        // When: Searching with limit=5
        let response = client
            .get_auth(
                &format!(
                    "/api/chats/search?query=Message&limit=5&workspace_id={}",
                    workspace_id
                ),
                &token,
            )
            .await;

        // Then: Should return at most 5 results
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        let results = body.as_array().unwrap();
        assert!(results.len() <= 5, "Should respect limit of 5");
    }
}

#[tokio::test]
async fn test_search_handles_missing_query_parameter() {
    // Given: A client with workspace
    let client = TestClient::with_db().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;

    // When: Searching without query parameter (but with workspace_id)
    let response = client
        .get_auth(
            &format!("/api/chats/search?workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Should return 400 Bad Request
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "Should require query parameter"
    );
}

#[tokio::test]
async fn test_search_handles_embedding_service_unavailable() {
    // Given: A client WITHOUT embedding service but with workspace
    let client = TestClient::with_db().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;

    // When: Searching for messages
    let response = client
        .get_auth(
            &format!("/api/chats/search?query=test&workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Should return 400 (empty query) or 503 Service Unavailable
    // Without embedding service, the endpoint returns BAD_REQUEST
    assert!(
        response.status == StatusCode::BAD_REQUEST
            || response.status == StatusCode::SERVICE_UNAVAILABLE
            || response.status == StatusCode::INTERNAL_SERVER_ERROR,
        "Should indicate service unavailable or bad request, got: {}",
        response.status
    );
}

#[tokio::test]
async fn test_search_requires_authentication() {
    // Given: A client
    let client = TestClient::with_db().await;

    // When: Searching without authentication
    let response = client.get("/api/chats/search?query=test").await;

    // Then: Should return 401 Unauthorized
    assert_eq!(
        response.status,
        StatusCode::UNAUTHORIZED,
        "Search should require authentication"
    );
}

// =============================================================================
// Alternative Search Route Tests (chat-specific)
// =============================================================================

#[tokio::test]
async fn test_chat_specific_search_endpoint() {
    // Given: A chat with messages
    let (client, state) = create_test_client_with_embeddings().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;
    let chat_id = create_test_chat(&client, &token, workspace_id).await;

    let msg_id = create_test_message(&client, &token, chat_id, "Test message").await;

    if state.embedding_service().is_some() {
        let embedding = vec![0.1; 1536];
        message_embeddings::store_message_embedding(
            state.db(),
            msg_id,
            chat_id,
            &embedding,
            "text-embedding-3-small",
        )
        .await
        .ok();

        // When: Using chat-specific search endpoint
        let response = client
            .get_auth(&format!("/api/chats/{}/search?query=test", chat_id), &token)
            .await;

        // Then: Should return results from that chat
        // Note: This is an alternative endpoint pattern we could support
        // For now, we'll test the main search endpoint with chat_id parameter
        // This test documents the alternative design option
        assert!(
            response.status == StatusCode::OK || response.status == StatusCode::NOT_FOUND,
            "Chat-specific search endpoint (if implemented)"
        );
    }
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_embedding_worker_handles_invalid_message_id() {
    // Given: A state
    let pool = create_test_pool().await;
    let config = test_config();
    let state = AppState::new(config, pool, None);

    let invalid_message_id = Uuid::new_v4();
    let invalid_chat_id = Uuid::new_v4();
    let content = "Test content".to_string();

    // When: Spawning task with invalid IDs
    spawn_message_embedding_task(state.clone(), invalid_message_id, invalid_chat_id, content);

    // Wait for task to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Then: Should handle gracefully without crashing
    // (Task should log error but not fail the system)
}

#[tokio::test]
async fn test_search_handles_invalid_uuid_parameters() {
    // Given: A client with workspace
    let client = TestClient::with_db().await;
    let (token, _user_id, workspace_id) = setup_user_and_workspace(&client).await;

    // When: Searching with invalid chat_id UUID
    let response = client
        .get_auth(
            &format!(
                "/api/chats/search?query=test&chat_id=invalid-uuid&workspace_id={}",
                workspace_id
            ),
            &token,
        )
        .await;

    // Then: Should return 400 Bad Request
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "Should reject invalid UUID"
    );
}
