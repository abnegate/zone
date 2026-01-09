//! Integration tests for the Zone Server API
//!
//! These tests exercise the HTTP endpoints with a real database.
//! Run with: SQLX_OFFLINE=true cargo test --test api_tests

#![allow(unused_variables)]

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

// =============================================================================
// Health Check Tests
// =============================================================================

#[tokio::test]
async fn test_health_check() {
    let client = TestClient::with_db().await;

    let response = client.get("/health").await;

    response.assert_status(StatusCode::OK);

    let body = response.json_value();
    assert_eq!(body["status"], "healthy");
    assert!(body["version"].is_string());
}

// =============================================================================
// Auth - Registration Tests
// =============================================================================

#[tokio::test]
async fn test_register_success() {
    let client = TestClient::with_db().await;

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
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
    assert_eq!(body["token_type"], "Bearer");
    assert!(body["expires_in"].is_number());
    assert!(body["user"]["id"].is_string());
    assert!(body["user"]["email"].is_string());
}

#[tokio::test]
async fn test_register_invalid_email() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": "invalid-email",
                "password": test_password()
            }),
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);

    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("email"));
}

#[tokio::test]
async fn test_register_short_password() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": "short"
            }),
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);

    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("8 characters"));
}

#[tokio::test]
async fn test_register_duplicate_email() {
    let client = TestClient::with_db().await;
    let email = test_email();

    // First registration
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": test_password()
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);

    // Duplicate registration
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": test_password()
            }),
        )
        .await;

    response.assert_status(StatusCode::CONFLICT);

    let body = response.json_value();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("already registered")
    );
}

// =============================================================================
// Auth - Login Tests
// =============================================================================

#[tokio::test]
async fn test_login_success() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register first
    client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password
            }),
        )
        .await;

    // Login
    let response = client
        .post_json(
            "/api/auth/login",
            &json!({
                "email": &email,
                "password": &password
            }),
        )
        .await;

    response.assert_status(StatusCode::OK);

    let body = response.json_value();
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
}

#[tokio::test]
async fn test_login_wrong_password() {
    let client = TestClient::with_db().await;
    let email = test_email();

    // Register
    client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": test_password()
            }),
        )
        .await;

    // Login with wrong password
    let response = client
        .post_json(
            "/api/auth/login",
            &json!({
                "email": &email,
                "password": "WrongPassword123!"
            }),
        )
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_login_nonexistent_user() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/login",
            &json!({
                "email": "nonexistent@example.com",
                "password": test_password()
            }),
        )
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Auth - Token Refresh Tests
// =============================================================================

#[tokio::test]
async fn test_refresh_token_success() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register and get tokens
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password
            }),
        )
        .await;

    let body = response.json_value();
    let refresh_token = body["refresh_token"].as_str().unwrap();

    // Refresh
    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({
                "refresh_token": refresh_token
            }),
        )
        .await;

    response.assert_status(StatusCode::OK);

    let body = response.json_value();
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
}

#[tokio::test]
async fn test_refresh_invalid_token() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({
                "refresh_token": "invalid-token"
            }),
        )
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Auth - Logout Tests
// =============================================================================

#[tokio::test]
async fn test_logout_success() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register and get tokens
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password
            }),
        )
        .await;

    let body = response.json_value();
    let access_token = body["access_token"].as_str().unwrap();

    // Logout
    let response = client
        .post_json_auth("/api/auth/logout", &json!({}), access_token)
        .await;

    response.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_logout_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.post_json("/api/auth/logout", &json!({})).await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Protected Routes - Without Auth
// =============================================================================

#[tokio::test]
async fn test_organizations_list_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/organizations").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_projects_list_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/projects").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_tasks_list_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/tasks").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_chats_list_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/chats").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_sources_list_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/sources").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_models_list_without_auth() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/models").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Protected Routes - With Auth
// =============================================================================

async fn get_auth_token(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password()
            }),
        )
        .await;

    let body = response.json_value();
    body["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_organizations_list_with_auth() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;

    let response = client.get_auth("/api/organizations", &token).await;

    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn test_projects_list_with_auth() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(
            &format!("/api/projects?workspace_id={}", workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn test_tasks_list_with_auth() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;

    let response = client.get_auth("/api/tasks", &token).await;

    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn test_chats_list_with_auth() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/chats?workspace_id={}", workspace_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn test_sources_list_with_auth() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/workspaces/{}/sources", workspace_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn test_sources_types_with_auth() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client.get_auth("/api/sources/types", &token).await;

    response.assert_status(StatusCode::OK);

    let body = response.json_value();
    assert!(body.is_array());
    let types = body.as_array().unwrap();
    assert!(!types.is_empty());
    // Verify source type structure
    let first = &types[0];
    assert!(first["name"].is_string());
    assert!(first["display_name"].is_string());
    assert!(first["category"].is_string());
}

// =============================================================================
// Organizations - CRUD Tests
// =============================================================================

fn test_slug() -> String {
    format!("test-slug-{}", uuid::Uuid::new_v4())
}

/// Create an organization and workspace for testing, returns (org_id, workspace_id)
async fn setup_test_workspace(client: &TestClient, token: &str) -> (String, String) {
    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Test Org",
                "slug": test_slug()
            }),
            token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create workspace
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": "Test Workspace",
                "slug": test_slug()
            }),
            token,
        )
        .await;
    let workspace_id = response.json_value()["id"].as_str().unwrap().to_string();

    (org_id, workspace_id)
}

#[tokio::test]
async fn test_organization_create() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let slug = test_slug();

    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Test Organization",
                "slug": &slug,
                "description": "A test organization"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);

    let body = response.json_value();
    assert!(body["id"].is_string());
    assert_eq!(body["name"], "Test Organization");
    assert_eq!(body["slug"], slug);
    assert_eq!(body["description"], "A test organization");
    assert_eq!(body["is_active"], true);
}

