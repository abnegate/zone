//! Tests for automatic source indexing functionality
//!
//! Run with: SQLX_OFFLINE=true cargo test --test indexing_tests

mod common;

use axum::http::StatusCode;
use common::{TestClient, create_test_pool, test_email, test_password};
use serde_json::json;
use uuid::Uuid;

/// Helper to create a test user with org and workspace
async fn setup_user_and_workspace(client: &TestClient) -> (String, String, String) {
    // Register user
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

    let body = response.json_value();
    let token = body["access_token"].as_str().unwrap().to_string();

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

    let org_id = org_response.json_value()["id"]
        .as_str()
        .unwrap()
        .to_string();

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

    let workspace_id = ws_response.json_value()["id"].as_str().unwrap().to_string();

    (token, org_id, workspace_id)
}

// =============================================================================
// Auto-indexing Tests
// =============================================================================

#[tokio::test]
async fn test_create_source_includes_index_status() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;
    let source_name = format!("Test Source {}", Uuid::new_v4());

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": source_name,
                "source_type": "text",
                "config": {
                    "content": "Test content"
                }
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);

    let source = response.json_value();

    // Verify index status fields are present
    assert!(
        source["index_status"].is_string(),
        "index_status should be present"
    );
    assert!(
        source.get("last_indexed_at").is_some(),
        "last_indexed_at should be present"
    );
    assert!(
        source.get("indexed_items_count").is_some(),
        "indexed_items_count should be present"
    );
}

#[tokio::test]
async fn test_create_source_triggers_auto_index() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;
    let pool = create_test_pool().await;

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": format!("Auto-Index Test {}", Uuid::new_v4()),
                "source_type": "text",
                "config": {
                    "content": "Test document"
                }
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);

    let source = response.json_value();
    let source_id = Uuid::parse_str(source["id"].as_str().unwrap()).unwrap();

    // Wait for background indexing to be queued
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Check that a gathering was created
    let gathering_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM context_gatherings WHERE $1 = ANY(source_ids))",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        gathering_exists,
        "Gathering should be created automatically"
    );
}

#[tokio::test]
async fn test_update_config_triggers_reindex() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;
    let pool = create_test_pool().await;

    // Create source
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": format!("Reindex Test {}", Uuid::new_v4()),
                "source_type": "text",
                "config": {
                    "content": "Original content"
                }
            }),
            &token,
        )
        .await;

    let source = response.json_value();
    let source_id = Uuid::parse_str(source["id"].as_str().unwrap()).unwrap();

    // Wait for initial index
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let initial_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(source_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Update config
    client
        .put_json_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &json!({
                "config": {
                    "content": "Updated content"
                }
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    // Wait for re-index
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let final_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(source_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(final_count > initial_count, "Re-index should be triggered");
}

#[tokio::test]
async fn test_update_name_no_reindex() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;
    let pool = create_test_pool().await;

    // Create source
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": format!("Original Name {}", Uuid::new_v4()),
                "source_type": "text",
                "config": {
                    "content": "Content"
                }
            }),
            &token,
        )
        .await;

    let source_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // Wait for initial index
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let initial_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(source_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Update only name
    let new_name = format!("Updated Name {}", Uuid::new_v4());
    client
        .put_json_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &json!({
                "name": new_name
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let final_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(source_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        initial_count, final_count,
        "Name change should not trigger re-index"
    );
}

#[tokio::test]
async fn test_manual_reindex_endpoint() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;
    let pool = create_test_pool().await;

    // Create source
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": format!("Manual Reindex {}", Uuid::new_v4()),
                "source_type": "text",
                "config": {
                    "content": "Content"
                }
            }),
            &token,
        )
        .await;

    let source_value = response.json_value();
    let source_id = source_value["id"].as_str().unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let initial_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(Uuid::parse_str(source_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();

    // Trigger manual reindex
    let reindex_response = client
        .post_json_auth(
            &format!(
                "/api/workspaces/{}/sources/{}/reindex",
                workspace_id, source_id
            ),
            &json!({}),
            &token,
        )
        .await;

    reindex_response.assert_status(StatusCode::ACCEPTED);

    let body = reindex_response.json_value();
    assert_eq!(body["message"], "Re-indexing started");

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let final_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(Uuid::parse_str(source_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(
        final_count > initial_count,
        "Manual reindex should create new gathering"
    );
}

#[tokio::test]
async fn test_manual_reindex_not_found() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;

    let response = client
        .post_json_auth(
            &format!(
                "/api/workspaces/{}/sources/{}/reindex",
                workspace_id,
                Uuid::new_v4()
            ),
            &json!({}),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_source_includes_index_status() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;

    // Create source
    let create_response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": format!("Get Test {}", Uuid::new_v4()),
                "source_type": "text",
                "config": {
                    "content": "Content"
                }
            }),
            &token,
        )
        .await;

    let create_value = create_response.json_value();
    let source_id = create_value["id"].as_str().unwrap();

    // Get source
    let get_response = client
        .get_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &token,
        )
        .await;

    get_response.assert_status(StatusCode::OK);

    let source = get_response.json_value();
    assert!(
        source["index_status"].is_string(),
        "index_status should be present"
    );

    let status = source["index_status"].as_str().unwrap();
    assert!(
        ["pending", "indexing", "indexed", "failed"].contains(&status),
        "Status should be valid: {}",
        status
    );
}

#[tokio::test]
async fn test_list_sources_includes_index_status() {
    let client = TestClient::with_db().await;
    let (token, _org_id, workspace_id) = setup_user_and_workspace(&client).await;

    // Create sources with unique names
    let base_id = Uuid::new_v4();
    for i in 0..3 {
        let response = client
            .post_json_auth(
                &format!("/api/workspaces/{}/sources", workspace_id),
                &json!({
                    "name": format!("Source {} {}", i, base_id),
                    "source_type": "text",
                    "config": {
                        "content": format!("Content {}", i)
                    }
                }),
                &token,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
    }

    // List sources
    let response = client
        .get_auth(&format!("/api/workspaces/{}/sources", workspace_id), &token)
        .await;

    response.assert_status(StatusCode::OK);

    let sources: Vec<serde_json::Value> = response.json();
    assert_eq!(sources.len(), 3);

    for source in sources {
        assert!(
            source["index_status"].is_string(),
            "All sources should have index_status"
        );
        assert!(
            source.get("last_indexed_at").is_some(),
            "All sources should have last_indexed_at"
        );
        assert!(
            source.get("indexed_items_count").is_some(),
            "All sources should have indexed_items_count"
        );
    }
}

// =============================================================================
// Unit Tests for Helper Functions
// =============================================================================

#[test]
fn test_config_changed_helper() {
    use zone_server::workers::indexing;

    let config1 = json!({"key": "value1"});
    let config2 = json!({"key": "value2"});
    let config3 = json!({"key": "value1"});

    assert!(
        indexing::config_changed(&config1, &config2),
        "Different configs should be detected"
    );
    assert!(
        !indexing::config_changed(&config1, &config3),
        "Same configs should not be detected as changed"
    );
}

#[test]
fn test_credentials_changed_helper() {
    use zone_server::workers::indexing;

    assert!(indexing::credentials_changed(Some("old"), Some("new")));
    assert!(!indexing::credentials_changed(Some("same"), Some("same")));
    assert!(indexing::credentials_changed(None, Some("new")));
    assert!(indexing::credentials_changed(Some("old"), None));
    assert!(!indexing::credentials_changed(None, None));
}
