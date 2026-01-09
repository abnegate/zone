//! Integration tests for invitation database queries
//!
//! Tests covering the complete invitation lifecycle using TDD approach

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use zone_server::db::{invitations, organization_members, organizations, users};

async fn create_test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string());

    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid) {
    // Create organization
    let org_id = organizations::create_organization(
        pool,
        &format!("Test Org {}", Uuid::new_v4()),
        &format!("test-org-{}", Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create organization")
    .id;

    // Create inviting user
    let inviter_email = format!("inviter-{}@example.com", Uuid::new_v4());
    let inviter_id = users::create_user(
        pool,
        &inviter_email,
        "password_hash",
        Some("Inviter"),
        false,
    )
    .await
    .expect("Failed to create inviter")
    .id;

    // Add inviter to organization as admin
    organization_members::add_member(
        pool,
        org_id,
        inviter_id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await
    .expect("Failed to add inviter to organization");

    (org_id, inviter_id)
}

#[tokio::test]
async fn test_create_invitation_generates_token_and_stores_hash() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    // Create invitation
    let (invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Verify token is generated
    assert_eq!(token.len(), 64, "Token should be 64 hex characters");
    assert!(
        token.chars().all(|c| c.is_ascii_hexdigit()),
        "Token should be hex"
    );

    // Verify invitation details
    assert_eq!(invitation.email, email);
    assert_eq!(invitation.organization_id, org_id);
    assert_eq!(invitation.org_role, "member");
    assert_eq!(invitation.workspace_role, "member");
    assert_eq!(invitation.invited_by, inviter_id);
    assert!(invitation.accepted_at.is_none());

    // Verify expiry is in the future (should be 7 days from now)
    let now = Utc::now();
    let expected_expiry = now + Duration::days(7);
    let diff = (invitation.expires_at - expected_expiry)
        .num_minutes()
        .abs();
    assert!(diff < 5, "Expiry should be approximately 7 days from now");
}

#[tokio::test]
async fn test_create_invitation_with_workspace_ids() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());
    let workspace_ids = vec![Uuid::new_v4(), Uuid::new_v4()];

    let (invitation, _token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        workspace_ids.clone(),
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    assert_eq!(invitation.workspace_ids, workspace_ids);
}

#[tokio::test]
async fn test_create_invitation_enforces_unique_email_per_org() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    // Create first invitation
    invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create first invitation");

    // Try to create duplicate invitation
    let result =
        invitations::create_invitation(&pool, &email, org_id, vec![], "admin", "admin", inviter_id)
            .await;

    assert!(
        result.is_err(),
        "Should not allow duplicate invitations for same email in same org"
    );
}

#[tokio::test]
async fn test_create_invitation_allows_same_email_different_orgs() {
    let pool = create_test_pool().await;
    let (org_id_1, inviter_id_1) = setup_test_data(&pool).await;
    let (org_id_2, inviter_id_2) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    // Create invitation in first org
    let result1 = invitations::create_invitation(
        &pool,
        &email,
        org_id_1,
        vec![],
        "member",
        "member",
        inviter_id_1,
    )
    .await;

    assert!(result1.is_ok(), "First invitation should succeed");

    // Create invitation in second org with same email
    let result2 = invitations::create_invitation(
        &pool,
        &email,
        org_id_2,
        vec![],
        "admin",
        "admin",
        inviter_id_2,
    )
    .await;

    assert!(
        result2.is_ok(),
        "Should allow same email in different organizations"
    );
}

#[tokio::test]
async fn test_get_invitation_by_token_returns_invitation() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (created_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "admin",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Get invitation by token
    let retrieved = invitations::get_invitation_by_token(&pool, &token)
        .await
        .expect("Failed to get invitation")
        .expect("Invitation should exist");

    assert_eq!(retrieved.id, created_invitation.id);
    assert_eq!(retrieved.email, email);
    assert_eq!(retrieved.organization_id, org_id);
    assert_eq!(retrieved.org_role, "admin");
    assert_eq!(retrieved.workspace_role, "member");
}

#[tokio::test]
async fn test_get_invitation_by_token_returns_none_for_invalid_token() {
    let pool = create_test_pool().await;

    let result = invitations::get_invitation_by_token(&pool, "invalid-token-abc123")
        .await
        .expect("Query should succeed");

    assert!(result.is_none(), "Should return None for invalid token");
}

#[tokio::test]
async fn test_get_invitation_by_token_returns_none_for_expired() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Manually expire the invitation
    let token_hash = zone_server::utils::crypto::hash_token(&token);
    let _ = sqlx::query(
        "UPDATE invitations SET expires_at = NOW() - INTERVAL '1 hour' WHERE token_hash = $1",
    )
    .bind(&token_hash)
    .execute(&pool)
    .await
    .expect("Failed to expire invitation");

    // Try to get expired invitation
    let result = invitations::get_invitation_by_token(&pool, &token)
        .await
        .expect("Query should succeed");

    assert!(result.is_none(), "Should not return expired invitation");
}

