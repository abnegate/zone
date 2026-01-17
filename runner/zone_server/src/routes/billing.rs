//! Billing and subscription routes

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::db::organization_members;
use crate::db::plans::{Plan, get_plan_by_id, list_public_plans};
use crate::db::subscriptions::{Subscription, get_org_limits, get_org_subscription};
use crate::db::usage::get_usage_for_period;
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

/// Safe subscription data without sensitive Stripe information
#[derive(Debug, Serialize)]
pub struct SafeSubscription {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub plan_id: Uuid,
    pub status: String,
    pub current_period_start: chrono::DateTime<chrono::Utc>,
    pub current_period_end: chrono::DateTime<chrono::Utc>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub trial_start: Option<chrono::DateTime<chrono::Utc>>,
    pub trial_end: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(flatten)]
    pub timestamps: Timestamps,
}

impl From<Subscription> for SafeSubscription {
    fn from(sub: Subscription) -> Self {
        SafeSubscription {
            id: sub.id,
            organization_id: sub.organization_id,
            plan_id: sub.plan_id,
            status: sub.status.as_str().to_string(),
            current_period_start: sub.current_period_start,
            current_period_end: sub.current_period_end,
            cancel_at_period_end: sub.cancel_at_period_end,
            canceled_at: sub.canceled_at,
            trial_start: sub.trial_start,
            trial_end: sub.trial_end,
            timestamps: Timestamps::from_utc_opt(sub.created_at, sub.updated_at),
        }
    }
}

/// Response for subscription details
#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub subscription: SafeSubscription,
    pub plan: Plan,
}

/// Response for usage stats
#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub current_period_start: chrono::DateTime<chrono::Utc>,
    pub current_period_end: chrono::DateTime<chrono::Utc>,
    pub usage: UsageStats,
}

#[derive(Debug, Serialize)]
pub struct UsageStats {
    pub chat_messages: i64,
}

/// List all public plans
///
/// GET /api/plans
pub async fn list_plans(State(state): State<AppState>) -> impl IntoResponse {
    match list_public_plans(state.db()).await {
        Ok(plans) => Json(plans).into_response(),
        Err(e) => {
            tracing::error!("Failed to list plans: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to list plans")),
            )
                .into_response()
        }
    }
}

/// Get a specific plan by ID
///
/// GET /api/plans/:plan_id
pub async fn get_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<Uuid>,
) -> impl IntoResponse {
    match get_plan_by_id(state.db(), plan_id).await {
        Ok(Some(plan)) => Json(plan).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Plan not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get plan: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get plan")),
            )
                .into_response()
        }
    }
}

/// Get organization subscription
///
/// GET /api/organizations/:org_id/subscription
pub async fn get_org_subscription_handler(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Verify user is an admin of the organization
    let is_admin = match organization_members::is_admin(state.db(), org_id, user_id).await {
        Ok(admin) => admin,
        Err(e) => {
            tracing::error!("Failed to check admin status: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to check admin status")),
            )
                .into_response();
        }
    };

    if !is_admin {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Admin access required")),
        )
            .into_response();
    }

    // Get subscription
    let subscription = match get_org_subscription(state.db(), org_id).await {
        Ok(Some(sub)) => sub,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new(
                    "No subscription found for this organization",
                )),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get subscription: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get subscription")),
            )
                .into_response();
        }
    };

    // Get plan details
    let plan = match get_plan_by_id(state.db(), subscription.plan_id).await {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            tracing::error!("Plan not found for subscription");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Plan not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get plan: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get plan")),
            )
                .into_response();
        }
    };

    Json(SubscriptionResponse {
        subscription: subscription.into(),
        plan,
    })
    .into_response()
}

/// Get organization usage
///
/// GET /api/organizations/:org_id/usage
pub async fn get_org_usage(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Verify user is an admin of the organization
    let is_admin = match organization_members::is_admin(state.db(), org_id, user_id).await {
        Ok(admin) => admin,
        Err(e) => {
            tracing::error!("Failed to check admin status: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to check admin status")),
            )
                .into_response();
        }
    };

    if !is_admin {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Admin access required")),
        )
            .into_response();
    }

    // Get subscription to determine billing period
    let subscription = match get_org_subscription(state.db(), org_id).await {
        Ok(Some(sub)) => sub,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new(
                    "No subscription found for this organization",
                )),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get subscription: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get subscription")),
            )
                .into_response();
        }
    };

    // Get usage for current period
    let chat_messages = match get_usage_for_period(
        state.db(),
        org_id,
        "chat_message",
        subscription.current_period_start,
        subscription.current_period_end,
    )
    .await
    {
        Ok(usage) => usage,
        Err(e) => {
            tracing::error!("Failed to get usage: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get usage")),
            )
                .into_response();
        }
    };

    Json(UsageResponse {
        current_period_start: subscription.current_period_start,
        current_period_end: subscription.current_period_end,
        usage: UsageStats { chat_messages },
    })
    .into_response()
}

/// Get organization limits
///
/// GET /api/organizations/:org_id/limits
pub async fn get_org_limits_handler(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Verify user is an admin of the organization
    let is_admin = match organization_members::is_admin(state.db(), org_id, user_id).await {
        Ok(admin) => admin,
        Err(e) => {
            tracing::error!("Failed to check admin status: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to check admin status")),
            )
                .into_response();
        }
    };

    if !is_admin {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Admin access required")),
        )
            .into_response();
    }

    // Get limits
    match get_org_limits(state.db(), org_id).await {
        Ok(limits) => Json(limits).into_response(),
        Err(e) => {
            tracing::error!("Failed to get limits: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get limits")),
            )
                .into_response()
        }
    }
}