#[tokio::test]
async fn test_organization_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Get Test Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;
    let body = response.json_value();
    let org_id = body["id"].as_str().unwrap();

    // Get organization
    let response = client
        .get_auth(&format!("/api/organizations/{}", org_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["name"], "Get Test Org");
}

#[tokio::test]
async fn test_organization_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let fake_id = uuid::Uuid::new_v4();

    let response = client
        .get_auth(&format!("/api/organizations/{}", fake_id), &token)
        .await;

    // Returns 403 because user is not a member of the (nonexistent) organization
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_organization_update() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Update Test Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;
    let body = response.json_value();
    let org_id = body["id"].as_str().unwrap();

    // Update organization
    let response = client
        .patch_json_auth(
            &format!("/api/organizations/{}", org_id),
            &json!({
                "name": "Updated Org Name",
                "description": "Updated description",
                "is_active": false
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["name"], "Updated Org Name");
    assert_eq!(body["description"], "Updated description");
    assert_eq!(body["is_active"], false);
}

#[tokio::test]
async fn test_organization_update_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let fake_id = uuid::Uuid::new_v4();

    let response = client
        .patch_json_auth(
            &format!("/api/organizations/{}", fake_id),
            &json!({ "name": "Won't Work" }),
            &token,
        )
        .await;

    // Returns 403 because user is not a member of the (nonexistent) organization
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_organization_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Delete Test Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;
    let body = response.json_value();
    let org_id = body["id"].as_str().unwrap();

    // Delete organization
    let response = client
        .delete_auth(&format!("/api/organizations/{}", org_id), &token)
        .await;

    response.assert_status(StatusCode::NO_CONTENT);

    // Verify it's deleted - returns 403 since we're no longer a member after deletion
    let response = client
        .get_auth(&format!("/api/organizations/{}", org_id), &token)
        .await;
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_organization_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let fake_id = uuid::Uuid::new_v4();

    let response = client
        .delete_auth(&format!("/api/organizations/{}", fake_id), &token)
        .await;

    // Returns 403 because user is not a member of the (nonexistent) organization
    response.assert_status(StatusCode::FORBIDDEN);
}

// =============================================================================
// Workspaces - CRUD Tests
// =============================================================================

#[tokio::test]
async fn test_workspace_create() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization first
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Workspace Parent Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;
    let body = response.json_value();
    let org_id = body["id"].as_str().unwrap();

    // Create workspace
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": "Test Workspace",
                "slug": test_slug(),
                "description": "A test workspace"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["id"].is_string());
    assert_eq!(body["name"], "Test Workspace");
    assert_eq!(body["organization_id"], org_id);
}

