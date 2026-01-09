//! Database operations for subscription plans

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// A subscription plan
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Plan {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub price_monthly_cents: i32,
    pub price_yearly_cents: i32,
    pub is_active: bool,
    pub is_public: bool,
    pub features: serde_json::Value,
    pub limits: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Default value for max_workspaces
fn default_max_workspaces() -> i32 {
    1
}

/// Default value for max_members
fn default_max_members() -> i32 {
    3
}

/// Default value for max_chats_per_month
fn default_max_chats() -> i32 {
    100
}

/// Plan limits extracted from the limits JSONB field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanLimits {
    #[serde(default = "default_max_workspaces")]
    pub max_workspaces: i32,
    #[serde(default = "default_max_members")]
    pub max_members: i32,
    #[serde(default = "default_max_chats")]
    pub max_chats_per_month: i32,
}

/// List all public and active plans
pub async fn list_public_plans(pool: &PgPool) -> Result<Vec<Plan>, sqlx::Error> {
    sqlx::query_as!(
        Plan,
        r#"
        SELECT id, name, slug, description, price_monthly_cents, price_yearly_cents,
               is_active, is_public, features, limits, created_at, updated_at
        FROM plans
        WHERE is_public = TRUE AND is_active = TRUE
        ORDER BY price_monthly_cents ASC
        "#
    )
    .fetch_all(pool)
    .await
}

/// Get a plan by its ID
pub async fn get_plan_by_id(pool: &PgPool, plan_id: Uuid) -> Result<Option<Plan>, sqlx::Error> {
    sqlx::query_as!(
        Plan,
        r#"
        SELECT id, name, slug, description, price_monthly_cents, price_yearly_cents,
               is_active, is_public, features, limits, created_at, updated_at
        FROM plans
        WHERE id = $1
        "#,
        plan_id
    )
    .fetch_optional(pool)
    .await
}

/// Get a plan by its slug
pub async fn get_plan_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Plan>, sqlx::Error> {
    sqlx::query_as!(
        Plan,
        r#"
        SELECT id, name, slug, description, price_monthly_cents, price_yearly_cents,
               is_active, is_public, features, limits, created_at, updated_at
        FROM plans
        WHERE slug = $1
        "#,
        slug
    )
    .fetch_optional(pool)
    .await
}

/// Get plan limits for a specific plan
pub async fn get_plan_limits(pool: &PgPool, plan_id: Uuid) -> Result<PlanLimits, sqlx::Error> {
    let plan = get_plan_by_id(pool, plan_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)?;

    // Parse the limits from the JSONB field
    let limits: PlanLimits =
        serde_json::from_value(plan.limits).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

    Ok(limits)
}
