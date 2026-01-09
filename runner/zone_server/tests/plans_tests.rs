//! Tests for plans database module

mod common;

use sqlx::PgPool;
use zone_server::db::plans::{
    get_plan_by_id, get_plan_by_slug, get_plan_limits, list_public_plans,
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

#[tokio::test]
async fn test_list_public_plans() {
    let pool = create_test_pool().await;
    let plans = list_public_plans(&pool).await.unwrap();

    // Should return the 3 seeded plans
    assert_eq!(plans.len(), 3);

    // Check that all plans are public and active
    for plan in &plans {
        assert!(plan.is_public);
        assert!(plan.is_active);
    }

    // Check that we have the expected plan slugs
    let slugs: Vec<&str> = plans.iter().map(|p| p.slug.as_str()).collect();
    assert!(slugs.contains(&"free"));
    assert!(slugs.contains(&"pro"));
    assert!(slugs.contains(&"enterprise"));
}

#[tokio::test]
async fn test_get_plan_by_slug() {
    let pool = create_test_pool().await;

    // Get the free plan
    let plan = get_plan_by_slug(&pool, "free").await.unwrap();
    assert!(plan.is_some());

    let free_plan = plan.unwrap();
    assert_eq!(free_plan.slug, "free");
    assert_eq!(free_plan.name, "Free");
    assert_eq!(free_plan.price_monthly_cents, 0);
    assert_eq!(free_plan.price_yearly_cents, 0);

    // Get the pro plan
    let plan = get_plan_by_slug(&pool, "pro").await.unwrap();
    assert!(plan.is_some());

    let pro_plan = plan.unwrap();
    assert_eq!(pro_plan.slug, "pro");
    assert_eq!(pro_plan.name, "Pro");
    assert_eq!(pro_plan.price_monthly_cents, 2900);
    assert_eq!(pro_plan.price_yearly_cents, 29000);

    // Get the enterprise plan
    let plan = get_plan_by_slug(&pool, "enterprise").await.unwrap();
    assert!(plan.is_some());

    let ent_plan = plan.unwrap();
    assert_eq!(ent_plan.slug, "enterprise");
    assert_eq!(ent_plan.name, "Enterprise");
    assert_eq!(ent_plan.price_monthly_cents, 9900);
    assert_eq!(ent_plan.price_yearly_cents, 99000);
}

#[tokio::test]
async fn test_get_plan_by_slug_not_found() {
    let pool = create_test_pool().await;
    let plan = get_plan_by_slug(&pool, "nonexistent").await.unwrap();
    assert!(plan.is_none());
}

#[tokio::test]
async fn test_get_plan_by_id() {
    let pool = create_test_pool().await;

    // First get a plan by slug to get its ID
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();

    // Now get it by ID
    let plan = get_plan_by_id(&pool, free_plan.id).await.unwrap();
    assert!(plan.is_some());

    let plan = plan.unwrap();
    assert_eq!(plan.id, free_plan.id);
    assert_eq!(plan.slug, "free");
}

#[tokio::test]
async fn test_get_plan_by_id_not_found() {
    let pool = create_test_pool().await;
    let fake_id = uuid::Uuid::new_v4();
    let plan = get_plan_by_id(&pool, fake_id).await.unwrap();
    assert!(plan.is_none());
}

#[tokio::test]
async fn test_get_plan_limits_free() {
    let pool = create_test_pool().await;
    let free_plan = get_plan_by_slug(&pool, "free").await.unwrap().unwrap();
    let limits = get_plan_limits(&pool, free_plan.id).await.unwrap();

    assert_eq!(limits.max_workspaces, 1);
    assert_eq!(limits.max_members, 3);
    assert_eq!(limits.max_chats_per_month, 100);
}

#[tokio::test]
async fn test_get_plan_limits_pro() {
    let pool = create_test_pool().await;
    let pro_plan = get_plan_by_slug(&pool, "pro").await.unwrap().unwrap();
    let limits = get_plan_limits(&pool, pro_plan.id).await.unwrap();

    assert_eq!(limits.max_workspaces, 10);
    assert_eq!(limits.max_members, 25);
    assert_eq!(limits.max_chats_per_month, 5000);
}

#[tokio::test]
async fn test_get_plan_limits_enterprise() {
    let pool = create_test_pool().await;
    let ent_plan = get_plan_by_slug(&pool, "enterprise")
        .await
        .unwrap()
        .unwrap();
    let limits = get_plan_limits(&pool, ent_plan.id).await.unwrap();

    // -1 means unlimited
    assert_eq!(limits.max_workspaces, -1);
    assert_eq!(limits.max_members, -1);
    assert_eq!(limits.max_chats_per_month, -1);
}

#[tokio::test]
async fn test_plan_features_parsing() {
    let pool = create_test_pool().await;
    let plans = list_public_plans(&pool).await.unwrap();

    // Check free plan features
    let free_plan = plans.iter().find(|p| p.slug == "free").unwrap();
    assert!(free_plan.features["api_access"].as_bool().unwrap());
    assert!(
        !free_plan
            .features
            .as_object()
            .unwrap()
            .contains_key("priority_support")
    );

    // Check pro plan features
    let pro_plan = plans.iter().find(|p| p.slug == "pro").unwrap();
    assert!(pro_plan.features["api_access"].as_bool().unwrap());
    assert!(pro_plan.features["priority_support"].as_bool().unwrap());

    // Check enterprise plan features
    let ent_plan = plans.iter().find(|p| p.slug == "enterprise").unwrap();
    assert!(ent_plan.features["api_access"].as_bool().unwrap());
    assert!(ent_plan.features["priority_support"].as_bool().unwrap());
    assert!(ent_plan.features["sso"].as_bool().unwrap());
    assert!(ent_plan.features["audit_log"].as_bool().unwrap());
}

#[tokio::test]
async fn test_plan_limits_parsing() {
    let pool = create_test_pool().await;
    let plans = list_public_plans(&pool).await.unwrap();

    for plan in plans {
        let limits_json = &plan.limits;
        assert!(limits_json["max_workspaces"].is_number());
        assert!(limits_json["max_members"].is_number());
        assert!(limits_json["max_chats_per_month"].is_number());
    }
}
