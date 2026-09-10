//! Only an owner deletes a workspace or seats another owner, so a workspace
//! with no active owner cannot be repaired from inside -- an admin cannot
//! promote anyone, and nobody can delete it. Migration 023 exists to undo that
//! state, so the role routes must stop producing it.
//!
//! Counting admins and owners together does produce it: with one owner and one
//! admin standing, the count says two are privileged and lets the owner go.

mod common;

use common::create_test_pool;
use sqlx::{PgPool, Row};
use uuid::Uuid;
use zone_server::db::workspace_members::{
    Removal, RoleChange, WorkspaceRole, change_role, remove_guarded,
};

async fn user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("x")
        .fetch_one(pool)
        .await
        .expect("a user")
}

/// A workspace holding one owner and one admin -- the shape a combined count
/// reads as "two privileged members, one may go".
async fn workspace_with_owner_and_admin(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let organization: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind("Floor")
            .bind(Uuid::new_v4().to_string())
            .fetch_one(pool)
            .await
            .expect("an organization");
    let workspace: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind("Floor")
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .expect("a workspace");

    let owner = user(pool).await;
    let admin = user(pool).await;
    for (member, role) in [(owner, "owner"), (admin, "admin")] {
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(workspace)
        .bind(member)
        .bind(role)
        .execute(pool)
        .await
        .expect("a member is seated");
    }
    (workspace, owner, admin)
}

async fn owners(pool: &PgPool, workspace: Uuid) -> i64 {
    sqlx::query(
        "SELECT COUNT(*) AS count FROM workspace_members \
         WHERE workspace_id = $1 AND role = 'owner' AND is_active = TRUE",
    )
    .bind(workspace)
    .fetch_one(pool)
    .await
    .expect("the owners are countable")
    .get::<i64, _>("count")
}

#[tokio::test]
async fn the_last_owner_cannot_be_demoted_to_member_while_an_admin_stands() {
    let pool = create_test_pool().await;
    let (workspace, owner, _admin) = workspace_with_owner_and_admin(&pool).await;

    let outcome = change_role(
        &pool,
        workspace,
        owner,
        WorkspaceRole::Member,
        WorkspaceRole::Owner,
    )
    .await
    .expect("the demotion is answered");

    assert!(
        matches!(outcome, RoleChange::LastOwner),
        "the workspace's only owner was demoted because an admin was counted \
         alongside them: {outcome:?}"
    );
    assert_eq!(owners(&pool, workspace).await, 1);
}

/// Stepping down to admin keeps the member privileged, so a guard keyed on
/// "still an admin" waves it through -- and the workspace is just as ownerless.
#[tokio::test]
async fn the_last_owner_cannot_step_down_to_admin() {
    let pool = create_test_pool().await;
    let (workspace, owner, _admin) = workspace_with_owner_and_admin(&pool).await;

    let outcome = change_role(
        &pool,
        workspace,
        owner,
        WorkspaceRole::Admin,
        WorkspaceRole::Owner,
    )
    .await
    .expect("the demotion is answered");

    assert!(
        matches!(outcome, RoleChange::LastOwner),
        "the workspace's only owner stepped down to admin and left it \
         ownerless: {outcome:?}"
    );
    assert_eq!(owners(&pool, workspace).await, 1);
}

#[tokio::test]
async fn the_last_owner_cannot_be_removed_while_an_admin_stands() {
    let pool = create_test_pool().await;
    let (workspace, owner, _admin) = workspace_with_owner_and_admin(&pool).await;

    let outcome = remove_guarded(&pool, workspace, owner, WorkspaceRole::Owner)
        .await
        .expect("the removal is answered");

    assert!(
        matches!(outcome, Removal::LastOwner),
        "the workspace's only owner was removed because an admin was counted \
         alongside them: {outcome:?}"
    );
    assert_eq!(owners(&pool, workspace).await, 1);
}

/// The floor must not become a wall: a second owner makes the first expendable.
#[tokio::test]
async fn an_owner_still_steps_down_once_another_owner_stands() {
    let pool = create_test_pool().await;
    let (workspace, owner, admin) = workspace_with_owner_and_admin(&pool).await;

    change_role(
        &pool,
        workspace,
        admin,
        WorkspaceRole::Owner,
        WorkspaceRole::Owner,
    )
    .await
    .expect("the admin is promoted");

    let outcome = change_role(
        &pool,
        workspace,
        owner,
        WorkspaceRole::Member,
        WorkspaceRole::Owner,
    )
    .await
    .expect("the demotion is answered");

    assert!(
        matches!(outcome, RoleChange::Applied(_)),
        "an owner must still be able to step down once another stands: {outcome:?}"
    );
    assert_eq!(owners(&pool, workspace).await, 1);
}

/// The rank being granted is guarded in the route as well. It is guarded here
/// because this is the entry point that calls itself guarded: a caller who
/// reaches it should not be able to acquire the target check without the grant
/// check, whichever route brought them.
#[tokio::test]
async fn an_admin_cannot_grant_a_rank_only_an_owner_grants() {
    let pool = create_test_pool().await;
    let (workspace, _owner, admin) = workspace_with_owner_and_admin(&pool).await;
    let member = user(&pool).await;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
    )
    .bind(workspace)
    .bind(member)
    .execute(&pool)
    .await
    .expect("a member is seated");

    for granted in [WorkspaceRole::Admin, WorkspaceRole::Owner] {
        let outcome = change_role(&pool, workspace, member, granted, WorkspaceRole::Admin)
            .await
            .expect("the grant is answered");
        assert!(
            matches!(outcome, RoleChange::Forbidden),
            "an admin granted {granted:?}, which only an owner grants: {outcome:?}"
        );
    }

    // The floor is not a wall: an owner still grants both, and an admin still
    // seats the ranks below them.
    let outcome = change_role(
        &pool,
        workspace,
        member,
        WorkspaceRole::Admin,
        WorkspaceRole::Owner,
    )
    .await
    .expect("the grant is answered");
    assert!(
        matches!(outcome, RoleChange::Applied(_)),
        "an owner must still grant admin: {outcome:?}"
    );

    let outcome = change_role(
        &pool,
        workspace,
        admin,
        WorkspaceRole::Member,
        WorkspaceRole::Owner,
    )
    .await
    .expect("the demotion is answered");
    assert!(
        matches!(outcome, RoleChange::Applied(_)),
        "an owner must still demote an admin: {outcome:?}"
    );
}