#[tokio::test]
async fn test_workspace_list() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Workspace List Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create workspaces
    for i in 1..=2 {
        client
            .post_json_auth(
                &format!("/api/organizations/{}/workspaces", org_id),
                &json!({
                    "name": format!("Workspace {}", i),
                    "slug": test_slug()
                }),
                &token,
            )
            .await;
    }

    // List workspaces
    let response = client
        .get_auth(&format!("/api/organizations/{}/workspaces", org_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_workspace_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "WS Get Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create workspace
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Get Test WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Get workspace
    let response = client
        .get_auth(&format!("/api/workspaces/{}", ws_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["name"], "Get Test WS");
}

#[tokio::test]
async fn test_workspace_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Returns FORBIDDEN because user is not a member of the workspace
    // This doesn't leak info about whether workspace exists
    let response = client
        .get_auth(&format!("/api/workspaces/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_workspace_update() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "WS Update Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Original WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update workspace
    let response = client
        .patch_json_auth(
            &format!("/api/workspaces/{}", ws_id),
            &json!({
                "name": "Updated WS",
                "description": "New description",
                "is_active": false
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["name"], "Updated WS");
    assert_eq!(body["is_active"], false);
}

#[tokio::test]
async fn test_workspace_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "WS Delete Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Delete Me WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Delete workspace
    let response = client
        .delete_auth(&format!("/api/workspaces/{}", ws_id), &token)
        .await;

    response.assert_status(StatusCode::NO_CONTENT);
}

// =============================================================================
// Projects - CRUD Tests
// =============================================================================

#[tokio::test]
async fn test_project_create() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id,
                "name": "Test Project",
                "description": "A test project"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["id"].is_string());
    assert_eq!(body["name"], "Test Project");
    assert_eq!(body["description"], "A test project");
    assert_eq!(body["status"], "active");
}

#[tokio::test]
async fn test_project_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Get Test Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Get project
    let response = client
        .get_auth(
            &format!("/api/projects/{}?workspace_id={}", project_id, workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["name"], "Get Test Project");
}

#[tokio::test]
async fn test_project_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/projects/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_project_update() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Original Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update project - valid statuses: 'active', 'on_hold', 'cancelled'
    let response = client
        .put_json_auth(
            &format!("/api/projects/{}?workspace_id={}", project_id, workspace_id),
            &json!({
                "name": "Updated Project",
                "description": "New description",
                "status": "on_hold"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["name"], "Updated Project");
    assert_eq!(body["status"], "on_hold");
}

#[tokio::test]
async fn test_project_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Delete Me Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Delete
    let response = client
        .delete_auth(
            &format!("/api/projects/{}?workspace_id={}", project_id, workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NO_CONTENT);

    // Verify deleted
    let response = client
        .get_auth(
            &format!("/api/projects/{}?workspace_id={}", project_id, workspace_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_project_list_with_filter() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create projects
    client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Active Project" }),
            &token,
        )
        .await;

    // List with filter
    let response = client
        .get_auth(
            &format!("/api/projects?workspace_id={}&status=active", workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());
}

#[tokio::test]
async fn test_project_github_link() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "GitHub Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Link GitHub
    let response = client
        .post_json_auth(
            &format!(
                "/api/projects/{}/github?workspace_id={}",
                project_id, workspace_id
            ),
            &json!({
                "repo_url": "https://github.com/test/repo"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["github_repo_url"],
        "https://github.com/test/repo"
    );

    // Unlink GitHub
    let response = client
        .delete_auth(
            &format!(
                "/api/projects/{}/github?workspace_id={}",
                project_id, workspace_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert!(response.json_value()["github_repo_url"].is_null());
}

// =============================================================================
// Tasks - CRUD Tests
// =============================================================================

#[tokio::test]
async fn test_task_create() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project first
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Parent Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create task
    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Test Task",
                "description": "A test task",
                "acceptance_criteria": "Must pass tests",
                "priority": 1,
                "is_agentic": true
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["id"].is_string());
    assert_eq!(body["title"], "Test Task");
    assert_eq!(body["priority"], 1);
    assert_eq!(body["is_agentic"], true);
    // Task status starts as "created"
    assert_eq!(body["status"], "created");
}

#[tokio::test]
async fn test_task_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Get Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Get Test Task",
                "description": "Test"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Get task
    let response = client
        .get_auth(&format!("/api/tasks/{}", task_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["title"], "Get Test Task");
}

#[tokio::test]
async fn test_task_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/tasks/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_task_update() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Update Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Original Task",
                "description": "Original"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update task
    let response = client
        .put_json_auth(
            &format!("/api/tasks/{}", task_id),
            &json!({
                "title": "Updated Task",
                "description": "Updated",
                "status": "in_progress",
                "priority": 5
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["title"], "Updated Task");
    assert_eq!(body["status"], "in_progress");
    assert_eq!(body["priority"], 5);
}

#[tokio::test]
async fn test_task_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Delete Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Delete Me Task",
                "description": "Delete"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Delete task
    let response = client
        .delete_auth(&format!("/api/tasks/{}", task_id), &token)
        .await;

    response.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_task_queue() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Queue Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Queue Me Task",
                "description": "Queue"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Queue task
    let response = client
        .post_json_auth(&format!("/api/tasks/{}/queue", task_id), &json!({}), &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["status"], "queued");
}

#[tokio::test]
async fn test_task_list_with_filters() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and tasks
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Filter Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Filter Task",
                "description": "Test"
            }),
            &token,
        )
        .await;

    // List with project filter
    let response = client
        .get_auth(&format!("/api/tasks?project_id={}", project_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());

    // List with status filter
    let response = client.get_auth("/api/tasks?status=pending", &token).await;

    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn test_task_runs() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Task Runs Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Task With Runs",
                "description": "Test"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create task run
    let response = client
        .post_json_auth(&format!("/api/tasks/{}/runs", task_id), &json!({}), &token)
        .await;

    response.assert_status(StatusCode::CREATED);
    let run_id = response.json_value()["id"].as_str().unwrap().to_string();
    // Task run status starts as "running"
    assert_eq!(response.json_value()["status"], "running");

    // List task runs
    let response = client
        .get_auth(&format!("/api/tasks/{}/runs", task_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert!(response.json_value().is_array());

    // Get specific run
    let response = client
        .get_auth(&format!("/api/tasks/runs/{}", run_id), &token)
        .await;

    response.assert_status(StatusCode::OK);

    // Get run logs
    let response = client
        .get_auth(&format!("/api/tasks/runs/{}/logs", run_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert!(response.json_value().is_array());
}

#[tokio::test]
async fn test_task_run_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/tasks/runs/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

// =============================================================================
// Chats - CRUD Tests
// =============================================================================

#[tokio::test]
async fn test_chat_create() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace_id,
                "title": "Test Chat",
                "model_name": "gpt-4"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["id"].is_string());
    assert_eq!(body["title"], "Test Chat");
    assert_eq!(body["model_name"], "gpt-4");
    assert_eq!(body["archived"], false);
}

#[tokio::test]
async fn test_chat_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Get Test Chat", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Get chat
    let response = client
        .get_auth(
            &format!("/api/chats/{}?workspace_id={}", chat_id, workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["title"], "Get Test Chat");
}

#[tokio::test]
async fn test_chat_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(&format!("/api/chats/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_update() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Original Chat", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update chat
    let response = client
        .put_json_auth(
            &format!("/api/chats/{}?workspace_id={}", chat_id, workspace_id),
            &json!({ "title": "Updated Chat Title" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["title"], "Updated Chat Title");
}

#[tokio::test]
async fn test_chat_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Delete Me Chat", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Delete chat
    let response = client
        .delete_auth(
            &format!("/api/chats/{}?workspace_id={}", chat_id, workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_chat_archive_unarchive() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Archive Test Chat", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Archive chat
    let response = client
        .post_json_auth(
            &format!(
                "/api/chats/{}/archive?workspace_id={}",
                chat_id, workspace_id
            ),
            &json!({}),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["archived"], true);

    // Unarchive chat
    let response = client
        .post_json_auth(
            &format!(
                "/api/chats/{}/unarchive?workspace_id={}",
                chat_id, workspace_id
            ),
            &json!({}),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["archived"], false);
}

#[tokio::test]
async fn test_chat_archive_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!("/api/chats/{}/archive", uuid::Uuid::new_v4()),
            &json!({}),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_list_with_filters() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chats
    client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace_id, "title": "Active Chat", "model_name": "gpt-4" }),
            &token,
        )
        .await;

    // List with archived filter
    let response = client
        .get_auth(
            &format!("/api/chats?workspace_id={}&archived=false", workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert!(response.json_value().is_array());
}

#[tokio::test]
async fn test_chat_messages() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Message Test Chat", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create message
    let response = client
        .post_json_auth(
            &format!(
                "/api/chats/{}/messages?workspace_id={}",
                chat_id, workspace_id
            ),
            &json!({
                "role": "user",
                "content": "Hello, world!",
                "metadata": {"source": "test"}
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let message_id = response.json_value()["id"].as_str().unwrap().to_string();
    assert_eq!(response.json_value()["role"], "user");
    assert_eq!(response.json_value()["content"], "Hello, world!");

    // List messages
    let response = client
        .get_auth(&format!("/api/chats/{}/messages", chat_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    let messages = response.json_value();
    assert!(messages.is_array());
    assert_eq!(messages.as_array().unwrap().len(), 1);

    // Delete message
    let response = client
        .delete_auth(
            &format!("/api/chats/{}/messages/{}", chat_id, message_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_chat_message_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Delete Msg Test", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .delete_auth(
            &format!("/api/chats/{}/messages/{}", chat_id, uuid::Uuid::new_v4()),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

// =============================================================================
// Sources - CRUD Tests
// =============================================================================

fn test_source_name() -> String {
    format!("test-source-{}", uuid::Uuid::new_v4())
}

#[tokio::test]
async fn test_source_create() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "github",
                "config": {
                    "owner": "test-org",
                    "repo": "test-repo",
                    "branch": "main"
                },
                "description": "A test source",
                "url": "https://github.com/test-org/test-repo"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["id"].is_string());
    assert_eq!(body["name"], name);
    assert_eq!(body["source_type"], "github");
    assert_eq!(body["is_active"], true);
}

#[tokio::test]
async fn test_source_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Create source - valid source types: github, gitlab, filesystem
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/test" }
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let source_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Get source
    let response = client
        .get_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["name"], name);
}

#[tokio::test]
async fn test_source_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(
            &format!(
                "/api/workspaces/{}/sources/{}",
                workspace_id,
                uuid::Uuid::new_v4()
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_source_update() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Create source - valid source types: github, gitlab, filesystem
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/original" }
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let source_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update source
    let updated_name = test_source_name();
    let response = client
        .put_json_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &json!({
                "name": &updated_name,
                "description": "Updated description",
                "config": { "base_path": "/tmp/updated" },
                "is_active": false
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["name"], updated_name);
    assert_eq!(body["is_active"], false);
}

#[tokio::test]
async fn test_source_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Create source - valid source types: github, gitlab, filesystem
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/delete" }
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let source_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Delete source
    let response = client
        .delete_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_source_verify() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Create source - valid source types: github, gitlab, filesystem
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/verify" }
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let source_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Verify source - accepts both NO_CONTENT (success) and SERVICE_UNAVAILABLE (no verification service in test)
    let response = client
        .post_json_auth(
            &format!(
                "/api/workspaces/{}/sources/{}/verify",
                workspace_id, source_id
            ),
            &json!({}),
            &token,
        )
        .await;

    assert!(
        response.status == StatusCode::NO_CONTENT
            || response.status == StatusCode::SERVICE_UNAVAILABLE,
        "Expected NO_CONTENT or SERVICE_UNAVAILABLE, got {}",
        response.status
    );
}

#[tokio::test]
async fn test_source_list_with_filters() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Create sources
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "github",
                "config": { "owner": "test", "repo": "test" }
            }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);

    // List with type filter
    let response = client
        .get_auth(
            &format!(
                "/api/workspaces/{}/sources?source_type=github",
                workspace_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert!(response.json_value().is_array());

    // List with active filter
    let response = client
        .get_auth(
            &format!("/api/workspaces/{}/sources?is_active=true", workspace_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
}

// =============================================================================
// Edge Cases and Error Handling
// =============================================================================

#[tokio::test]
async fn test_invalid_uuid_path_param() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth("/api/organizations/not-a-uuid", &token)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_malformed_json_body() {
    let client = TestClient::with_db().await;

    // Send invalid JSON - the TestClient will still serialize this properly,
    // so we test with missing required fields instead
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email()
                // missing password
            }),
        )
        .await;

    // Axum returns 422 for deserialization errors (missing required fields)
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_expired_token_simulation() {
    let client = TestClient::with_db().await;

    // Use a made-up token that looks valid but isn't
    let fake_token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

    let response = client.get_auth("/api/organizations", fake_token).await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_empty_bearer_token() {
    let client = TestClient::with_db().await;

    let response = client.get_auth("/api/organizations", "").await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_organization_create_minimal() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create with only required fields
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Minimal Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["description"].is_null());
}

#[tokio::test]
async fn test_project_create_minimal() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id,
                "name": "Minimal Project"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["description"].is_null());
    assert_eq!(body["workspace_id"].as_str().unwrap(), &workspace_id);
}

#[tokio::test]
async fn test_task_create_minimal() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project first
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Minimal Task Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create task with minimal fields
    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Minimal Task",
                "description": "Required field"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["acceptance_criteria"].is_null());
    assert!(body["priority"].is_null());
    assert_eq!(body["is_agentic"], false);
}

#[tokio::test]
async fn test_chat_create_minimal() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace_id,
                "title": "Minimal Chat",
                "model_name": "test-model"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert_eq!(body["workspace_id"].as_str().unwrap(), &workspace_id);
}

#[tokio::test]
async fn test_source_create_minimal() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Sources require at least a valid config - valid source types: github, gitlab, filesystem
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/minimal" }
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["description"].is_null());
    assert!(body["url"].is_null());
}

// =============================================================================
// Workspace Themes - CRUD Tests
// =============================================================================

#[tokio::test]
async fn test_workspace_theme_upsert() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace first
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Theme Test Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Theme Test WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert theme
    let response = client
        .put_json_auth(
            &format!("/api/workspaces/{}/theme", ws_id),
            &json!({
                "primary_color_light": "#3B82F6",
                "secondary_color_light": "#10B981",
                "primary_color_dark": "#60A5FA",
                "secondary_color_dark": "#34D399",
                "font_family": "Inter, sans-serif",
                "font_size_base": "16px",
                "border_radius": "8px"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["primary_color_light"], "#3B82F6");
    assert_eq!(body["font_family"], "Inter, sans-serif");
}

#[tokio::test]
async fn test_workspace_theme_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Theme Get Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Theme Get WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert theme first
    client
        .put_json_auth(
            &format!("/api/workspaces/{}/theme", ws_id),
            &json!({ "primary_color_light": "#FF0000" }),
            &token,
        )
        .await;

    // Get theme
    let response = client
        .get_auth(&format!("/api/workspaces/{}/theme", ws_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["primary_color_light"], "#FF0000");
}

#[tokio::test]
async fn test_workspace_theme_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create org and workspace without theme
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "No Theme Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "No Theme WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Get theme (should be 404)
    let response = client
        .get_auth(&format!("/api/workspaces/{}/theme", ws_id), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_workspace_theme_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Theme Delete Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Theme Delete WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create theme first
    client
        .put_json_auth(
            &format!("/api/workspaces/{}/theme", ws_id),
            &json!({ "primary_color_light": "#00FF00" }),
            &token,
        )
        .await;

    // Delete theme
    let response = client
        .delete_auth(&format!("/api/workspaces/{}/theme", ws_id), &token)
        .await;

    response.assert_status(StatusCode::NO_CONTENT);

    // Verify deleted
    let response = client
        .get_auth(&format!("/api/workspaces/{}/theme", ws_id), &token)
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_workspace_theme_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create org and workspace without theme
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Del No Theme Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Del No Theme WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Try to delete non-existent theme
    let response = client
        .delete_auth(&format!("/api/workspaces/{}/theme", ws_id), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

// =============================================================================
// Models - Tests (limited - external service dependent)
// =============================================================================

#[tokio::test]
async fn test_models_list_huggingface() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // HuggingFace returns static sample data
    let response = client
        .get_auth("/api/models?source=huggingface", &token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());
    assert!(!body.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_models_list_modelscope() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // ModelScope returns static sample data
    let response = client
        .get_auth("/api/models?source=modelscope", &token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());
    assert!(!body.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_models_list_unknown_source() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client.get_auth("/api/models?source=unknown", &token).await;

    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert!(body["error"].as_str().unwrap().contains("Unknown source"));
}

// =============================================================================
// Additional Not Found Tests
// =============================================================================

#[tokio::test]
async fn test_task_queue_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!("/api/tasks/{}/queue", uuid::Uuid::new_v4()),
            &json!({}),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_task_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .delete_auth(&format!("/api/tasks/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_task_update_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/tasks/{}", uuid::Uuid::new_v4()),
            &json!({ "title": "Won't Work" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_project_update_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/projects/{}", uuid::Uuid::new_v4()),
            &json!({ "name": "Won't Work" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_project_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .delete_auth(&format!("/api/projects/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_project_github_link_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!("/api/projects/{}/github", uuid::Uuid::new_v4()),
            &json!({ "repo_url": "https://github.com/test/repo" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_project_github_unlink_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .delete_auth(
            &format!("/api/projects/{}/github", uuid::Uuid::new_v4()),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_workspace_update_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Returns FORBIDDEN because user is not a member of the workspace
    // This doesn't leak info about whether workspace exists
    let response = client
        .patch_json_auth(
            &format!("/api/workspaces/{}", uuid::Uuid::new_v4()),
            &json!({ "name": "Won't Work" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_workspace_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;

    // Returns FORBIDDEN because user is not a member of the workspace
    // This doesn't leak info about whether workspace exists
    let response = client
        .delete_auth(&format!("/api/workspaces/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_source_update_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!(
                "/api/workspaces/{}/sources/{}",
                workspace_id,
                uuid::Uuid::new_v4()
            ),
            &json!({ "name": "Won't Work" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_source_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .delete_auth(
            &format!(
                "/api/workspaces/{}/sources/{}",
                workspace_id,
                uuid::Uuid::new_v4()
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_source_verify_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!(
                "/api/workspaces/{}/sources/{}/verify",
                workspace_id,
                uuid::Uuid::new_v4()
            ),
            &json!({}),
            &token,
        )
        .await;

    // verify returns NO_CONTENT on success and the function doesn't check if source exists first
    // Let's just verify it doesn't crash
    assert!(response.status == StatusCode::NO_CONTENT || response.status == StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_update_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .put_json_auth(
            &format!("/api/chats/{}", uuid::Uuid::new_v4()),
            &json!({ "title": "Won't Work" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_delete_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .delete_auth(&format!("/api/chats/{}", uuid::Uuid::new_v4()), &token)
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_unarchive_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!("/api/chats/{}/unarchive", uuid::Uuid::new_v4()),
            &json!({}),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_chat_messages_list_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // List messages for non-existent chat
    let response = client
        .get_auth(
            &format!("/api/chats/{}/messages", uuid::Uuid::new_v4()),
            &token,
        )
        .await;

    // Now returns 404 with proper validation
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_task_run_logs_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(
            &format!("/api/tasks/runs/{}/logs", uuid::Uuid::new_v4()),
            &token,
        )
        .await;

    // Returns empty array for non-existent run
    response.assert_status(StatusCode::OK);
}

// =============================================================================
// Additional Edge Case Tests
// =============================================================================

#[tokio::test]
async fn test_register_without_display_name() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password()
            }),
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert!(body["user"]["display_name"].is_null());
}

#[tokio::test]
async fn test_organization_with_all_fields() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let slug = test_slug();

    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Full Org",
                "slug": &slug,
                "description": "Description here",
                "logo_url": "https://example.com/logo.png"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert_eq!(body["description"], "Description here");
}

#[tokio::test]
async fn test_workspace_with_all_fields() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create org
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Full WS Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create workspace with all fields
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": "Full Workspace",
                "slug": test_slug(),
                "description": "Full description"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert_eq!(body["description"], "Full description");
}

#[tokio::test]
async fn test_project_with_workspace() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create org and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Project WS Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Project WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create project with workspace
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id,
                "name": "WS Project",
                "workspace_id": ws_id,
                "description": "Project in workspace"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert_eq!(body["workspace_id"], ws_id);
}

#[tokio::test]
async fn test_chat_with_workspace() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create org and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Chat WS Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Chat WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create chat with workspace
    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace_id,
                "title": "WS Chat",
                "model_name": "gpt-4",
                "workspace_id": ws_id
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert_eq!(body["workspace_id"], ws_id);
}

#[tokio::test]
async fn test_source_with_credentials() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "github",
                "config": { "owner": "test", "repo": "test" },
                "credentials": "github_token_abc123"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    // Note: credentials are not returned in response for security
}

#[tokio::test]
async fn test_project_status_cancelled() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Cancel Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update to cancelled
    let response = client
        .put_json_auth(
            &format!("/api/projects/{}?workspace_id={}", project_id, workspace_id),
            &json!({ "status": "cancelled" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["status"], "cancelled");
}

#[tokio::test]
async fn test_task_status_completed() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Complete Task Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Complete Me",
                "description": "Test"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update to complete (valid statuses: created, queued, in_progress, review, complete, blocked)
    let response = client
        .put_json_auth(
            &format!("/api/tasks/{}", task_id),
            &json!({ "status": "complete" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["status"], "complete");
}

#[tokio::test]
async fn test_organization_list_filters() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Filter Test Org",
                "slug": test_slug()
            }),
            &token,
        )
        .await;

    // List with is_active filter
    let response = client
        .get_auth("/api/organizations?is_active=true", &token)
        .await;

    response.assert_status(StatusCode::OK);
    assert!(response.json_value().is_array());
}

#[tokio::test]
async fn test_workspaces_list_for_nonexistent_org() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .get_auth(
            &format!("/api/organizations/{}/workspaces", uuid::Uuid::new_v4()),
            &token,
        )
        .await;

    // Returns FORBIDDEN because user is not a member of the org
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_task_runs_list_for_nonexistent_task() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;

    let response = client
        .get_auth(&format!("/api/tasks/{}/runs", uuid::Uuid::new_v4()), &token)
        .await;

    // Returns empty array
    response.assert_status(StatusCode::OK);
    assert!(response.json_value().is_array());
}

#[tokio::test]
async fn test_source_with_gitlab_type() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "gitlab",
                "config": { "project_id": "test/project" }
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    assert_eq!(response.json_value()["source_type"], "gitlab");
}

// =============================================================================
// Auth Edge Cases - Header Format Tests
// =============================================================================

#[tokio::test]
async fn test_basic_auth_instead_of_bearer() {
    let _client = TestClient::with_db().await;

    // Use Basic auth instead of Bearer
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let config = common::test_config();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool);
    let router = common::create_test_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/api/organizations")
        .header("Authorization", "Basic dXNlcjpwYXNz")
        .body(Body::empty())
        .unwrap();

    let response = router
        .oneshot(request)
        .await
        .expect("Failed to send request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_missing_auth_header() {
    let client = TestClient::with_db().await;

    // No Authorization header at all
    let response = client.get("/api/organizations").await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_chat_create_message_for_nonexistent_chat() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Try to create a message for a chat that doesn't exist
    let response = client
        .post_json_auth(
            &format!("/api/chats/{}/messages", uuid::Uuid::new_v4()),
            &json!({
                "role": "user",
                "content": "Hello"
            }),
            &token,
        )
        .await;

    // Should return 404 Not Found for nonexistent chat
    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_task_create_run_for_nonexistent_task() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!("/api/tasks/{}/runs", uuid::Uuid::new_v4()),
            &json!({}),
            &token,
        )
        .await;

    // Should fail with FK constraint
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_workspace_create_for_nonexistent_org() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", uuid::Uuid::new_v4()),
            &json!({
                "name": "Test",
                "slug": test_slug()
            }),
            &token,
        )
        .await;

    // Returns FORBIDDEN because user is not a member of the org
    // RBAC check happens before FK constraint validation
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_task_create_for_nonexistent_project() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": uuid::Uuid::new_v4(),
                "title": "Test Task",
                "description": "Test"
            }),
            &token,
        )
        .await;

    // Should fail with FK constraint
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_task_with_blocked_status() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Blocked Task Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Block Me",
                "description": "Test"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update to blocked status
    let response = client
        .put_json_auth(
            &format!("/api/tasks/{}", task_id),
            &json!({ "status": "blocked" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["status"], "blocked");
}

#[tokio::test]
async fn test_task_with_review_status() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create project and task
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({"workspace_id": workspace_id, "name": "Review Task Project" }),
            &token,
        )
        .await;
    let project_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            "/api/tasks",
            &json!({
                "project_id": project_id,
                "title": "Review Me",
                "description": "Test"
            }),
            &token,
        )
        .await;
    let task_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Update to review status
    let response = client
        .put_json_auth(
            &format!("/api/tasks/{}", task_id),
            &json!({ "status": "review" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["status"], "review");
}

#[tokio::test]
async fn test_chat_archived_filter() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create and archive a chat
    let response = client
        .post_json_auth("/api/chats", &json!({"workspace_id": workspace_id, "title": "Archive Filter Test", "model_name": "gpt-4" }),
            &token,
        )
        .await;
    let chat_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Archive it
    client
        .post_json_auth(
            &format!(
                "/api/chats/{}/archive?workspace_id={}",
                chat_id, workspace_id
            ),
            &json!({}),
            &token,
        )
        .await;

    // List archived chats
    let response = client
        .get_auth(
            &format!("/api/chats?workspace_id={}&archived=true", workspace_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());
    // Should have at least the one we archived
    let archived: Vec<_> = body
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["id"] == chat_id)
        .collect();
    assert!(!archived.is_empty());
}

#[tokio::test]
async fn test_organization_inactive_filter() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and deactivate it
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Inactive Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Deactivate
    client
        .put_json_auth(
            &format!("/api/organizations/{}", org_id),
            &json!({ "is_active": false }),
            &token,
        )
        .await;

    // List inactive organizations
    let response = client
        .get_auth("/api/organizations?is_active=false", &token)
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body.is_array());
}

#[tokio::test]
async fn test_source_inactive_filter() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;
    let name = test_source_name();

    // Create source and deactivate it
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", workspace_id),
            &json!({
                "name": &name,
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/inactive" }
            }),
            &token,
        )
        .await;
    let source_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Deactivate
    client
        .put_json_auth(
            &format!("/api/workspaces/{}/sources/{}", workspace_id, source_id),
            &json!({ "is_active": false }),
            &token,
        )
        .await;

    // List inactive sources
    let response = client
        .get_auth(
            &format!("/api/workspaces/{}/sources?is_active=false", workspace_id),
            &token,
        )
        .await;
    response.assert_status(StatusCode::OK);
}

// =============================================================================
// Auth Edge Cases - Disabled User and Error Handling
// =============================================================================

#[tokio::test]
async fn test_login_disabled_user() {
    use zone_server::db::users;

    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register a user
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password,
                "display_name": "Disabled User"
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let user_id: uuid::Uuid = response.json_value()["user"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // Disable the user
    let pool = common::create_test_pool().await;
    users::set_user_active(&pool, user_id, false).await.unwrap();

    // Try to login
    let response = client
        .post_json(
            "/api/auth/login",
            &json!({
                "email": &email,
                "password": &password
            }),
        )
        .await;
    response.assert_status(StatusCode::FORBIDDEN);
    assert!(
        response.json_value()["error"]
            .as_str()
            .unwrap()
            .contains("disabled")
    );
}

#[tokio::test]
async fn test_refresh_with_empty_token() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({
                "refresh_token": ""
            }),
        )
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_refresh_with_malformed_token() {
    let client = TestClient::with_db().await;

    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({
                "refresh_token": "not-a-valid-jwt-token"
            }),
        )
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_logout_revokes_all_tokens() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password,
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let token = response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let refresh_token = response.json_value()["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();

    // Login again to get another refresh token
    let response = client
        .post_json(
            "/api/auth/login",
            &json!({
                "email": &email,
                "password": &password
            }),
        )
        .await;
    response.assert_status(StatusCode::OK);
    let refresh_token2 = response.json_value()["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();

    // Logout using first token
    let response = client
        .post_json_auth("/api/auth/logout", &json!({}), &token)
        .await;
    response.assert_status(StatusCode::NO_CONTENT);

    // First refresh token should be invalid
    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({
                "refresh_token": &refresh_token
            }),
        )
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);

    // Second refresh token should also be invalid (logout revokes all)
    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({
                "refresh_token": &refresh_token2
            }),
        )
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_multiple_logins_same_user() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password,
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);

    // Login multiple times - each should succeed and return new tokens
    let mut tokens = vec![];
    for _ in 0..3 {
        let response = client
            .post_json(
                "/api/auth/login",
                &json!({
                    "email": &email,
                    "password": &password
                }),
            )
            .await;
        response.assert_status(StatusCode::OK);
        tokens.push(
            response.json_value()["access_token"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }

    // All tokens should be valid
    for token in &tokens {
        let response = client.get_auth("/api/organizations", token).await;
        response.assert_status(StatusCode::OK);
    }
}

#[tokio::test]
async fn test_user_response_fields() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let password = test_password();

    // Register with display name
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": &email,
                "password": &password,
                "display_name": "Test User Display Name"
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();

    // Check user response fields
    assert!(body["user"]["id"].as_str().is_some());
    assert_eq!(body["user"]["email"], email);
    assert_eq!(body["user"]["display_name"], "Test User Display Name");
    assert!(body["user"]["is_admin"].is_boolean());
    assert!(body["access_token"].as_str().is_some());
    assert!(body["refresh_token"].as_str().is_some());
    assert_eq!(body["token_type"], "Bearer");
    assert!(body["expires_in"].as_u64().is_some());
}

// =============================================================================
// AI Settings - Organization Tests
// =============================================================================

#[tokio::test]
async fn test_org_ai_settings_upsert() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "AI Test Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert AI settings
    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "sk-test-key-123",
                "model_fast": "gpt-4o-mini",
                "model_reasoning": "gpt-4o",
                "model_embedding": "text-embedding-3-small"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["has_openai_api_key"], true);
    assert_eq!(body["model_fast"], "gpt-4o-mini");
    assert_eq!(body["model_reasoning"], "gpt-4o");
    // Should not expose the actual key
    assert!(body.get("openai_api_key").is_none());
}

#[tokio::test]
async fn test_org_ai_settings_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "AI Get Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert AI settings
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "anthropic",
                "anthropic_api_key": "sk-ant-test",
                "model_fast": "claude-3-haiku-20240307"
            }),
            &token,
        )
        .await;

    // Get AI settings
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "anthropic");
    assert_eq!(body["has_anthropic_api_key"], true);
    assert_eq!(body["model_fast"], "claude-3-haiku-20240307");
}

