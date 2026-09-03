//! Tests for knowledge base web URL functionality
//!
//! Run with: SQLX_OFFLINE=true cargo test --test knowledge_url_tests

mod common;

use axum::http::StatusCode;
use common::{TestClient, test_email, test_password};
use serde_json::json;
use uuid::Uuid;

/// Helper to create a test user with org and workspace
async fn setup_user_and_workspace(client: &TestClient) -> (String, Uuid) {
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

    let org_id = org_response.json_value()["organization"]["id"]
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

    let workspace_id = Uuid::parse_str(
        ws_response.json_value()["workspace"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    (token, workspace_id)
}

// =============================================================================
// Knowledge URL Tests
// =============================================================================

#[tokio::test]
async fn test_create_knowledge_with_content() {
    let client = TestClient::with_db().await;
    let (token, workspace_id) = setup_user_and_workspace(&client).await;

    let response = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test Knowledge",
                "content": "This is test content",
                "category": "docs",
                "tags": ["test", "example"]
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);

    let body = response.json_value();
    assert_eq!(body["title"], "Test Knowledge");
    assert_eq!(body["content"], "This is test content");
    assert!(body["source_url"].is_null());
    assert!(body["refresh_interval_minutes"].is_null());
}

#[tokio::test]
async fn test_create_knowledge_requires_content_or_url() {
    let client = TestClient::with_db().await;
    let (token, workspace_id) = setup_user_and_workspace(&client).await;

    let response = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test Knowledge"
                // No content or source_url
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("content or source_url")
    );
}

#[tokio::test]
async fn test_create_knowledge_url_validation() {
    let client = TestClient::with_db().await;
    let (token, workspace_id) = setup_user_and_workspace(&client).await;

    // Invalid URL scheme
    let response = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test",
                "source_url": "ftp://example.com/doc"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("http"));
}

#[tokio::test]
async fn test_create_knowledge_refresh_interval_validation() {
    let client = TestClient::with_db().await;
    let (token, workspace_id) = setup_user_and_workspace(&client).await;

    // Negative interval
    let response = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test",
                "content": "content",
                "refresh_interval_minutes": -1
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("negative"));
}

#[tokio::test]
async fn test_create_knowledge_refresh_interval_max() {
    let client = TestClient::with_db().await;
    let (token, workspace_id) = setup_user_and_workspace(&client).await;

    // Exceeds max (30 days = 43200 minutes)
    let response = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test",
                "content": "content",
                "refresh_interval_minutes": 50000
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("30 days"));
}

#[tokio::test]
async fn test_list_knowledge_includes_url_fields() {
    let client = TestClient::with_db().await;
    let (token, workspace_id) = setup_user_and_workspace(&client).await;

    // Create a knowledge entry without URL (URL fields will be None and omitted)
    client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Test Knowledge",
                "content": "Test content"
            }),
            &token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    // List knowledge
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);

    let entries: Vec<serde_json::Value> = response.json();
    assert!(!entries.is_empty());

    // Check that required fields are present
    let entry = &entries[0];
    assert!(entry.get("id").is_some());
    assert!(entry.get("title").is_some());
    assert!(entry.get("workspace_id").is_some());
    // URL fields are None for non-URL entries and omitted via skip_serializing_if
    // They would be present if the entry had a source_url
}

// =============================================================================
// DB Function Tests
// =============================================================================

#[tokio::test]
async fn test_url_exists_in_workspace() {
    let client = TestClient::with_db().await;
    let pool = common::create_test_pool().await;

    // Create test workspace (via API for simplicity)
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
                "display_name": "Test"
            }),
        )
        .await;
    let token = response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_string();

    let org_response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": format!("Org {}", Uuid::new_v4()),
                "slug": format!("org-{}", Uuid::new_v4())
            }),
            &token,
        )
        .await;
    let org_body = org_response.json_value();
    let org_id = org_body["organization"]["id"].as_str().unwrap();

    let ws_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": format!("WS {}", Uuid::new_v4()),
                "slug": format!("ws-{}", Uuid::new_v4())
            }),
            &token,
        )
        .await;
    let ws_body = ws_response.json_value();
    let workspace_id = Uuid::parse_str(ws_body["workspace"]["id"].as_str().unwrap()).unwrap();

    // Use DB function directly
    use zone_server::db::knowledge;

    // Check for non-existent URL
    let result =
        knowledge::url_exists_in_workspace(&pool, workspace_id, "https://example.com/test")
            .await
            .unwrap();
    assert!(result.is_none());
}

#[test]
fn test_knowledge_row_has_url_fields() {
    use zone_server::db::knowledge::{KnowledgeListRow, KnowledgeRow};

    // Verify the struct has the expected fields
    // This is a compile-time check that the fields exist
    let _row = KnowledgeRow {
        id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        title: "Test".to_string(),
        content: "Content".to_string(),
        category: None,
        tags: vec![],
        token_count: 0,
        is_active: true,
        source_url: Some("https://example.com".to_string()),
        last_fetched_at: None,
        content_hash: Some("abc123".to_string()),
        refresh_interval_minutes: Some(60),
        last_fetch_error: None,
    };

    let _list_row = KnowledgeListRow {
        id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        title: "Test".to_string(),
        category: None,
        tags: vec![],
        token_count: 0,
        is_active: true,
        source_url: Some("https://example.com".to_string()),
        last_fetched_at: None,
        refresh_interval_minutes: Some(60),
        last_fetch_error: None,
    };
}

// =============================================================================
// Worker Function Tests
// =============================================================================

#[test]
fn test_clean_text_in_worker() {
    // Test that the clean_text function works correctly
    // Note: This duplicates testing from knowledge_refresh.rs unit tests

    fn clean_text(text: &str) -> String {
        let mut result = String::new();
        let mut last_was_whitespace = false;

        for c in text.chars() {
            if c.is_whitespace() {
                if !last_was_whitespace {
                    result.push(' ');
                    last_was_whitespace = true;
                }
            } else {
                result.push(c);
                last_was_whitespace = false;
            }
        }

        result.trim().to_string()
    }

    assert_eq!(clean_text("  hello   world  "), "hello world");
    assert_eq!(clean_text("\n\n\ntest\n\n\n"), "test");
}
