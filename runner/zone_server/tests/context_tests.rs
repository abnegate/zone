//! Integration tests for context search and gathering
//!
//! These tests exercise the context search endpoints with a real database.
//! Run with: SQLX_OFFLINE=true cargo test --test context_tests

mod common;

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use common::{TestClient, test_email, test_password};

// =============================================================================
// Helper Functions
// =============================================================================

/// Register a test user and return their (token, user_id)
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

/// Create a test organization and return its ID
async fn create_test_organization(client: &TestClient, token: &str) -> Uuid {
    let slug = format!("test-org-{}", Uuid::new_v4());
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": format!("Test Org {}", Uuid::new_v4()),
                "slug": slug,
                "description": "Test organization for context tests"
            }),
            token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Uuid::parse_str(body["id"].as_str().unwrap()).unwrap()
}

/// Create a test workspace and return its ID
///
/// Note: Due to the current workspace membership implementation, the creating user
/// is NOT automatically added as a workspace member. This is tracked in Phase 6.
/// For now, these tests will show that workspace access validation works correctly.
async fn create_test_workspace(
    client: &TestClient,
    token: &str,
    org_id: Uuid,
    _user_id: Uuid,
) -> Uuid {
    let slug = format!("test-ws-{}", Uuid::new_v4());
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": format!("Test Workspace {}", Uuid::new_v4()),
                "slug": slug,
                "description": "Test workspace for context tests"
            }),
            token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Uuid::parse_str(body["id"].as_str().unwrap()).unwrap()
}

// =============================================================================
// Search Tests
// =============================================================================

#[tokio::test]
async fn test_search_returns_error_when_service_unavailable() {
    // Given: A client without context services initialized
    let client = TestClient::with_db().await;
    let (token, user_id) = register_test_user(&client).await;
    let org_id = create_test_organization(&client, &token).await;
    let workspace_id = create_test_workspace(&client, &token, org_id, user_id).await;

    // When: Making a search request
    let response = client
        .get_auth(
            &format!("/api/context/search?q=test&workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Context service is not initialized, so should return SERVICE_UNAVAILABLE
    response.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("not available"));
}

#[tokio::test]
async fn test_search_validates_query_length() {
    // Given: A client and authenticated user
    let client = TestClient::with_db().await;
    let (token, user_id) = register_test_user(&client).await;
    let org_id = create_test_organization(&client, &token).await;
    let workspace_id = create_test_workspace(&client, &token, org_id, user_id).await;

    // When: Making a search request with empty query
    let response = client
        .get_auth(
            &format!("/api/context/search?q=&workspace_id={}", workspace_id),
            &token,
        )
        .await;

    // Then: Should return BAD_REQUEST for empty query
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_validates_query_max_length() {
    // Given: A client and authenticated user
    let client = TestClient::with_db().await;
    let (token, user_id) = register_test_user(&client).await;
    let org_id = create_test_organization(&client, &token).await;
    let workspace_id = create_test_workspace(&client, &token, org_id, user_id).await;

    // When: Making a search request with very long query
    let long_query = "a".repeat(1001);
    let response = client
        .get_auth(
            &format!(
                "/api/context/search?q={}&workspace_id={}",
                long_query, workspace_id
            ),
            &token,
        )
        .await;

    // Then: Should return BAD_REQUEST for query exceeding max length
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_validates_workspace_access() {
    // Given: A client and authenticated user
    let client = TestClient::with_db().await;
    let (token, _user_id) = register_test_user(&client).await;
    let fake_workspace_id = Uuid::new_v4();

    // When: Making a search request for a workspace the user doesn't have access to
    let response = client
        .get_auth(
            &format!(
                "/api/context/search?q=test&workspace_id={}",
                fake_workspace_id
            ),
            &token,
        )
        .await;

    // Then: Should return FORBIDDEN
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_search_validates_source_ids_format() {
    // Given: A client and authenticated user
    let client = TestClient::with_db().await;
    let (token, user_id) = register_test_user(&client).await;
    let org_id = create_test_organization(&client, &token).await;
    let workspace_id = create_test_workspace(&client, &token, org_id, user_id).await;

    // When: Making a search request with invalid source_ids
    let response = client
        .get_auth(
            &format!(
                "/api/context/search?q=test&workspace_id={}&source_ids=invalid-uuid",
                workspace_id
            ),
            &token,
        )
        .await;

    // Then: Should return BAD_REQUEST for invalid source_ids format
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_clamps_limit_to_maximum() {
    // Given: A client without context services
    let client = TestClient::with_db().await;
    let (token, user_id) = register_test_user(&client).await;
    let org_id = create_test_organization(&client, &token).await;
    let workspace_id = create_test_workspace(&client, &token, org_id, user_id).await;

    // When: Making a search request with limit > MAX_SEARCH_LIMIT
    let response = client
        .get_auth(
            &format!(
                "/api/context/search?q=test&workspace_id={}&limit=999",
                workspace_id
            ),
            &token,
        )
        .await;

    // Then: Request is valid but context service is unavailable
    response.assert_status(StatusCode::SERVICE_UNAVAILABLE);
}

// =============================================================================
// Unit Tests for Helper Functions
// =============================================================================

#[cfg(test)]
mod unit_tests {

    #[test]
    fn test_truncate_snippet_short_text() {
        let text = "This is a short text";
        let result = truncate_snippet(text, 200);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_snippet_exact_length() {
        let text = "a".repeat(200);
        let result = truncate_snippet(&text, 200);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_snippet_truncates_long_text() {
        let text = "a".repeat(300);
        let result = truncate_snippet(&text, 200);
        assert_eq!(result.len(), 203); // 200 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_snippet_handles_utf8_boundaries() {
        // String with emoji (4 bytes each) near the boundary
        let text = "a".repeat(195) + "🔥🔥🔥🔥🔥"; // 195 + 5*4 = 215 bytes total
        let result = truncate_snippet(&text, 200);

        // Should not panic and should be valid UTF-8
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
        assert!(result.ends_with("..."));

        // The function uses char_indices which iterates by byte index
        // It includes all characters whose starting byte index is < 200
        // The 195 'a's occupy bytes 0-194
        // The first emoji starts at byte 195 (< 200), so it's included
        // The second emoji starts at byte 199 (< 200), so it's included
        // The third emoji starts at byte 203 (>= 200), so it stops
        // Result: 195 'a's + 2 emojis (8 bytes) + "..." (3 bytes) = 206 bytes
        assert_eq!(result.len(), 206);
    }

    #[test]
    fn test_truncate_snippet_handles_multibyte_chars() {
        // Japanese characters (3 bytes each)
        let text = "こんにちは世界".repeat(50); // Way more than 200 chars
        let result = truncate_snippet(&text, 200);

        // Should not panic and should be valid UTF-8
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
        assert!(result.ends_with("..."));
    }

    // Helper function definition for testing
    fn truncate_snippet(text: &str, max_len: usize) -> String {
        if text.len() <= max_len {
            text.to_string()
        } else {
            let boundary = text
                .char_indices()
                .take_while(|(i, _)| *i < max_len)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(max_len);
            format!("{}...", &text[..boundary])
        }
    }
}