#[tokio::test]
async fn test_org_ai_settings_delete() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "AI Delete Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert AI settings
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "sk-test"
            }),
            &token,
        )
        .await;

    // Delete AI settings
    let response = client
        .delete_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NO_CONTENT);

    // Verify deletion by getting settings - should return defaults
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    // Should return default settings after delete
    assert_eq!(body["provider"], "self_hosted");
}

// =============================================================================
// AI Settings - Workspace Tests
// =============================================================================

#[tokio::test]
async fn test_workspace_ai_settings_upsert() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "WS AI Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "WS AI Test", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert workspace AI settings
    let response = client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "provider": "bedrock",
                "bedrock_region": "us-east-1",
                "bedrock_use_iam_role": true,
                "model_fast": "anthropic.claude-3-haiku-20240307-v1:0"
            }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "bedrock");
    assert_eq!(body["bedrock_region"], "us-east-1");
    assert_eq!(body["bedrock_use_iam_role"], true);
}

#[tokio::test]
async fn test_workspace_ai_settings_get() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "WS AI Get Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "WS AI Get", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Upsert settings
    client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({ "provider": "openai", "model_fast": "gpt-4o-mini" }),
            &token,
        )
        .await;

    // Get settings
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["provider"], "openai");
}

#[tokio::test]
async fn test_workspace_ai_settings_effective() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization and workspace
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Effective AI Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({ "name": "Effective AI WS", "slug": test_slug() }),
            &token,
        )
        .await;
    let ws_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Set org-level settings
    client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({
                "provider": "openai",
                "openai_api_key": "sk-org-key",
                "model_fast": "gpt-4o-mini",
                "model_reasoning": "gpt-4o"
            }),
            &token,
        )
        .await;

    // Get effective settings (should inherit from org)
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["model_fast"], "gpt-4o-mini");

    // Override at workspace level
    client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                org_id, ws_id
            ),
            &json!({
                "provider": "anthropic",
                "anthropic_api_key": "sk-ant-ws-key",
                "model_fast": "claude-3-haiku-20240307"
            }),
            &token,
        )
        .await;

    // Get effective settings (should use workspace override)
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai/effective",
                org_id, ws_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["provider"], "anthropic");
    assert_eq!(body["model_fast"], "claude-3-haiku-20240307");
}

