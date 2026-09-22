//! Billing and subscription routes
//!
//! Provides endpoints for plans, subscriptions, usage and limits. An
//! organization that has no subscription row yet is put on the default plan
//! the first time its billing is read.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::db::organization_members;
use crate::db::plans::{Plan, get_plan_by_id, get_plan_limits, list_public_plans};
use crate::db::subscriptions::{Subscription, get_or_create_org_subscription};
use crate::db::usage::get_usage_for_period;
use crate::db::workspaces;
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

const USAGE_CHAT_MESSAGE: &str = "chat_message";

/// A subscription without its Stripe identifiers, named by its plan.
#[derive(Debug, Serialize)]
pub struct SafeSubscription {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub plan_id: Uuid,
    pub plan_name: String,
    pub plan_slug: String,
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

impl SafeSubscription {
    fn new(subscription: Subscription, plan: &Plan) -> Self {
        SafeSubscription {
            id: subscription.id,
            organization_id: subscription.organization_id,
            plan_id: subscription.plan_id,
            plan_name: plan.name.clone(),
            plan_slug: plan.slug.clone(),
            status: subscription.status.as_str().to_string(),
            current_period_start: subscription.current_period_start,
            current_period_end: subscription.current_period_end,
            cancel_at_period_end: subscription.cancel_at_period_end,
            canceled_at: subscription.canceled_at,
            trial_start: subscription.trial_start,
            trial_end: subscription.trial_end,
            timestamps: Timestamps::from_utc_opt(subscription.created_at, subscription.updated_at),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub subscription: SafeSubscription,
    pub plan: Plan,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub current_period_start: chrono::DateTime<chrono::Utc>,
    pub current_period_end: chrono::DateTime<chrono::Utc>,
    pub usage: UsageStats,
}

#[derive(Debug, Serialize)]
pub struct UsageStats {
    pub chat_messages: i64,
    pub members: i64,
    pub workspaces: i64,
}

#[derive(Debug, Serialize)]
struct PlansListResponse {
    plans: Vec<Plan>,
}

#[derive(Debug, Serialize)]
struct SinglePlanResponse {
    plan: Plan,
}

fn failure(status: StatusCode, message: &str) -> Box<Response> {
    Box::new((status, Json(ErrorResponse::new(message))).into_response())
}

fn internal(context: &str, error: impl std::fmt::Display) -> Box<Response> {
    tracing::error!("{context}: {error}");
    failure(StatusCode::INTERNAL_SERVER_ERROR, context)
}

async fn require_admin(
    state: &AppState,
    org_id: Uuid,
    claims_sub: &str,
) -> Result<(), Box<Response>> {
    let user_id = Uuid::parse_str(claims_sub)
        .map_err(|_| failure(StatusCode::UNAUTHORIZED, "Invalid user ID in token"))?;

    let is_admin = organization_members::is_admin(state.db(), org_id, user_id)
        .await
        .map_err(|error| internal("Failed to check admin status", error))?;

    if is_admin {
        Ok(())
    } else {
        Err(failure(StatusCode::FORBIDDEN, "Admin access required"))
    }
}

async fn current_subscription(
    state: &AppState,
    org_id: Uuid,
) -> Result<Subscription, Box<Response>> {
    get_or_create_org_subscription(state.db(), org_id)
        .await
        .map_err(|error| internal("Failed to get subscription", error))?
        .ok_or_else(|| {
            failure(
                StatusCode::NOT_FOUND,
                "No subscription found for this organization",
            )
        })
}

/// List all public plans
///
/// GET /api/plans
pub async fn list_plans(State(state): State<AppState>) -> impl IntoResponse {
    match list_public_plans(state.db()).await {
        Ok(plans) => Json(PlansListResponse { plans }).into_response(),
        Err(error) => *internal("Failed to list plans", error),
    }
}

/// Get a specific plan
///
/// GET /api/plans/:plan_id
pub async fn get_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<Uuid>,
) -> impl IntoResponse {
    match get_plan_by_id(state.db(), plan_id).await {
        Ok(Some(plan)) => Json(SinglePlanResponse { plan }).into_response(),
        Ok(None) => *failure(StatusCode::NOT_FOUND, "Plan not found"),
        Err(error) => *internal("Failed to get plan", error),
    }
}

/// Get the organization's subscription
///
/// GET /api/organizations/:org_id/subscription
pub async fn get_org_subscription_handler(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    if let Err(response) = require_admin(&state, org_id, &claims.sub).await {
        return *response;
    }

    let subscription = match current_subscription(&state, org_id).await {
        Ok(subscription) => subscription,
        Err(response) => return *response,
    };

    let plan = match get_plan_by_id(state.db(), subscription.plan_id).await {
        Ok(Some(plan)) => plan,
        Ok(None) => return *internal("Plan not found", subscription.plan_id),
        Err(error) => return *internal("Failed to get plan", error),
    };

    Json(SubscriptionResponse {
        subscription: SafeSubscription::new(subscription, &plan),
        plan,
    })
    .into_response()
}

/// Get usage for the current billing period
///
/// GET /api/organizations/:org_id/usage
pub async fn get_org_usage(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    if let Err(response) = require_admin(&state, org_id, &claims.sub).await {
        return *response;
    }

    let subscription = match current_subscription(&state, org_id).await {
        Ok(subscription) => subscription,
        Err(response) => return *response,
    };

    let chat_messages = match get_usage_for_period(
        state.db(),
        org_id,
        USAGE_CHAT_MESSAGE,
        subscription.current_period_start,
        subscription.current_period_end,
    )
    .await
    {
        Ok(usage) => usage,
        Err(error) => return *internal("Failed to get usage", error),
    };

    let members = match organization_members::count_active_members(state.db(), org_id).await {
        Ok(count) => count,
        Err(error) => return *internal("Failed to count members", error),
    };

    let workspaces = match workspaces::count_active_workspaces(state.db(), org_id).await {
        Ok(count) => count,
        Err(error) => return *internal("Failed to count workspaces", error),
    };

    Json(UsageResponse {
        current_period_start: subscription.current_period_start,
        current_period_end: subscription.current_period_end,
        usage: UsageStats {
            chat_messages,
            members,
            workspaces,
        },
    })
    .into_response()
}

/// Get the organization's plan limits
///
/// GET /api/organizations/:org_id/limits
pub async fn get_org_limits_handler(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    if let Err(response) = require_admin(&state, org_id, &claims.sub).await {
        return *response;
    }

    let subscription = match current_subscription(&state, org_id).await {
        Ok(subscription) => subscription,
        Err(response) => return *response,
    };

    match get_plan_limits(state.db(), subscription.plan_id).await {
        Ok(limits) => Json(limits).into_response(),
        Err(error) => *internal("Failed to get limits", error),
    }
}
