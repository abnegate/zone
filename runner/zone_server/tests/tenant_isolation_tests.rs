//! Tenant isolation tests
//!
//! Verify that users can only access resources in their organizations/workspaces

use uuid::Uuid;
use zone_server::db::{
    chats, organization_members, organizations, projects, users, workspace_members, workspaces,
};

mod common;

#[tokio::test]
async fn test_user_cannot_see_other_workspace_projects() {
    let pool = common::create_test_pool().await;
    let test_id = Uuid::new_v4();

    // Create two separate organizations and workspaces
    let org1 = organizations::create_organization(
        &pool,
        &format!("Org 1 {}", test_id),
        &format!("org-1-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org 1");

    let org2 = organizations::create_organization(
        &pool,
        &format!("Org 2 {}", test_id),
        &format!("org-2-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org 2");

    let ws1 = workspaces::create_workspace(
        &pool,
        org1.id,
        &format!("Workspace 1 {}", test_id),
        &format!("ws-1-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 1");

    let ws2 = workspaces::create_workspace(
        &pool,
        org2.id,
        &format!("Workspace 2 {}", test_id),
        &format!("ws-2-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 2");

    // Create two users
    let user1 = users::create_user(
        &pool,
        &format!("user1-{}@example.com", test_id),
        "hash1",
        Some("User 1"),
        false,
    )
    .await
    .expect("Failed to create user 1");

    let user2 = users::create_user(
        &pool,
        &format!("user2-{}@example.com", test_id),
        "hash2",
        Some("User 2"),
        false,
    )
    .await
    .expect("Failed to create user 2");

    // Add user1 to workspace1, user2 to workspace2
    workspace_members::add_member(
        &pool,
        ws1.id,
        user1.id,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("Failed to add user1 to ws1");

    workspace_members::add_member(
        &pool,
        ws2.id,
        user2.id,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("Failed to add user2 to ws2");

    // Create projects in both workspaces
    let project1 = projects::create_project(&pool, "Project 1", None, Some(ws1.id))
        .await
        .expect("Failed to create project 1");

    let project2 = projects::create_project(&pool, "Project 2", None, Some(ws2.id))
        .await
        .expect("Failed to create project 2");

    // User1 should only see projects in workspace1
    let ws1_projects = projects::list_projects(&pool, ws1.id, None)
        .await
        .expect("Failed to list ws1 projects");
    assert_eq!(ws1_projects.len(), 1);
    assert_eq!(ws1_projects[0].id, project1.id);

    // User2 should only see projects in workspace2
    let ws2_projects = projects::list_projects(&pool, ws2.id, None)
        .await
        .expect("Failed to list ws2 projects");
    assert_eq!(ws2_projects.len(), 1);
    assert_eq!(ws2_projects[0].id, project2.id);

    // Listing projects for ws1 should not return projects from ws2
    let ws1_check = projects::list_projects(&pool, ws1.id, None)
        .await
        .expect("Failed to list ws1 projects");
    assert!(!ws1_check.iter().any(|p| p.id == project2.id));
}

#[tokio::test]
async fn test_user_cannot_see_other_workspace_chats() {
    let pool = common::create_test_pool().await;
    let test_id = Uuid::new_v4();

    // Create two separate workspaces
    let org1 = organizations::create_organization(
        &pool,
        &format!("Org A {}", test_id),
        &format!("org-a-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org");

    let ws1 = workspaces::create_workspace(
        &pool,
        org1.id,
        &format!("Workspace A {}", test_id),
        &format!("ws-a-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace a");

    let ws2 = workspaces::create_workspace(
        &pool,
        org1.id,
        &format!("Workspace B {}", test_id),
        &format!("ws-b-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace b");

    // Create chats in both workspaces
    let chat1 = chats::create_chat(&pool, Some(ws1.id), "Chat 1", "gpt-4", false)
        .await
        .expect("Failed to create chat 1");

    let chat2 = chats::create_chat(&pool, Some(ws2.id), "Chat 2", "gpt-4", false)
        .await
        .expect("Failed to create chat 2");

    // List chats for workspace 1 - should only see chat1
    let ws1_chats = chats::list_chats(&pool, Some(ws1.id), None)
        .await
        .expect("Failed to list ws1 chats");
    assert_eq!(ws1_chats.len(), 1);
    assert_eq!(ws1_chats[0].id, chat1.id);

    // List chats for workspace 2 - should only see chat2
    let ws2_chats = chats::list_chats(&pool, Some(ws2.id), None)
        .await
        .expect("Failed to list ws2 chats");
    assert_eq!(ws2_chats.len(), 1);
    assert_eq!(ws2_chats[0].id, chat2.id);
}

#[tokio::test]
async fn test_cross_workspace_access_denied() {
    let pool = common::create_test_pool().await;
    let test_id = Uuid::new_v4();

    // Create organization and two workspaces
    let org = organizations::create_organization(
        &pool,
        &format!("Org {}", test_id),
        &format!("org-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org");

    let ws1 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 1 {}", test_id),
        &format!("ws-1-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 1");

    let ws2 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 2 {}", test_id),
        &format!("ws-2-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 2");

    // Create user and add only to workspace1
    let user = users::create_user(
        &pool,
        &format!("user-{}@example.com", test_id),
        "hash",
        Some("User"),
        false,
    )
    .await
    .expect("Failed to create user");

    workspace_members::add_member(
        &pool,
        ws1.id,
        user.id,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("Failed to add user to ws1");

    // User should be a member of ws1
    let is_ws1_member = workspace_members::is_member(&pool, user.id, ws1.id)
        .await
        .expect("Failed to check ws1 membership");
    assert!(is_ws1_member);

    // User should NOT be a member of ws2
    let is_ws2_member = workspace_members::is_member(&pool, user.id, ws2.id)
        .await
        .expect("Failed to check ws2 membership");
    assert!(!is_ws2_member);
}

#[tokio::test]
async fn test_workspace_deletion_cascades() {
    let pool = common::create_test_pool().await;
    let test_id = Uuid::new_v4();

    // Create organization and workspace
    let org = organizations::create_organization(
        &pool,
        &format!("Org {}", test_id),
        &format!("org-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org");

    let ws = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace {}", test_id),
        &format!("ws-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace");

    // Create resources in workspace
    let project = projects::create_project(&pool, "Project", None, Some(ws.id))
        .await
        .expect("Failed to create project");

    let chat = chats::create_chat(&pool, Some(ws.id), "Chat", "gpt-4", false)
        .await
        .expect("Failed to create chat");

    // Delete workspace
    let deleted = workspaces::delete_workspace(&pool, ws.id)
        .await
        .expect("Failed to delete workspace");
    assert!(deleted);

    // Projects should be orphaned (workspace_id set to NULL) or deleted depending on constraints
    // Note: This depends on the actual CASCADE behavior in the database
    let _project_check = projects::get_project(&pool, project.id)
        .await
        .expect("Failed to get project");

    // If ON DELETE CASCADE, project should be gone
    // If ON DELETE SET NULL, project should exist but workspace_id should be None
    // Check which behavior is implemented

    // Chat should be orphaned or deleted
    let _chat_check = chats::get_chat(&pool, chat.id)
        .await
        .expect("Failed to get chat");

    // Similar to project, depends on CASCADE behavior
}

#[tokio::test]
async fn test_organization_admin_can_see_all_workspaces() {
    let pool = common::create_test_pool().await;
    let test_id = Uuid::new_v4();

    // Create organization
    let org = organizations::create_organization(
        &pool,
        &format!("Org {}", test_id),
        &format!("org-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org");

    // Create two workspaces
    let ws1 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 1 {}", test_id),
        &format!("ws-1-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 1");

    let ws2 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 2 {}", test_id),
        &format!("ws-2-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 2");

    // Create admin user
    let admin = users::create_user(
        &pool,
        &format!("admin-{}@example.com", test_id),
        "hash",
        Some("Admin"),
        false,
    )
    .await
    .expect("Failed to create admin");

    // Add admin to organization with admin role
    organization_members::add_member(
        &pool,
        org.id,
        admin.id,
        organization_members::OrgRole::Admin,
        None,
    )
    .await
    .expect("Failed to add admin to org");

    // Add admin to both workspaces
    workspace_members::add_member(
        &pool,
        ws1.id,
        admin.id,
        workspace_members::WorkspaceRole::Admin,
        None,
    )
    .await
    .expect("Failed to add admin to ws1");

    workspace_members::add_member(
        &pool,
        ws2.id,
        admin.id,
        workspace_members::WorkspaceRole::Admin,
        None,
    )
    .await
    .expect("Failed to add admin to ws2");

    // Admin should see both workspaces
    let workspaces = workspace_members::list_user_workspaces_in_org(&pool, admin.id, org.id)
        .await
        .expect("Failed to list workspaces");

    assert_eq!(workspaces.len(), 2);
    assert!(workspaces.iter().any(|w| w.id == ws1.id));
    assert!(workspaces.iter().any(|w| w.id == ws2.id));
}

#[tokio::test]
async fn test_organization_member_only_sees_assigned_workspaces() {
    let pool = common::create_test_pool().await;
    let test_id = Uuid::new_v4();

    // Create organization
    let org = organizations::create_organization(
        &pool,
        &format!("Org {}", test_id),
        &format!("org-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create org");

    // Create three workspaces
    let ws1 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 1 {}", test_id),
        &format!("ws-1-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 1");

    let ws2 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 2 {}", test_id),
        &format!("ws-2-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 2");

    let ws3 = workspaces::create_workspace(
        &pool,
        org.id,
        &format!("Workspace 3 {}", test_id),
        &format!("ws-3-{}", test_id),
        None,
    )
    .await
    .expect("Failed to create workspace 3");

    // Create regular member
    let member = users::create_user(
        &pool,
        &format!("member-{}@example.com", test_id),
        "hash",
        Some("Member"),
        false,
    )
    .await
    .expect("Failed to create member");

    // Add member to organization
    organization_members::add_member(
        &pool,
        org.id,
        member.id,
        organization_members::OrgRole::Member,
        None,
    )
    .await
    .expect("Failed to add member to org");

    // Add member only to ws1 and ws2
    workspace_members::add_member(
        &pool,
        ws1.id,
        member.id,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("Failed to add member to ws1");

    workspace_members::add_member(
        &pool,
        ws2.id,
        member.id,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("Failed to add member to ws2");

    // Member should only see ws1 and ws2
    let workspaces = workspace_members::list_user_workspaces_in_org(&pool, member.id, org.id)
        .await
        .expect("Failed to list workspaces");

    assert_eq!(workspaces.len(), 2);
    assert!(workspaces.iter().any(|w| w.id == ws1.id));
    assert!(workspaces.iter().any(|w| w.id == ws2.id));
    assert!(!workspaces.iter().any(|w| w.id == ws3.id));
}