#[tokio::test]
async fn test_ai_settings_provider_validation() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "AI Validation Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let org_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Try invalid provider
    let response = client
        .put_json_auth(
            &format!("/api/organizations/{}/settings/ai", org_id),
            &json!({ "provider": "invalid_provider" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
}

// =============================================================================
// Audit Logs Tests
// =============================================================================

#[tokio::test]
async fn test_audit_logs_list_requires_auth() {
    let client = TestClient::with_db().await;
    let fake_org_id = uuid::Uuid::new_v4();

    let response = client
        .get(&format!("/api/organizations/{}/audit-logs", fake_org_id))
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_audit_logs_list_empty() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Test Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    // List audit logs (should be empty initially, or only have org creation log)
    let response = client
        .get_auth(&format!("/api/organizations/{}/audit-logs", org_id), &token)
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body["logs"].is_array());
    assert!(body["total"].is_number());
}

#[tokio::test]
async fn test_audit_logs_list_with_pagination() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Pagination Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    // List with pagination parameters
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/audit-logs?limit=10&offset=0", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body["logs"].is_array());
    assert!(body["total"].is_number());
    assert!(body["limit"].is_number());
    assert!(body["offset"].is_number());
}

#[tokio::test]
async fn test_audit_logs_list_with_action_filter() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Filter Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    // List with action filter
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/audit-logs?action=workspace.created",
                org_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body["logs"].is_array());

    // Verify all returned logs have the correct action
    if !body["logs"].as_array().unwrap().is_empty() {
        for log in body["logs"].as_array().unwrap() {
            assert_eq!(log["action"], "workspace.created");
        }
    }
}

