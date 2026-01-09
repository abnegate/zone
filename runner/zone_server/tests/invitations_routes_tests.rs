//! Integration tests for invitation routes
//!
//! Tests covering the HTTP endpoints for invitation management

mod common;

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use common::{TestClient, test_email, test_password};

/// Helper to register a user and get their access token and user_id
async fn register_user(client: &TestClient, email: &str, password: &str) -> (String, Uuid) {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": email,
                "password": password,
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

/// Helper to create an organization
async fn create_organization(client: &TestClient, token: &str, name: &str, slug: &str) -> Uuid {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": name,
                "slug": slug
            }),
            token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Uuid::parse_str(body["id"].as_str().unwrap()).unwrap()
}

// =============================================================================
// Create Invitation Tests
// =============================================================================

#[tokio::test]
async fn test_create_invitation_success() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    let invite_email = test_email();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();

    assert!(body["id"].is_string());
    assert_eq!(body["email"], invite_email);
    assert_eq!(body["org_role"], "member");
    assert_eq!(body["workspace_role"], "member");
    assert!(
        body["token"].is_string(),
        "Should return token for email sending"
    );
    assert!(body["expires_at"].is_string());
}

#[tokio::test]
async fn test_create_invitation_with_workspaces() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create a workspace
    let ws_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": "Test Workspace",
                "slug": format!("ws-{}", Uuid::new_v4())
            }),
            &owner_token,
        )
        .await;

    ws_response.assert_status(StatusCode::CREATED);
    let ws_body = ws_response.json_value();
    let workspace_id = ws_body["id"].as_str().unwrap();

    let invite_email = test_email();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [workspace_id],
                "org_role": "admin",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();

    assert_eq!(body["workspace_ids"].as_array().unwrap().len(), 1);
    assert_eq!(body["org_role"], "admin");
}

#[tokio::test]
async fn test_create_invitation_requires_admin() {
    let client = TestClient::with_db().await;

    // Create org owner
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create member (not admin)
    let member_email = test_email();
    let (member_token, member_id) = register_user(&client, &member_email, &test_password()).await;

    // Add member to org with member role
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/members", org_id),
            &json!({
                "user_id": member_id,
                "role": "member"
            }),
            &owner_token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);

    // Try to create invitation as member (should fail)
    let invite_email = test_email();
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &member_token,
        )
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_create_invitation_duplicate_email_fails() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    let invite_email = test_email();

    // Create first invitation
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);

    // Try to create duplicate
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "admin",
                "workspace_role": "admin"
            }),
            &owner_token,
        )
        .await;

    response.assert_status(StatusCode::CONFLICT);
}

// =============================================================================
// List Invitations Tests
// =============================================================================

#[tokio::test]
async fn test_list_invitations() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create multiple invitations
    let email1 = test_email();
    let email2 = test_email();

    client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": email1,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": email2,
                "workspace_ids": [],
                "org_role": "admin",
                "workspace_role": "admin"
            }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    // List invitations
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &owner_token,
        )
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let invitations = body.as_array().unwrap();

    assert_eq!(invitations.len(), 2);
}

#[tokio::test]
async fn test_list_invitations_requires_admin() {
    let client = TestClient::with_db().await;

    // Create org owner
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create member (not admin)
    let member_email = test_email();
    let (member_token, member_id) = register_user(&client, &member_email, &test_password()).await;

    // Add member to org
    client
        .post_json_auth(
            &format!("/api/organizations/{}/members", org_id),
            &json!({
                "user_id": member_id,
                "role": "member"
            }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    // Try to list invitations as member (should fail)
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &member_token,
        )
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

// =============================================================================
// Revoke Invitation Tests
// =============================================================================

#[tokio::test]
async fn test_revoke_invitation() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create invitation
    let invite_email = test_email();
    let create_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    create_response.assert_status(StatusCode::CREATED);
    let json = create_response.json_value();
    let invitation_id = json["id"].as_str().unwrap();

    // Revoke invitation
    let response = client
        .delete_auth(
            &format!(
                "/api/organizations/{}/invitations/{}",
                org_id, invitation_id
            ),
            &owner_token,
        )
        .await;

    response.assert_status(StatusCode::NO_CONTENT);

    // Verify invitation is gone
    let list_response = client
        .get_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &owner_token,
        )
        .await;

    list_response.assert_status(StatusCode::OK);
    let json = list_response.json_value();
    let invitations = json.as_array().unwrap();
    assert_eq!(invitations.len(), 0);
}

