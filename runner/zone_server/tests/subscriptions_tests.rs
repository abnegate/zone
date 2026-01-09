//! Tests for subscriptions database module

mod common;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::plans::get_plan_by_slug;
use zone_server::db::subscriptions::{
    SubscriptionStatus, cancel_subscription, create_subscription, get_org_limits,
    get_org_subscription, update_subscription_status,
};

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

#[tokio::test]
async fn test_create_subscription() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    let subscription =
        create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end)
            .await
            .unwrap();

    assert_eq!(subscription.organization_id, org_id);
    assert_eq!(subscription.plan_id, free_plan.id);
    assert_eq!(subscription.status, SubscriptionStatus::Active);
    assert!(!subscription.cancel_at_period_end);
    assert!(subscription.canceled_at.is_none());
    assert!(subscription.stripe_subscription_id.is_none());
    assert!(subscription.stripe_customer_id.is_none());
}

#[tokio::test]
async fn test_create_subscription_with_stripe() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    let subscription = create_subscription(
        &pool,
        org_id,
        pro_plan.id,
        Some("sub_test123".to_string()),
        Some("cus_test123".to_string()),
        now,
        period_end,
    )
    .await
    .unwrap();

    assert_eq!(
        subscription.stripe_subscription_id,
        Some("sub_test123".to_string())
    );
    assert_eq!(
        subscription.stripe_customer_id,
        Some("cus_test123".to_string())
    );
}

#[tokio::test]
async fn test_create_subscription_with_trial() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();

    let now = Utc::now();
    let trial_end = now + Duration::days(14);
    let period_end = now + Duration::days(30);

    let subscription = create_subscription(&pool, org_id, pro_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Update to add trial period
    let _ = sqlx::query(
        r#"
        UPDATE subscriptions
        SET status = 'trialing', trial_start = $1, trial_end = $2
        WHERE id = $3
        "#,
    )
    .bind(now)
    .bind(trial_end)
    .bind(subscription.id)
    .execute(&pool)
    .await
    .unwrap();

    let sub = get_org_subscription(&pool, org_id).await.unwrap().unwrap();
    assert_eq!(sub.status, SubscriptionStatus::Trialing);
    assert!(sub.trial_start.is_some());
    assert!(sub.trial_end.is_some());
}

#[tokio::test]
async fn test_get_org_subscription() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    let created_sub = create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    let subscription = get_org_subscription(&pool, org_id).await.unwrap().unwrap();

    assert_eq!(subscription.id, created_sub.id);
    assert_eq!(subscription.organization_id, org_id);
    assert_eq!(subscription.plan_id, free_plan.id);
}

#[tokio::test]
async fn test_get_org_subscription_not_found() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let subscription = get_org_subscription(&pool, org_id).await.unwrap();
    assert!(subscription.is_none());
}

#[tokio::test]
async fn test_update_subscription_status() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    let subscription = create_subscription(&pool, org_id, pro_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Update to past_due
    update_subscription_status(&pool, subscription.id, SubscriptionStatus::PastDue)
        .await
        .unwrap();

    let updated = get_org_subscription(&pool, org_id).await.unwrap().unwrap();
    assert_eq!(updated.status, SubscriptionStatus::PastDue);

    // Update to canceled
    update_subscription_status(&pool, subscription.id, SubscriptionStatus::Canceled)
        .await
        .unwrap();

    let updated = get_org_subscription(&pool, org_id).await.unwrap().unwrap();
    assert_eq!(updated.status, SubscriptionStatus::Canceled);
}

#[tokio::test]
async fn test_cancel_subscription() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    let subscription = create_subscription(&pool, org_id, pro_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Cancel at period end
    cancel_subscription(&pool, subscription.id, true)
        .await
        .unwrap();

    let canceled = get_org_subscription(&pool, org_id).await.unwrap().unwrap();
    assert!(canceled.cancel_at_period_end);
    assert!(canceled.canceled_at.is_some());
    assert_eq!(canceled.status, SubscriptionStatus::Active); // Still active until period end

    // Immediately cancel
    let org_id2 = create_test_org(&pool).await;
    let subscription2 =
        create_subscription(&pool, org_id2, pro_plan.id, None, None, now, period_end)
            .await
            .unwrap();

    cancel_subscription(&pool, subscription2.id, false)
        .await
        .unwrap();

    let canceled2 = get_org_subscription(&pool, org_id2).await.unwrap().unwrap();
    assert!(!canceled2.cancel_at_period_end);
    assert!(canceled2.canceled_at.is_some());
    assert_eq!(canceled2.status, SubscriptionStatus::Canceled);
}

#[tokio::test]
async fn test_get_org_limits() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    let limits = get_org_limits(&pool, org_id).await.unwrap();

    assert_eq!(limits.max_workspaces, 1);
    assert_eq!(limits.max_members, 3);
    assert_eq!(limits.max_chats_per_month, 100);
}

#[tokio::test]
async fn test_get_org_limits_pro() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    create_subscription(&pool, org_id, pro_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    let limits = get_org_limits(&pool, org_id).await.unwrap();

    assert_eq!(limits.max_workspaces, 10);
    assert_eq!(limits.max_members, 25);
    assert_eq!(limits.max_chats_per_month, 5000);
}

#[tokio::test]
async fn test_get_org_limits_enterprise() {
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

    let limits = get_org_limits(&pool, org_id).await.unwrap();

    // -1 means unlimited
    assert_eq!(limits.max_workspaces, -1);
    assert_eq!(limits.max_members, -1);
    assert_eq!(limits.max_chats_per_month, -1);
}

#[tokio::test]
async fn test_unique_org_subscription() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    // Create first subscription
    create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Try to create second subscription for same org - should fail
    let result =
        create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_subscription_cascade_delete() {
    let pool = create_test_pool().await;
    let org_id = create_test_org(&pool).await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    let now = Utc::now();
    let period_end = now + Duration::days(30);

    create_subscription(&pool, org_id, free_plan.id, None, None, now, period_end)
        .await
        .unwrap();

    // Delete the organization
    sqlx::query!("DELETE FROM organizations WHERE id = $1", org_id)
        .execute(&pool)
        .await
        .unwrap();

    // Subscription should be deleted too
    let subscription = get_org_subscription(&pool, org_id).await.unwrap();
    assert!(subscription.is_none());
}