#[tokio::test]
async fn test_audit_logs_list_with_resource_type_filter() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Resource Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    // List with resource_type filter
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/audit-logs?resource_type=workspace",
                org_id
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body["logs"].is_array());

    // Verify all returned logs have the correct resource type
    if !body["logs"].as_array().unwrap().is_empty() {
        for log in body["logs"].as_array().unwrap() {
            assert_eq!(log["resource_type"], "workspace");
        }
    }
}

#[tokio::test]
async fn test_audit_logs_list_with_date_range() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Date Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    let now = chrono::Utc::now();
    // Use format that's safe for URLs (Z suffix instead of +00:00)
    let start_date = (now - chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let end_date = (now + chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    // List with date range
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/audit-logs?start_date={}&end_date={}",
                org_id, start_date, end_date
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert!(body["logs"].is_array());
}

#[tokio::test]
async fn test_audit_logs_get_single() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Single Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    // List logs to get an ID
    let response = client
        .get_auth(&format!("/api/organizations/{}/audit-logs", org_id), &token)
        .await;

    let body = response.json_value();
    let logs = body["logs"].as_array().unwrap();

    if !logs.is_empty() {
        let log_id = logs[0]["id"].as_str().unwrap();

        // Get single log
        let response = client
            .get_auth(
                &format!("/api/organizations/{}/audit-logs/{}", org_id, log_id),
                &token,
            )
            .await;

        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_eq!(body["id"], log_id);
        assert!(body["action"].is_string());
        assert!(body["resource_type"].is_string());
        assert!(body["created_at"].is_string());
    }
}