#[tokio::test]
async fn test_get_invitation_by_token_returns_none_for_accepted() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Accept invitation by creating user and accepting
    let user_id = users::create_user(&pool, &email, "password_hash", Some("Invited User"), false)
        .await
        .expect("Failed to create user")
        .id;

    invitations::accept_invitation(&pool, &token, user_id)
        .await
        .expect("Failed to accept invitation");

    // Try to get accepted invitation
    let result = invitations::get_invitation_by_token(&pool, &token)
        .await
        .expect("Query should succeed");

    assert!(
        result.is_none(),
        "Should not return already accepted invitation"
    );
}

#[tokio::test]
async fn test_accept_invitation_adds_user_to_org_and_workspaces() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    // Create workspaces in the organization
    let workspace_id_1 = zone_server::db::workspaces::create_workspace(
        &pool,
        org_id,
        &format!("Workspace 1 {}", Uuid::new_v4()),
        &format!("ws-1-{}", Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create workspace")
    .id;

    let workspace_id_2 = zone_server::db::workspaces::create_workspace(
        &pool,
        org_id,
        &format!("Workspace 2 {}", Uuid::new_v4()),
        &format!("ws-2-{}", Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create workspace")
    .id;

    let workspace_ids = vec![workspace_id_1, workspace_id_2];

    // Create invitation
    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        workspace_ids.clone(),
        "admin",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Create user
    let user_id = users::create_user(&pool, &email, "password_hash", Some("Invited User"), false)
        .await
        .expect("Failed to create user")
        .id;

    // Accept invitation
    invitations::accept_invitation(&pool, &token, user_id)
        .await
        .expect("Failed to accept invitation");

    // Verify user is added to organization with correct role
    let org_member = organization_members::get_member(&pool, org_id, user_id)
        .await
        .expect("Failed to get org member")
        .expect("User should be in organization");

    assert_eq!(org_member.role, organization_members::OrgRole::Admin);

    // Verify user is added to both workspaces
    let ws_role_1 = zone_server::db::workspace_members::get_role(&pool, user_id, workspace_id_1)
        .await
        .expect("Failed to get workspace role");

    let ws_role_2 = zone_server::db::workspace_members::get_role(&pool, user_id, workspace_id_2)
        .await
        .expect("Failed to get workspace role");

    assert_eq!(
        ws_role_1,
        Some(zone_server::db::workspace_members::WorkspaceRole::Member)
    );
    assert_eq!(
        ws_role_2,
        Some(zone_server::db::workspace_members::WorkspaceRole::Member)
    );
}

#[tokio::test]
async fn test_accept_invitation_marks_accepted_timestamp() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Create user
    let user_id = users::create_user(&pool, &email, "password_hash", Some("Invited User"), false)
        .await
        .expect("Failed to create user")
        .id;

    let before_accept = Utc::now() - chrono::Duration::seconds(2); // Allow 2s tolerance for clock skew

    // Accept invitation
    invitations::accept_invitation(&pool, &token, user_id)
        .await
        .expect("Failed to accept invitation");

    let after_accept = Utc::now() + chrono::Duration::seconds(2); // Allow 2s tolerance for clock skew

    // Verify accepted_at is set
    let token_hash = zone_server::utils::crypto::hash_token(&token);
    let row: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT accepted_at FROM invitations WHERE token_hash = $1")
            .bind(&token_hash)
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch invitation");

    let accepted_at = row.0.expect("accepted_at should be set");
    assert!(
        accepted_at >= before_accept && accepted_at <= after_accept,
        "accepted_at should be between before and after timestamps (with 2s tolerance)"
    );
}

#[tokio::test]
async fn test_accept_invitation_fails_for_expired_token() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Manually expire the invitation
    let token_hash = zone_server::utils::crypto::hash_token(&token);
    let _ = sqlx::query(
        "UPDATE invitations SET expires_at = NOW() - INTERVAL '1 hour' WHERE token_hash = $1",
    )
    .bind(&token_hash)
    .execute(&pool)
    .await
    .expect("Failed to expire invitation");

    // Create user
    let user_id = users::create_user(&pool, &email, "password_hash", Some("Invited User"), false)
        .await
        .expect("Failed to create user")
        .id;

    // Try to accept expired invitation
    let result = invitations::accept_invitation(&pool, &token, user_id).await;

    assert!(result.is_err(), "Should not accept expired invitation");
}

#[tokio::test]
async fn test_accept_invitation_fails_for_already_accepted() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Create user
    let user_id = users::create_user(&pool, &email, "password_hash", Some("Invited User"), false)
        .await
        .expect("Failed to create user")
        .id;

    // Accept invitation first time
    invitations::accept_invitation(&pool, &token, user_id)
        .await
        .expect("Failed to accept invitation");

    // Try to accept again
    let result = invitations::accept_invitation(&pool, &token, user_id).await;

    assert!(
        result.is_err(),
        "Should not accept already accepted invitation"
    );
}

