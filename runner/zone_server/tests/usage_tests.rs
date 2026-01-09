//! Tests for usage tracking database module

mod common;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::plans::get_plan_by_slug;
use zone_server::db::subscriptions::create_subscription;
use zone_server::db::usage::{check_limit, get_usage_for_period, record_event};

async fn create_test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string());

    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

async fn create_test_org(pool: &PgPool) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO organizations (name, slug)
        VALUES ($1, $2)
        RETURNING id
        "#,
    )
    .bind("Test Org")
    .bind(format!("test-org-{}", Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn create_test_workspace(pool: &PgPool, org_id: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO workspaces (name, slug, organization_id)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
    )
    .bind("Test Workspace")
    .bind(format!("test-ws-{}", Uuid::new_v4()))
    .bind(org_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn create_test_user(pool: &PgPool) -> Uuid {
    let email = format!("test-{}@example.com", Uuid::new_v4());
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO users (email, password_hash)
        VALUES ($1, $2)
        RETURNING id
        "#,
    )
    .bind(email)
    .bind("hashed_password")
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn test_record_event() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    record_event(&pool, org_id, "chat_message", 1, None, None, None)
        .await
        .unwrap();

    // Verify the event was recorded
    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM usage_events WHERE organization_id = $1",
    )
    .bind(org_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_record_event_with_workspace() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let workspace_id = create_test_workspace(&pool, org_id).await;

    record_event(
        &pool,
        org_id,
        "chat_message",
        1,
        Some(workspace_id),
        None,
        None,
    )
    .await
    .unwrap();

    let event: (Option<Uuid>,) =
        sqlx::query_as("SELECT workspace_id FROM usage_events WHERE organization_id = $1")
            .bind(org_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(event.0, Some(workspace_id));
}

#[tokio::test]
async fn test_record_event_with_user() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let user_id = create_test_user(&pool).await;

    record_event(&pool, org_id, "chat_message", 1, None, Some(user_id), None)
        .await
        .unwrap();

    let event: (Option<Uuid>,) =
        sqlx::query_as("SELECT user_id FROM usage_events WHERE organization_id = $1")
            .bind(org_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(event.0, Some(user_id));
}

#[tokio::test]
async fn test_record_event_with_metadata() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    let metadata = serde_json::json!({
        "model": "gpt-4",
        "tokens": 150
    });

    record_event(
        &pool,
        org_id,
        "chat_message",
        1,
        None,
        None,
        Some(metadata.clone()),
    )
    .await
    .unwrap();

    let event: (Option<serde_json::Value>,) =
        sqlx::query_as("SELECT metadata FROM usage_events WHERE organization_id = $1")
            .bind(org_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(event.0, Some(metadata));
}

#[tokio::test]
async fn test_record_multiple_events() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    for _i in 0..5 {
        record_event(&pool, org_id, "chat_message", 1, None, None, None)
            .await
            .unwrap();
    }

    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM usage_events WHERE organization_id = $1 AND event_type = 'chat_message'",
    )
    .bind(org_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_get_usage_for_period() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    // Record 3 events
    for _ in 0..3 {
        record_event(&pool, org_id, "chat_message", 1, None, None, None)
            .await
            .unwrap();
    }

    let now = Utc::now();
    let start = now - Duration::hours(1);
    let end = now + Duration::hours(1);

    let usage = get_usage_for_period(&pool, org_id, "chat_message", start, end)
        .await
        .unwrap();

    assert_eq!(usage, 3);
}

#[tokio::test]
async fn test_get_usage_for_period_empty() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    let now = Utc::now();
    let start = now - Duration::hours(1);
    let end = now + Duration::hours(1);

    let usage = get_usage_for_period(&pool, org_id, "chat_message", start, end)
        .await
        .unwrap();

    assert_eq!(usage, 0);
}

#[tokio::test]
async fn test_get_usage_for_period_with_quantity() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    // Record events with different quantities
    record_event(&pool, org_id, "tokens_used", 100, None, None, None)
        .await
        .unwrap();
    record_event(&pool, org_id, "tokens_used", 200, None, None, None)
        .await
        .unwrap();
    record_event(&pool, org_id, "tokens_used", 50, None, None, None)
        .await
        .unwrap();

    let now = Utc::now();
    let start = now - Duration::hours(1);
    let end = now + Duration::hours(1);

    let usage = get_usage_for_period(&pool, org_id, "tokens_used", start, end)
        .await
        .unwrap();

    assert_eq!(usage, 350);
}

