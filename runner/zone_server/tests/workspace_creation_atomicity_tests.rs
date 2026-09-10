//! A workspace is only reachable through a membership guard, so one created
//! without its owner row cannot be repaired through the API by anyone -- not
//! even the person who just created it. Creation has to be one unit.

mod common;

use common::create_test_pool;
use sqlx::Row;
use uuid::Uuid;
use zone_server::db::workspaces;

async fn organization(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
        .bind("Atomicity")
        .bind(Uuid::new_v4().to_string())
        .fetch_one(pool)
        .await
        .expect("an organization to hold the workspace")
}

async fn user(pool: &sqlx::PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("x")
        .fetch_one(pool)
        .await
        .expect("a user to own the workspace")
}

#[tokio::test]
async fn a_created_workspace_has_its_creator_seated_as_owner() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let owner = user(&pool).await;
    let slug = Uuid::new_v4().to_string();

    let workspace =
        workspaces::create_workspace_with_owner(&pool, organization, "Seated", &slug, None, owner)
            .await
            .expect("the workspace is created");

    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND is_active",
    )
    .bind(workspace.id)
    .bind(owner)
    .fetch_optional(&pool)
    .await
    .expect("the membership is readable");
    assert_eq!(
        role.as_deref(),
        Some("owner"),
        "the creator must be seated as owner, or the workspace is unreachable"
    );
}

/// The owner row is what can fail in production; a user id with no row behind
/// it makes its foreign key fail on demand. Without one transaction the
/// workspace survives that failure and is stranded.
#[tokio::test]
async fn a_workspace_whose_owner_cannot_be_seated_is_not_left_behind() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let slug = Uuid::new_v4().to_string();

    let result = workspaces::create_workspace_with_owner(
        &pool,
        organization,
        "Stranded",
        &slug,
        None,
        Uuid::new_v4(),
    )
    .await;
    assert!(
        result.is_err(),
        "seating an owner who does not exist must fail"
    );

    let surviving = sqlx::query("SELECT id FROM workspaces WHERE slug = $1")
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .expect("the workspaces table is readable");
    assert!(
        surviving.is_none(),
        "the workspace outlived the owner it could not seat, and nothing can reach it now: {:?}",
        surviving.map(|row| row.get::<Uuid, _>("id"))
    );
}