#[tokio::test]
async fn test_revoke_invitation_requires_admin() {
    let client = TestClient::with_db().await;

    // Create org owner
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create invitation
    let invite_email = test_email();
    let create_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    create_response.assert_status(StatusCode::CREATED);
    let json = create_response.json_value();
    let invitation_id = json["id"].as_str().unwrap();

    // Create member (not admin)
    let member_email = test_email();
    let (member_token, member_id) = register_user(&client, &member_email, &test_password()).await;

    // Add member to org
    client
        .post_json_auth(
            &format!("/api/organizations/{}/members", org_id),
            &json!({
                "user_id": member_id,
                "role": "member"
            }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    // Try to revoke as member (should fail)
    let response = client
        .delete_auth(
            &format!(
                "/api/organizations/{}/invitations/{}",
                org_id, invitation_id
            ),
            &member_token,
        )
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

// =============================================================================
// Accept Invitation Tests
// =============================================================================

#[tokio::test]
async fn test_accept_invitation_success() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create invitation
    let invite_email = test_email();
    let create_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "admin",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    create_response.assert_status(StatusCode::CREATED);
    let json = create_response.json_value();
    let token = json["token"].as_str().unwrap();

    // Register the invited user
    let (invited_token, invited_id) = register_user(&client, &invite_email, &test_password()).await;

    // Accept invitation
    let response = client
        .post_json_auth(
            &format!("/api/invitations/{}/accept", token),
            &json!({}),
            &invited_token,
        )
        .await;

    response.assert_status(StatusCode::OK);

    // Verify user is now a member of the organization
    let members_response = client
        .get_auth(
            &format!("/api/organizations/{}/members", org_id),
            &owner_token,
        )
        .await;

    members_response.assert_status(StatusCode::OK);
    let json = members_response.json_value();
    let members = json.as_array().unwrap();

    // Should have owner + new member
    assert!(members.len() >= 2);

    let invited_id_str = invited_id.to_string();
    let invited_member = members.iter().find(|m| m["user_id"] == invited_id_str);
    assert!(invited_member.is_some(), "Invited user should be a member");
    assert_eq!(invited_member.unwrap()["role"], "admin");
}

#[tokio::test]
async fn test_accept_invitation_adds_to_workspaces() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create workspace
    let ws_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/workspaces", org_id),
            &json!({
                "name": "Test Workspace",
                "slug": format!("ws-{}", Uuid::new_v4())
            }),
            &owner_token,
        )
        .await;

    ws_response.assert_status(StatusCode::CREATED);
    let json = ws_response.json_value();
    let workspace_id = json["id"].as_str().unwrap();

    // Create invitation with workspace
    let invite_email = test_email();
    let create_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [workspace_id],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    create_response.assert_status(StatusCode::CREATED);
    let json = create_response.json_value();
    let token = json["token"].as_str().unwrap();

    // Register and accept
    let (invited_token, invited_id) = register_user(&client, &invite_email, &test_password()).await;

    let response = client
        .post_json_auth(
            &format!("/api/invitations/{}/accept", token),
            &json!({}),
            &invited_token,
        )
        .await;

    response.assert_status(StatusCode::OK);

    // Verify user is in workspace
    let ws_members_response = client
        .get_auth(
            &format!("/api/workspaces/{}/members", workspace_id),
            &owner_token,
        )
        .await;

    ws_members_response.assert_status(StatusCode::OK);
    let json = ws_members_response.json_value();
    let ws_members = json.as_array().unwrap();

    let invited_id_str = invited_id.to_string();
    let invited_ws_member = ws_members.iter().find(|m| m["user_id"] == invited_id_str);
    assert!(
        invited_ws_member.is_some(),
        "Invited user should be in workspace"
    );
    assert_eq!(invited_ws_member.unwrap()["role"], "member");
}

#[tokio::test]
async fn test_accept_invitation_invalid_token() {
    let client = TestClient::with_db().await;
    let user_email = test_email();
    let (user_token, _user_id) = register_user(&client, &user_email, &test_password()).await;

    let response = client
        .post_json_auth(
            "/api/invitations/invalid-token-12345/accept",
            &json!({}),
            &user_token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_accept_invitation_twice_fails() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create invitation
    let invite_email = test_email();
    let create_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    create_response.assert_status(StatusCode::CREATED);
    let json = create_response.json_value();
    let token = json["token"].as_str().unwrap();

    // Register and accept
    let (invited_token, _invited_id) =
        register_user(&client, &invite_email, &test_password()).await;

    client
        .post_json_auth(
            &format!("/api/invitations/{}/accept", token),
            &json!({}),
            &invited_token,
        )
        .await
        .assert_status(StatusCode::OK);

    // Try to accept again (should fail)
    let response = client
        .post_json_auth(
            &format!("/api/invitations/{}/accept", token),
            &json!({}),
            &invited_token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
}

// =============================================================================
// Get Invitation Details Tests
// =============================================================================

#[tokio::test]
async fn test_get_invitation_details() {
    let client = TestClient::with_db().await;
    let owner_email = test_email();
    let (owner_token, _owner_id) = register_user(&client, &owner_email, &test_password()).await;
    let org_id = create_organization(
        &client,
        &owner_token,
        "Test Org",
        &format!("org-{}", Uuid::new_v4()),
    )
    .await;

    // Create invitation
    let invite_email = test_email();
    let create_response = client
        .post_json_auth(
            &format!("/api/organizations/{}/invitations", org_id),
            &json!({
                "email": invite_email,
                "workspace_ids": [],
                "org_role": "admin",
                "workspace_role": "member"
            }),
            &owner_token,
        )
        .await;

    create_response.assert_status(StatusCode::CREATED);
    let json = create_response.json_value();
    let token = json["token"].as_str().unwrap();

    // Get invitation details (public route, no auth required)
    let response = client.get(&format!("/api/invitations/{}", token)).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();

    assert_eq!(body["email"], invite_email);
    assert_eq!(body["org_role"], "admin");
    assert_eq!(body["workspace_role"], "member");
    assert!(body["organization_name"].is_string());
}

#[tokio::test]
async fn test_get_invitation_details_invalid_token() {
    let client = TestClient::with_db().await;

    let response = client.get("/api/invitations/invalid-token-12345").await;

    response.assert_status(StatusCode::NOT_FOUND);
}
