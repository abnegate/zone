//! Database operations for subscriptions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::plans::{PlanLimits, get_plan_limits};

/// Subscription status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text")]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Active,
    PastDue,
    Canceled,
    Trialing,
}

impl std::str::FromStr for SubscriptionStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "past_due" => Ok(Self::PastDue),
            "canceled" => Ok(Self::Canceled),
            "trialing" => Ok(Self::Trialing),
            _ => Err(()),
        }
    }
}

impl SubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::PastDue => "past_due",
            Self::Canceled => "canceled",
            Self::Trialing => "trialing",
        }
    }
}

/// A subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub plan_id: Uuid,
    pub status: SubscriptionStatus,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    pub trial_start: Option<DateTime<Utc>>,
    pub trial_end: Option<DateTime<Utc>>,
    pub stripe_subscription_id: Option<String>,
    pub stripe_customer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

struct SubscriptionRow {
    id: Uuid,
    organization_id: Uuid,
    plan_id: Uuid,
    status: String,
    current_period_start: DateTime<Utc>,
    current_period_end: DateTime<Utc>,
    cancel_at_period_end: bool,
    canceled_at: Option<DateTime<Utc>>,
    trial_start: Option<DateTime<Utc>>,
    trial_end: Option<DateTime<Utc>>,
    stripe_subscription_id: Option<String>,
    stripe_customer_id: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

impl From<SubscriptionRow> for Subscription {
    fn from(row: SubscriptionRow) -> Self {
        let status = row.status.parse().unwrap_or_else(|_| {
            tracing::error!(
                subscription_id = %row.id,
                unknown_status = %row.status,
                "Unknown subscription status, defaulting to Active"
            );
            SubscriptionStatus::Active
        });

        Subscription {
            id: row.id,
            organization_id: row.organization_id,
            plan_id: row.plan_id,
            status,
            current_period_start: row.current_period_start,
            current_period_end: row.current_period_end,
            cancel_at_period_end: row.cancel_at_period_end,
            canceled_at: row.canceled_at,
            trial_start: row.trial_start,
            trial_end: row.trial_end,
            stripe_subscription_id: row.stripe_subscription_id,
            stripe_customer_id: row.stripe_customer_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Get the subscription for an organization
pub async fn get_org_subscription(
    pool: &PgPool,
    org_id: Uuid,
) -> Result<Option<Subscription>, sqlx::Error> {
    let row = sqlx::query_as!(
        SubscriptionRow,
        r#"
        SELECT id, organization_id, plan_id, status, current_period_start, current_period_end,
               cancel_at_period_end, canceled_at, trial_start, trial_end,
               stripe_subscription_id, stripe_customer_id, created_at, updated_at
        FROM subscriptions
        WHERE organization_id = $1
        "#,
        org_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

/// Create a new subscription
pub async fn create_subscription(
    pool: &PgPool,
    org_id: Uuid,
    plan_id: Uuid,
    stripe_subscription_id: Option<String>,
    stripe_customer_id: Option<String>,
    current_period_start: DateTime<Utc>,
    current_period_end: DateTime<Utc>,
) -> Result<Subscription, sqlx::Error> {
    // Verify plan is valid
    let plan = super::plans::get_plan_by_id(pool, plan_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)?;

    if !plan.is_active {
        return Err(sqlx::Error::Protocol("Plan is not active".into()));
    }

    let row = sqlx::query_as!(
        SubscriptionRow,
        r#"
        INSERT INTO subscriptions (
            organization_id, plan_id, status,
            current_period_start, current_period_end,
            stripe_subscription_id, stripe_customer_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, organization_id, plan_id, status, current_period_start, current_period_end,
                  cancel_at_period_end, canceled_at, trial_start, trial_end,
                  stripe_subscription_id, stripe_customer_id, created_at, updated_at
        "#,
        org_id,
        plan_id,
        SubscriptionStatus::Active.as_str(),
        current_period_start,
        current_period_end,
        stripe_subscription_id,
        stripe_customer_id
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

/// Update subscription status
pub async fn update_subscription_status(
    pool: &PgPool,
    subscription_id: Uuid,
    status: SubscriptionStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE subscriptions
        SET status = $1, updated_at = NOW()
        WHERE id = $2
        "#,
        status.as_str(),
        subscription_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Cancel a subscription
pub async fn cancel_subscription(
    pool: &PgPool,
    subscription_id: Uuid,
    cancel_at_period_end: bool,
) -> Result<(), sqlx::Error> {
    if cancel_at_period_end {
        // Mark for cancellation at period end
        sqlx::query!(
            r#"
            UPDATE subscriptions
            SET cancel_at_period_end = TRUE, canceled_at = NOW(), updated_at = NOW()
            WHERE id = $1
            "#,
            subscription_id
        )
        .execute(pool)
        .await?;
    } else {
        // Cancel immediately
        sqlx::query!(
            r#"
            UPDATE subscriptions
            SET status = $1, cancel_at_period_end = FALSE, canceled_at = NOW(), updated_at = NOW()
            WHERE id = $2
            "#,
            SubscriptionStatus::Canceled.as_str(),
            subscription_id
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Get plan limits for an organization based on their subscription
pub async fn get_org_limits(pool: &PgPool, org_id: Uuid) -> Result<PlanLimits, sqlx::Error> {
    let subscription = get_org_subscription(pool, org_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)?;

    get_plan_limits(pool, subscription.plan_id).await
}