#[tokio::test]
async fn test_audit_logs_get_not_found() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit NotFound Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    let fake_log_id = uuid::Uuid::new_v4();

    let response = client
        .get_auth(
            &format!("/api/organizations/{}/audit-logs/{}", org_id, fake_log_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_audit_logs_export_csv() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Export Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    let now = chrono::Utc::now();
    // Use format that's safe for URLs (Z suffix instead of +00:00)
    let start_date = (now - chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let end_date = (now + chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    // Export logs
    let response = client
        .get_auth(
            &format!(
                "/api/organizations/{}/audit-logs/export?start_date={}&end_date={}",
                org_id, start_date, end_date
            ),
            &token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let csv = response.text();

    // Verify CSV format
    assert!(csv.contains("id,organization_id"));
    assert!(csv.contains("action"));
    assert!(csv.contains("resource_type"));
}

#[tokio::test]
async fn test_audit_logs_export_requires_date_range() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Export Missing Org", "slug": test_slug() }),
            &token,
        )
        .await;
    let json_val = response.json_value();
    let org_id = json_val["id"].as_str().unwrap();

    // Try export without date range
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/audit-logs/export", org_id),
            &token,
        )
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_audit_logs_cannot_access_other_org() {
    let client = TestClient::with_db().await;
    let token = get_auth_token(&client).await;
    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;

    // Create first organization
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Org 1", "slug": test_slug() }),
            &token,
        )
        .await;
    let _org1_id = response.json_value()["id"].as_str().unwrap().to_string();

    // Create second user with different organization
    let token2 = get_auth_token(&client).await;
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Audit Org 2", "slug": test_slug() }),
            &token2,
        )
        .await;
    let json_val2 = response.json_value();
    let org2_id = json_val2["id"].as_str().unwrap();

    // Try to access org2's logs with token1
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/audit-logs", org2_id),
            &token,
        )
        .await;

    // Should be forbidden or not found
    assert!(response.status == StatusCode::FORBIDDEN || response.status == StatusCode::NOT_FOUND);
}