#[tokio::test]
async fn test_list_pending_invitations() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;

    // Create multiple invitations
    let email1 = format!("invited1-{}@example.com", Uuid::new_v4());
    let email2 = format!("invited2-{}@example.com", Uuid::new_v4());
    let email3 = format!("invited3-{}@example.com", Uuid::new_v4());

    invitations::create_invitation(
        &pool,
        &email1,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation 1");

    invitations::create_invitation(&pool, &email2, org_id, vec![], "admin", "admin", inviter_id)
        .await
        .expect("Failed to create invitation 2");

    let (_, token3) = invitations::create_invitation(
        &pool,
        &email3,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation 3");

    // Accept one invitation
    let user_id = users::create_user(&pool, &email3, "password_hash", Some("User 3"), false)
        .await
        .expect("Failed to create user")
        .id;

    invitations::accept_invitation(&pool, &token3, user_id)
        .await
        .expect("Failed to accept invitation");

    // List pending invitations
    let pending = invitations::list_pending_invitations(&pool, org_id)
        .await
        .expect("Failed to list invitations");

    // Should only return 2 pending invitations (3rd is accepted)
    assert_eq!(pending.len(), 2);

    let emails: Vec<String> = pending.iter().map(|i| i.email.clone()).collect();
    assert!(emails.contains(&email1));
    assert!(emails.contains(&email2));
    assert!(
        !emails.contains(&email3),
        "Accepted invitation should not be in pending list"
    );
}

#[tokio::test]
async fn test_revoke_invitation() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Revoke invitation
    invitations::revoke_invitation(&pool, invitation.id)
        .await
        .expect("Failed to revoke invitation");

    // Verify invitation is deleted
    let result = invitations::get_invitation_by_token(&pool, &token)
        .await
        .expect("Query should succeed");

    assert!(
        result.is_none(),
        "Revoked invitation should not be retrievable"
    );
}

#[tokio::test]
async fn test_revoke_invitation_fails_for_nonexistent() {
    let pool = create_test_pool().await;

    let result = invitations::revoke_invitation(&pool, Uuid::new_v4()).await;

    assert!(
        result.is_err(),
        "Should fail when revoking nonexistent invitation"
    );
}

#[tokio::test]
async fn test_get_pending_invitation_for_email() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (created, _token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "admin",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Get pending invitation by email
    let retrieved = invitations::get_pending_invitation_for_email(&pool, &email, org_id)
        .await
        .expect("Failed to get invitation")
        .expect("Invitation should exist");

    assert_eq!(retrieved.id, created.id);
    assert_eq!(retrieved.email, email);
    assert_eq!(retrieved.org_role, "admin");
}

#[tokio::test]
async fn test_get_pending_invitation_for_email_returns_none_for_accepted() {
    let pool = create_test_pool().await;
    let (org_id, inviter_id) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    let (_invitation, token) = invitations::create_invitation(
        &pool,
        &email,
        org_id,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Accept invitation
    let user_id = users::create_user(&pool, &email, "password_hash", Some("User"), false)
        .await
        .expect("Failed to create user")
        .id;

    invitations::accept_invitation(&pool, &token, user_id)
        .await
        .expect("Failed to accept invitation");

    // Try to get pending invitation
    let result = invitations::get_pending_invitation_for_email(&pool, &email, org_id)
        .await
        .expect("Query should succeed");

    assert!(
        result.is_none(),
        "Should not return accepted invitation as pending"
    );
}

#[tokio::test]
async fn test_get_pending_invitation_for_email_returns_none_for_different_org() {
    let pool = create_test_pool().await;
    let (org_id_1, inviter_id) = setup_test_data(&pool).await;
    let (org_id_2, _) = setup_test_data(&pool).await;
    let email = format!("invited-{}@example.com", Uuid::new_v4());

    invitations::create_invitation(
        &pool,
        &email,
        org_id_1,
        vec![],
        "member",
        "member",
        inviter_id,
    )
    .await
    .expect("Failed to create invitation");

    // Try to get invitation for different org
    let result = invitations::get_pending_invitation_for_email(&pool, &email, org_id_2)
        .await
        .expect("Query should succeed");

    assert!(
        result.is_none(),
        "Should not return invitation from different organization"
    );
}
