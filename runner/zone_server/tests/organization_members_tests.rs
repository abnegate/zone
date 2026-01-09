//! Organization membership integration tests
//!
//! Tests for organization member CRUD operations, role hierarchy,
//! cascade deletes, and permission checks.

mod common;

use sqlx::PgPool;
use uuid::Uuid;

use zone_server::db::{organization_members, organizations, users};

async fn create_test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string());

    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
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

    // Create first user
    let user1_email = format!("test-user1-{}@example.com", Uuid::new_v4());
    let user1_id = users::create_user(
        pool,
        &user1_email,
        "password_hash",
        Some("Test User 1"),
        false,
    )
    .await
    .expect("Failed to create user 1")
    .id;

    // Create second user
    let user2_email = format!("test-user2-{}@example.com", Uuid::new_v4());
    let user2_id = users::create_user(
        pool,
        &user2_email,
        "password_hash",
        Some("Test User 2"),
        false,
    )
    .await
    .expect("Failed to create user 2")
    .id;

    (org_id, user1_id, user2_id)
}

// =============================================================================
// CRUD Operations Tests
// =============================================================================

#[tokio::test]
async fn test_add_member_creates_membership() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    let result = organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await;

    assert!(result.is_ok(), "Should successfully add member");
    let member = result.unwrap();
    assert_eq!(member.organization_id, org_id);
    assert_eq!(member.user_id, user_id);
    assert_eq!(member.role, organization_members::OrgRole::Member);
}

#[tokio::test]
async fn test_add_member_with_inviter() {
    let pool = create_test_pool().await;
    let (org_id, user1_id, user2_id) = setup_test_data(&pool).await;

    // Add user1 as owner
    organization_members::add_member(
        &pool,
        org_id,
        user1_id,
        organization_members::OrgRole::Owner,
        None,
    )
    .await
    .expect("Failed to add owner");

    // Add user2, invited by user1
    let result = organization_members::add_member(
        &pool,
        org_id,
        user2_id,
        organization_members::OrgRole::Member,
        Some(user1_id),
    )
    .await;

    assert!(result.is_ok());
    let member = result.unwrap();
    assert_eq!(member.invited_by, Some(user1_id));
}

#[tokio::test]
async fn test_add_member_enforces_unique_constraint() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    // Add member first time
    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("First add should succeed");

    // Add same member again (should fail - CRITICAL-7: add_member now fails if member exists)
    let result = organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "Duplicate add should fail due to unique constraint"
    );
}

#[tokio::test]
async fn test_get_member_returns_member() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await
    .expect("Failed to add member");

    let result = organization_members::get_member(&pool, org_id, user_id).await;
    assert!(result.is_ok());

    let member = result.unwrap();
    assert!(member.is_some(), "Should find member");
    let member = member.unwrap();
    assert_eq!(member.user_id, user_id);
    assert_eq!(member.organization_id, org_id);
    assert_eq!(member.role, organization_members::OrgRole::Admin);
}

#[tokio::test]
async fn test_get_member_returns_none_for_non_member() {
    let pool = create_test_pool().await;
    let (org_id, _, user_id) = setup_test_data(&pool).await;

    let result = organization_members::get_member(&pool, org_id, user_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none(), "Should not find non-member");
}

#[tokio::test]
async fn test_remove_member_deactivates_membership() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    let removed = organization_members::remove_member(&pool, org_id, user_id)
        .await
        .expect("Failed to remove member");

    assert!(removed, "Should successfully remove member");

    // Verify member is no longer active
    let is_member = organization_members::is_member(&pool, org_id, user_id)
        .await
        .expect("Failed to check membership");

    assert!(!is_member, "User should not be active member after removal");
}