#[tokio::test]
async fn test_get_usage_different_event_types() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    // Record different event types
    record_event(&pool, org_id, "chat_message", 5, None, None, None)
        .await
        .unwrap();
    record_event(&pool, org_id, "api_call", 10, None, None, None)
        .await
        .unwrap();

    let now = Utc::now();
    let start = now - Duration::hours(1);
    let end = now + Duration::hours(1);

    let chat_usage = get_usage_for_period(&pool, org_id, "chat_message", start, end)
        .await
        .unwrap();
    let api_usage = get_usage_for_period(&pool, org_id, "api_call", start, end)
        .await
        .unwrap();

    assert_eq!(chat_usage, 5);
    assert_eq!(api_usage, 10);
}

#[tokio::test]
async fn test_check_limit_free_plan() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Free plan has 100 chats per month limit
    // Record 99 chats - should be under limit
    for _ in 0..99 {
        record_event(&pool, org_id, "chat_message", 1, None, None, None)
            .await
            .unwrap();
    }

    let result = check_limit(&pool, org_id, "chat_message").await;
    assert!(result.is_ok());

    // Record one more - should hit limit
    record_event(&pool, org_id, "chat_message", 1, None, None, None)
        .await
        .unwrap();

    let result = check_limit(&pool, org_id, "chat_message").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_check_limit_pro_plan() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    create_subscription(&pool, org_id, pro_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Pro plan has 5000 chats per month limit
    // Record 100 chats - should be well under limit
    for _ in 0..100 {
        record_event(&pool, org_id, "chat_message", 1, None, None, None)
            .await
            .unwrap();
    }

    let result = check_limit(&pool, org_id, "chat_message").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_check_limit_enterprise_plan_unlimited() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let ent_plan = get_plan_by_slug(&pool, "enterprise")
        .await
        .unwrap()
        .unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    create_subscription(&pool, org_id, ent_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Enterprise plan has unlimited chats (-1)
    // Record many chats - should never hit limit
    for _ in 0..10000 {
        record_event(&pool, org_id, "chat_message", 1, None, None, None)
            .await
            .unwrap();
    }

    let result = check_limit(&pool, org_id, "chat_message").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_usage_cascade_delete_org() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;

    record_event(&pool, org_id, "chat_message", 1, None, None, None)
        .await
        .unwrap();

    // Delete the organization
    sqlx::query!("DELETE FROM organizations WHERE id = $1", org_id)
        .execute(&pool)
        .await
        .unwrap();

    // Usage events should be deleted
    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM usage_events WHERE organization_id = $1",
    )
    .bind(org_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_usage_cascade_delete_workspace_set_null() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let workspace_id = create_test_workspace(&pool, org_id).await;

    record_event(
        &pool,
        org_id,
        "chat_message",
        1,
        Some(workspace_id),
        None,
        None,
    )
    .await
    .unwrap();

    // Delete the workspace
    sqlx::query!("DELETE FROM workspaces WHERE id = $1", workspace_id)
        .execute(&pool)
        .await
        .unwrap();

    // Usage events should still exist but workspace_id should be NULL
    let event: (Option<Uuid>,) =
        sqlx::query_as("SELECT workspace_id FROM usage_events WHERE organization_id = $1")
            .bind(org_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(event.0, None);
}

#[tokio::test]
async fn test_usage_cascade_delete_user_set_null() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let user_id = create_test_user(&pool).await;

    record_event(&pool, org_id, "chat_message", 1, None, Some(user_id), None)
        .await
        .unwrap();

    // Delete the user
    sqlx::query!("DELETE FROM users WHERE id = $1", user_id)
        .execute(&pool)
        .await
        .unwrap();

    // Usage events should still exist but user_id should be NULL
    let event: (Option<Uuid>,) =
        sqlx::query_as("SELECT user_id FROM usage_events WHERE organization_id = $1")
            .bind(org_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(event.0, None);
}
