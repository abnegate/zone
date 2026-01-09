//! Integration tests for Knowledge CRUD endpoints
//!
//! Tests the /api/knowledge endpoints for creating, listing, and deleting
//! user-curated knowledge entries.

mod common;

use axum::http::StatusCode;
use common::{TestClient, test_email, test_password};
use serde_json::json;
use uuid::Uuid;

// ============================================================================
// Test Setup Helpers
// ============================================================================

/// Create a test user and get their access token
async fn create_test_user(client: &TestClient) -> (String, Uuid) {
    let email = test_email();
    let password = test_password();

    // Register
    let register_body = json!({
        "email": email,
        "password": password,
        "display_name": "Test User"
    });
    let response = client.post_json("/api/auth/register", &register_body).await;
    response.assert_status(StatusCode::CREATED);

    // Login
    let login_body = json!({
        "email": email,
        "password": password
    });
    let response = client.post_json("/api/auth/login", &login_body).await;
    response.assert_status(StatusCode::OK);

    let json = response.json_value();
    let token = json["access_token"].as_str().unwrap().to_string();
    let user_id = Uuid::parse_str(json["user"]["id"].as_str().unwrap()).unwrap();

    (token, user_id)
}

/// Create a test organization and workspace
async fn create_test_workspace(client: &TestClient, token: &str, _user_id: Uuid) -> (Uuid, Uuid) {
    // Create organization
    let org_body = json!({
        "name": format!("Test Org {}", Uuid::new_v4()),
        "slug": format!("test-org-{}", Uuid::new_v4())
    });
    let response = client
        .post_json_auth("/api/organizations", &org_body, token)
        .await;
    response.assert_status(StatusCode::CREATED);
    let org_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // Create workspace (note: endpoint is under organizations)
    let ws_body = json!({
        "name": format!("Test Workspace {}", Uuid::new_v4()),
        "slug": format!("test-ws-{}", Uuid::new_v4())
    });
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &ws_body,
            token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let workspace_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // Note: Workspace creation now auto-adds the creator as a workspace admin

    (org_id, workspace_id)
}

// ============================================================================
// Create Knowledge Tests
// ============================================================================

#[tokio::test]
async fn test_create_knowledge_success() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge entry with valid data
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Test Knowledge Entry",
        "content": "This is test content for the knowledge base.",
        "category": "documentation",
        "tags": ["test", "example"]
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should succeed with CREATED status
    response.assert_status(StatusCode::CREATED);
    let json = response.json_value();

    assert!(json["id"].is_string());
    assert_eq!(json["workspace_id"], workspace_id.to_string());
    assert_eq!(json["title"], "Test Knowledge Entry");
    assert_eq!(
        json["content"],
        "This is test content for the knowledge base."
    );
    assert_eq!(json["category"], "documentation");
    assert_eq!(json["tags"][0], "test");
    assert_eq!(json["tags"][1], "example");
    assert!(json["token_count"].as_u64().unwrap() > 0);
    assert_eq!(json["is_active"], true);
}

#[tokio::test]
async fn test_create_knowledge_calculates_token_count() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge with known content
    let content = "The quick brown fox jumps over the lazy dog.";
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Token Test",
        "content": content,
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should calculate token count correctly
    response.assert_status(StatusCode::CREATED);
    let json = response.json_value();

    let token_count = json["token_count"].as_u64().unwrap();
    assert!(
        (9..=15).contains(&token_count),
        "Token count should be reasonable for the content"
    );
}

#[tokio::test]
async fn test_create_knowledge_without_optional_fields() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge without category and tags
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Minimal Entry",
        "content": "Minimal content",
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should succeed
    response.assert_status(StatusCode::CREATED);
    let json = response.json_value();

    assert_eq!(json["category"], serde_json::Value::Null);
    assert_eq!(json["tags"], json!([]));
}

#[tokio::test]
async fn test_create_knowledge_empty_title() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge with empty title
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "",
        "content": "Some content",
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should fail with BAD_REQUEST
    response.assert_status(StatusCode::BAD_REQUEST);
    let json = response.json_value();
    assert!(json["error"].as_str().unwrap().contains("Title"));
}

#[tokio::test]
async fn test_create_knowledge_empty_content() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge with empty content
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Empty Content",
        "content": "",
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should fail with BAD_REQUEST
    response.assert_status(StatusCode::BAD_REQUEST);
    let json = response.json_value();
    assert!(
        json["error"].as_str().unwrap().contains("content"),
        "Error message should mention content: {}",
        json["error"]
    );
}