#[tokio::test]
async fn test_list_members_returns_all_active_members() {
    let pool = create_test_pool().await;
    let (org_id, user1_id, user2_id) = setup_test_data(&pool).await;

    // Add two members
    organization_members::add_member(
        &pool,
        org_id,
        user1_id,
        organization_members::OrgRole::Owner,
        None,
    )
    .await
    .expect("Failed to add user1");

    organization_members::add_member(
        &pool,
        org_id,
        user2_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add user2");

    let members = organization_members::list_members(&pool, org_id)
        .await
        .expect("Failed to list members");

    assert_eq!(members.len(), 2, "Should have 2 members");
}

#[tokio::test]
async fn test_list_members_excludes_inactive_members() {
    let pool = create_test_pool().await;
    let (org_id, user1_id, user2_id) = setup_test_data(&pool).await;

    // Add two members
    organization_members::add_member(
        &pool,
        org_id,
        user1_id,
        organization_members::OrgRole::Owner,
        None,
    )
    .await
    .expect("Failed to add user1");

    organization_members::add_member(
        &pool,
        org_id,
        user2_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add user2");

    // Remove one member
    organization_members::remove_member(&pool, org_id, user2_id)
        .await
        .expect("Failed to remove user2");

    let members = organization_members::list_members(&pool, org_id)
        .await
        .expect("Failed to list members");

    assert_eq!(members.len(), 1, "Should have 1 active member");
    assert_eq!(members[0].user_id, user1_id);
}

#[tokio::test]
async fn test_list_user_organizations_returns_all_orgs() {
    let pool = create_test_pool().await;

    // Create user
    let user_email = format!("test-multi-org-{}@example.com", Uuid::new_v4());
    let user_id = users::create_user(
        &pool,
        &user_email,
        "password_hash",
        Some("Test User"),
        false,
    )
    .await
    .expect("Failed to create user")
    .id;

    // Create two organizations
    let org1_id = organizations::create_organization(
        &pool,
        "Org 1",
        &format!("org1-{}", Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create org1")
    .id;

    let org2_id = organizations::create_organization(
        &pool,
        "Org 2",
        &format!("org2-{}", Uuid::new_v4()),
        None,
    )
    .await
    .expect("Failed to create org2")
    .id;

    // Add user to both orgs
    organization_members::add_member(
        &pool,
        org1_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add to org1");

    organization_members::add_member(
        &pool,
        org2_id,
        user_id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await
    .expect("Failed to add to org2");

    let orgs = organization_members::list_user_organizations(&pool, user_id)
        .await
        .expect("Failed to list user organizations");

    assert_eq!(orgs.len(), 2, "User should be in 2 organizations");
}

#[tokio::test]
async fn test_update_member_role_changes_role() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    // Add member as Member
    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    // Update to Admin
    let result = organization_members::update_member_role(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Admin,
    )
    .await;

    assert!(result.is_ok());
    let updated = result.unwrap();
    assert_eq!(updated.role, organization_members::OrgRole::Admin);
}

// =============================================================================
// Role Hierarchy Tests
// =============================================================================

#[tokio::test]
async fn test_is_member_returns_true_for_active_member() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    let is_member = organization_members::is_member(&pool, org_id, user_id)
        .await
        .expect("Failed to check membership");

    assert!(is_member, "User should be a member");
}

#[tokio::test]
async fn test_is_member_returns_false_for_non_member() {
    let pool = create_test_pool().await;
    let (org_id, _, user_id) = setup_test_data(&pool).await;

    let is_member = organization_members::is_member(&pool, org_id, user_id)
        .await
        .expect("Failed to check membership");

    assert!(!is_member, "Non-member should return false");
}

#[tokio::test]
async fn test_is_admin_returns_true_for_admin() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await
    .expect("Failed to add admin");

    let is_admin = organization_members::is_admin(&pool, org_id, user_id)
        .await
        .expect("Failed to check admin status");

    assert!(is_admin, "Admin should return true");
}

#[tokio::test]
async fn test_is_admin_returns_true_for_owner() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Owner,
        None,
    )
    .await
    .expect("Failed to add owner");

    let is_admin = organization_members::is_admin(&pool, org_id, user_id)
        .await
        .expect("Failed to check admin status");

    assert!(is_admin, "Owner should satisfy admin requirement");
}

#[tokio::test]
async fn test_is_admin_returns_false_for_member() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    let is_admin = organization_members::is_admin(&pool, org_id, user_id)
        .await
        .expect("Failed to check admin status");

    assert!(!is_admin, "Regular member should not be admin");
}

#[tokio::test]
async fn test_is_owner_returns_true_for_owner() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Owner,
        None,
    )
    .await
    .expect("Failed to add owner");

    let is_owner = organization_members::is_owner(&pool, org_id, user_id)
        .await
        .expect("Failed to check owner status");

    assert!(is_owner, "Owner should return true");
}

#[tokio::test]
async fn test_is_owner_returns_false_for_admin() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await
    .expect("Failed to add admin");

    let is_owner = organization_members::is_owner(&pool, org_id, user_id)
        .await
        .expect("Failed to check owner status");

    assert!(!is_owner, "Admin should not be owner");
}

// =============================================================================
// Cascade Delete Tests
// =============================================================================

#[tokio::test]
async fn test_cascade_delete_when_org_deleted() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    // Add member
    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    // Verify membership exists
    let member = organization_members::get_member(&pool, org_id, user_id)
        .await
        .expect("Failed to get member");
    assert!(member.is_some(), "Member should exist before org deletion");

    // Delete organization
    organizations::delete_organization(&pool, org_id)
        .await
        .expect("Failed to delete organization");

    // Verify membership is gone (due to CASCADE)
    let member = organization_members::get_member(&pool, org_id, user_id)
        .await
        .expect("Failed to get member");
    assert!(
        member.is_none(),
        "Member should be deleted when org is deleted"
    );
}

#[tokio::test]
async fn test_cascade_delete_when_user_deleted() {
    let pool = create_test_pool().await;
    let (org_id, user_id, _) = setup_test_data(&pool).await;

    // Add member
    organization_members::add_member(
        &pool,
        org_id,
        user_id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member");

    // Delete user
    sqlx::query!("DELETE FROM users WHERE id = $1", user_id)
        .execute(&pool)
        .await
        .expect("Failed to delete user");

    // Verify membership is gone
    let member = organization_members::get_member(&pool, org_id, user_id)
        .await
        .expect("Failed to get member");
    assert!(
        member.is_none(),
        "Member should be deleted when user is deleted"
    );
}

// =============================================================================
// Role Comparison Tests
// =============================================================================

#[tokio::test]
async fn test_role_hierarchy_ordering() {
    use organization_members::OrgRole;

    assert!(OrgRole::Owner > OrgRole::Admin);
    assert!(OrgRole::Admin > OrgRole::Member);
    assert!(OrgRole::Owner > OrgRole::Member);
}

#[tokio::test]
async fn test_role_from_str() {
    use organization_members::OrgRole;

    assert_eq!("owner".parse::<OrgRole>(), Ok(OrgRole::Owner));
    assert_eq!("admin".parse::<OrgRole>(), Ok(OrgRole::Admin));
    assert_eq!("member".parse::<OrgRole>(), Ok(OrgRole::Member));
    assert!("invalid".parse::<OrgRole>().is_err());
}

#[tokio::test]
async fn test_role_as_str() {
    use organization_members::OrgRole;

    assert_eq!(OrgRole::Owner.as_str(), "owner");
    assert_eq!(OrgRole::Admin.as_str(), "admin");
    assert_eq!(OrgRole::Member.as_str(), "member");
}
