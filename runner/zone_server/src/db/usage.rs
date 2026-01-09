//! Database operations for usage tracking

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::subscriptions::{SubscriptionStatus, get_org_subscription};

/// Custom error for limit exceeded
#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    #[error("Usage limit exceeded for {event_type}: {current}/{limit}")]
    LimitExceeded {
        event_type: String,
        current: i64,
        limit: i32,
    },
    #[error("Organization has no subscription")]
    NoSubscription,
    #[error("Subscription is inactive or canceled")]
    SubscriptionInactive,
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Record a usage event
pub async fn record_event(
    pool: &PgPool,
    org_id: Uuid,
    event_type: &str,
    quantity: i64,
    workspace_id: Option<Uuid>,
    user_id: Option<Uuid>,
    metadata: Option<Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO usage_events (organization_id, workspace_id, user_id, event_type, quantity, metadata)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        org_id,
        workspace_id,
        user_id,
        event_type,
        quantity,
        metadata
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Get total usage for a specific event type within a time period
pub async fn get_usage_for_period(
    pool: &PgPool,
    org_id: Uuid,
    event_type: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(SUM(quantity), 0)::bigint as "total!"
        FROM usage_events
        WHERE organization_id = $1
          AND event_type = $2
          AND recorded_at >= $3
          AND recorded_at < $4
        "#,
        org_id,
        event_type,
        start,
        end
    )
    .fetch_one(pool)
    .await?;

    Ok(result)
}

/// Check if organization has exceeded their limit for a specific event type
/// Uses current billing period for the check
pub async fn check_limit(pool: &PgPool, org_id: Uuid, event_type: &str) -> Result<(), UsageError> {
    // Get the organization's subscription
    let subscription = get_org_subscription(pool, org_id)
        .await?
        .ok_or(UsageError::NoSubscription)?;

    // Get the plan limits
    let plan = super::plans::get_plan_by_id(pool, subscription.plan_id)
        .await?
        .ok_or(UsageError::NoSubscription)?;

    // Parse the limits
    let limits: super::plans::PlanLimits =
        serde_json::from_value(plan.limits).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

    // Determine which limit to check based on event type
    let limit = match event_type {
        "chat_message" => limits.max_chats_per_month,
        _ => return Ok(()), // Unknown event types are not limited
    };

    // -1 means unlimited
    if limit == -1 {
        return Ok(());
    }

    // Get current usage for this billing period
    let current_usage = get_usage_for_period(
        pool,
        org_id,
        event_type,
        subscription.current_period_start,
        subscription.current_period_end,
    )
    .await?;

    if current_usage >= limit as i64 {
        return Err(UsageError::LimitExceeded {
            event_type: event_type.to_string(),
            current: current_usage,
            limit,
        });
    }

    Ok(())
}

/// Record an event with atomic limit checking
/// This function atomically checks the limit and records the event in a single query,
/// preventing race conditions where concurrent requests could bypass limits.
pub async fn record_event_with_limit_check(
    pool: &PgPool,
    org_id: Uuid,
    event_type: &str,
    quantity: i64,
    workspace_id: Option<Uuid>,
    user_id: Option<Uuid>,
    metadata: Option<Value>,
) -> Result<Uuid, UsageError> {
    // Get subscription and limits first
    let subscription = get_org_subscription(pool, org_id)
        .await
        .map_err(|e| UsageError::DatabaseError(e.to_string()))?
        .ok_or(UsageError::NoSubscription)?;

    // Check subscription status
    if subscription.status == SubscriptionStatus::Canceled {
        return Err(UsageError::SubscriptionInactive);
    }

    let limits = super::plans::get_plan_limits(pool, subscription.plan_id)
        .await
        .map_err(|e| UsageError::DatabaseError(e.to_string()))?;

    let limit = match event_type {
        "chat_message" => limits.max_chats_per_month,
        _ => {
            // Unknown event types are not limited, just record them
            record_event(
                pool,
                org_id,
                event_type,
                quantity,
                workspace_id,
                user_id,
                metadata,
            )
            .await?;
            // Return a dummy UUID since we don't have the ID from the non-returning insert
            return Ok(Uuid::nil());
        }
    };

    // -1 means unlimited
    if limit < 0 {
        record_event(
            pool,
            org_id,
            event_type,
            quantity,
            workspace_id,
            user_id,
            metadata,
        )
        .await?;
        return Ok(Uuid::nil());
    }

    // Use CTE to atomically check and insert
    let (period_start, period_end) = (
        subscription.current_period_start,
        subscription.current_period_end,
    );

    let result = sqlx::query_scalar::<_, Uuid>(
        r#"
        WITH current_usage AS (
            SELECT COALESCE(SUM(quantity), 0) as total
            FROM usage_events
            WHERE organization_id = $1
              AND event_type = $2
              AND recorded_at >= $3
              AND recorded_at < $4
        )
        INSERT INTO usage_events (organization_id, event_type, quantity, workspace_id, user_id, metadata, recorded_at)
        SELECT $1, $2, $5, $6, $7, $8, NOW()
        WHERE (SELECT total FROM current_usage) + $5 <= $9
        RETURNING id
        "#,
    )
    .bind(org_id)
    .bind(event_type)
    .bind(period_start)
    .bind(period_end)
    .bind(quantity)
    .bind(workspace_id)
    .bind(user_id)
    .bind(metadata)
    .bind(limit as i64)
    .fetch_optional(pool)
    .await
    .map_err(|e| UsageError::DatabaseError(e.to_string()))?;

    result.ok_or(UsageError::LimitExceeded {
        limit,
        current: -1, // We don't know exact current in atomic case
        event_type: event_type.to_string(),
    })
}