#[tokio::test]
async fn test_create_knowledge_title_too_long() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge with title exceeding 256 characters
    let long_title = "a".repeat(257);
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": long_title,
        "content": "Some content",
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should fail with BAD_REQUEST
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_knowledge_too_many_tags() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge with more than 20 tags
    let many_tags: Vec<String> = (0..21).map(|i| format!("tag{}", i)).collect();
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Too Many Tags",
        "content": "Some content",
        "tags": many_tags,
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should fail with BAD_REQUEST
    response.assert_status(StatusCode::BAD_REQUEST);
    let json = response.json_value();
    assert!(json["error"].as_str().unwrap().contains("tags"));
}

#[tokio::test]
async fn test_create_knowledge_tag_too_long() {
    // Given: Authenticated user with workspace
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Creating knowledge with tag exceeding 64 characters
    let long_tag = "a".repeat(65);
    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Long Tag",
        "content": "Some content",
        "tags": [long_tag],
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;

    // Then: Should fail with BAD_REQUEST
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_knowledge_without_workspace_access() {
    // Given: Two users with different workspaces
    let client = TestClient::with_db().await;
    let (token1, user_id1) = create_test_user(&client).await;
    let (_org_id1, workspace_id1) = create_test_workspace(&client, &token1, user_id1).await;

    let (token2, _user_id2) = create_test_user(&client).await;

    // When: User 2 tries to create knowledge in User 1's workspace
    let knowledge_body = json!({
        "workspace_id": workspace_id1.to_string(),
        "title": "Unauthorized",
        "content": "Should not work",
    });

    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token2)
        .await;

    // Then: Should fail with FORBIDDEN
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_create_knowledge_without_auth() {
    // Given: No authentication
    let client = TestClient::with_db().await;

    // When: Creating knowledge without token
    let knowledge_body = json!({
        "workspace_id": Uuid::new_v4().to_string(),
        "title": "No Auth",
        "content": "Should not work",
    });

    let response = client.post_json("/api/knowledge", &knowledge_body).await;

    // Then: Should fail with UNAUTHORIZED
    response.assert_status(StatusCode::UNAUTHORIZED);
}

// ============================================================================
// List Knowledge Tests
// ============================================================================

#[tokio::test]
async fn test_list_knowledge_empty() {
    // Given: Authenticated user with workspace but no knowledge entries
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // When: Listing knowledge
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Should return empty array
    response.assert_status(StatusCode::OK);
    let json = response.json_value();
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_list_knowledge_multiple_entries() {
    // Given: Authenticated user with multiple knowledge entries
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // Create three knowledge entries
    for i in 1..=3 {
        let knowledge_body = json!({
            "workspace_id": workspace_id.to_string(),
            "title": format!("Entry {}", i),
            "content": format!("Content {}", i),
        });
        client
            .post_json_auth("/api/knowledge", &knowledge_body, &token)
            .await
            .assert_status(StatusCode::CREATED);
    }

    // When: Listing knowledge
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Should return all three entries
    response.assert_status(StatusCode::OK);
    let json = response.json_value();
    assert_eq!(json.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn test_list_knowledge_filter_by_category() {
    // Given: Knowledge entries with different categories
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // Create entries with different categories
    let categories = ["docs", "code", "docs"];
    for (i, category) in categories.iter().enumerate() {
        let knowledge_body = json!({
            "workspace_id": workspace_id.to_string(),
            "title": format!("Entry {}", i + 1),
            "content": format!("Content {}", i + 1),
            "category": category,
        });
        client
            .post_json_auth("/api/knowledge", &knowledge_body, &token)
            .await
            .assert_status(StatusCode::CREATED);
    }

    // When: Filtering by "docs" category
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}&category=docs", workspace_id),
            &token,
        )
        .await;

    // Then: Should return only "docs" entries
    response.assert_status(StatusCode::OK);
    let json = response.json_value();
    let entries = json.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        assert_eq!(entry["category"], "docs");
    }
}

#[tokio::test]
async fn test_list_knowledge_excludes_inactive() {
    // Given: Knowledge entries where one is deleted
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    // Create two entries
    let knowledge_body1 = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "Active Entry",
        "content": "Content 1",
    });
    client
        .post_json_auth("/api/knowledge", &knowledge_body1, &token)
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id.to_string(),
                "title": "To Delete",
                "content": "Content 2",
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let entry_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // Delete the second entry
    client
        .delete_auth(&format!("/api/knowledge/{}", entry_id), &token)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // When: Listing knowledge
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Should return only active entry
    response.assert_status(StatusCode::OK);
    let json = response.json_value();
    let entries = json.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["title"], "Active Entry");
}

