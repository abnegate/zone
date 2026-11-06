//! Server-wide model permissions read straight from the database, as the chat
//! agent's model tools do when they act for a user.
use crate::common;

use zone_server::auth::Permission;
use zone_server::auth::permission::holds;
use zone_server::db::users;

async fn user(pool: &sqlx::PgPool, is_admin: bool, role: Option<&str>) -> uuid::Uuid {
    let user = users::create_user(
        pool,
        &common::test_email(),
        "password-hash",
        Some("Model permission user"),
        is_admin,
    )
    .await
    .unwrap();
    if let Some(role) = role {
        users::assign_user_role(pool, user.id, role).await.unwrap();
    }
    user.id
}

#[tokio::test]
async fn a_standard_user_holds_no_model_write_permission() {
    let pool = common::create_test_pool().await;
    let user_id = user(&pool, false, Some("user")).await;
    assert!(
        !holds(&pool, user_id, Permission::ModelsCreate)
            .await
            .unwrap()
    );
    assert!(
        !holds(&pool, user_id, Permission::ModelsDelete)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_viewer_holds_no_model_write_permission() {
    let pool = common::create_test_pool().await;
    let user_id = user(&pool, false, Some("viewer")).await;
    assert!(
        !holds(&pool, user_id, Permission::ModelsCreate)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn the_global_admin_role_holds_both_model_permissions() {
    let pool = common::create_test_pool().await;
    let user_id = user(&pool, false, Some("admin")).await;
    assert!(
        holds(&pool, user_id, Permission::ModelsCreate)
            .await
            .unwrap()
    );
    assert!(
        holds(&pool, user_id, Permission::ModelsDelete)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn an_instance_admin_holds_both_model_permissions_without_a_role() {
    let pool = common::create_test_pool().await;
    let user_id = user(&pool, true, None).await;
    assert!(
        holds(&pool, user_id, Permission::ModelsCreate)
            .await
            .unwrap()
    );
    assert!(
        holds(&pool, user_id, Permission::ModelsDelete)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn an_unknown_user_holds_nothing() {
    let pool = common::create_test_pool().await;
    assert!(
        !holds(&pool, uuid::Uuid::new_v4(), Permission::ModelsCreate)
            .await
            .unwrap()
    );
}