#[tokio::test]
async fn test_list_knowledge_without_workspace_access() {
    // Given: Two users with different workspaces
    let client = TestClient::with_db().await;
    let (token1, user_id1) = create_test_user(&client).await;
    let (_org_id1, workspace_id1) = create_test_workspace(&client, &token1, user_id1).await;

    let (token2, _user_id2) = create_test_user(&client).await;

    // When: User 2 tries to list knowledge in User 1's workspace
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", workspace_id1),
            &token2,
        )
        .await;

    // Then: Should fail with FORBIDDEN
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_list_knowledge_without_auth() {
    // Given: No authentication
    let client = TestClient::with_db().await;

    // When: Listing knowledge without token
    let response = client
        .get(&format!("/api/knowledge?workspace_id={}", Uuid::new_v4()))
        .await;

    // Then: Should fail with UNAUTHORIZED
    response.assert_status(StatusCode::UNAUTHORIZED);
}

// ============================================================================
// Delete Knowledge Tests
// ============================================================================

#[tokio::test]
async fn test_delete_knowledge_success() {
    // Given: Knowledge entry exists
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "To Delete",
        "content": "Will be deleted",
    });
    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;
    response.assert_status(StatusCode::CREATED);
    let entry_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // When: Deleting the entry
    let response = client
        .delete_auth(&format!("/api/knowledge/{}", entry_id), &token)
        .await;

    // Then: Should succeed with NO_CONTENT
    response.assert_status(StatusCode::NO_CONTENT);

    // And: Entry should not appear in list
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", workspace_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value().as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_delete_knowledge_nonexistent() {
    // Given: Authenticated user
    let client = TestClient::with_db().await;
    let (token, _user_id) = create_test_user(&client).await;

    // When: Deleting non-existent entry
    let fake_id = Uuid::new_v4();
    let response = client
        .delete_auth(&format!("/api/knowledge/{}", fake_id), &token)
        .await;

    // Then: Should return NOT_FOUND
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_knowledge_without_workspace_access() {
    // Given: Two users with different workspaces
    let client = TestClient::with_db().await;
    let (token1, user_id1) = create_test_user(&client).await;
    let (_org_id1, workspace_id1) = create_test_workspace(&client, &token1, user_id1).await;

    // Create knowledge entry with user 1
    let knowledge_body = json!({
        "workspace_id": workspace_id1.to_string(),
        "title": "User 1 Entry",
        "content": "User 1 content",
    });
    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token1)
        .await;
    response.assert_status(StatusCode::CREATED);
    let entry_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // Create user 2
    let (token2, _user_id2) = create_test_user(&client).await;

    // When: User 2 tries to delete User 1's knowledge entry
    let response = client
        .delete_auth(&format!("/api/knowledge/{}", entry_id), &token2)
        .await;

    // Then: Should fail with FORBIDDEN
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_delete_knowledge_without_auth() {
    // Given: No authentication
    let client = TestClient::with_db().await;

    // When: Deleting without token
    let fake_id = Uuid::new_v4();
    let response = client
        .delete_auth(&format!("/api/knowledge/{}", fake_id), "invalid-token")
        .await;

    // Then: Should fail with UNAUTHORIZED
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_delete_knowledge_idempotent() {
    // Given: Knowledge entry that has already been deleted
    let client = TestClient::with_db().await;
    let (token, user_id) = create_test_user(&client).await;
    let (_org_id, workspace_id) = create_test_workspace(&client, &token, user_id).await;

    let knowledge_body = json!({
        "workspace_id": workspace_id.to_string(),
        "title": "To Delete",
        "content": "Will be deleted",
    });
    let response = client
        .post_json_auth("/api/knowledge", &knowledge_body, &token)
        .await;
    response.assert_status(StatusCode::CREATED);
    let entry_id = Uuid::parse_str(response.json_value()["id"].as_str().unwrap()).unwrap();

    // Delete once
    client
        .delete_auth(&format!("/api/knowledge/{}", entry_id), &token)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // When: Deleting again
    let response = client
        .delete_auth(&format!("/api/knowledge/{}", entry_id), &token)
        .await;

    // Then: Should return NOT_FOUND (already deleted)
    response.assert_status(StatusCode::NOT_FOUND);
}
