//! Audit logging routes
//!
//! Provides endpoints for viewing and exporting audit logs.
//! Only organization admins can access audit logs.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::db::{audit, organization_members};
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

/// Query parameters for listing audit logs
#[derive(Debug, Deserialize)]
pub struct ListAuditLogsQuery {
    /// Filter by action type
    #[serde(default)]
    pub action: Option<String>,
    /// Filter by resource type
    #[serde(default)]
    pub resource_type: Option<String>,
    /// Filter by resource ID
    #[serde(default)]
    pub resource_id: Option<Uuid>,
    /// Filter by actor ID
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    /// Filter by start date (ISO 8601)
    #[serde(default)]
    pub start_date: Option<String>,
    /// Filter by end date (ISO 8601)
    #[serde(default)]
    pub end_date: Option<String>,
    /// Pagination limit
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Pagination offset
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Query parameters for exporting audit logs
#[derive(Debug, Deserialize)]
pub struct ExportAuditLogsQuery {
    /// Start date (ISO 8601) - required
    pub start_date: String,
    /// End date (ISO 8601) - required
    pub end_date: String,
}

/// Response for listing audit logs
#[derive(Debug, Serialize)]
pub struct ListAuditLogsResponse {
    pub logs: Vec<audit::AuditLog>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// List audit logs for an organization
///
/// GET /api/organizations/:org_id/audit-logs
///
/// Requires admin access to the organization.
/// Supports filtering by action, resource_type, resource_id, actor_id, and date range.
/// Returns paginated results.
pub async fn list_audit_logs(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    Query(query): Query<ListAuditLogsQuery>,
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

    // Parse date filters if provided
    let start_date = if let Some(ref date_str) = query.start_date {
        match DateTime::parse_from_rfc3339(date_str) {
            Ok(dt) => Some(dt.with_timezone(&Utc)),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "Invalid start_date format. Use ISO 8601 format.",
                    )),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    let end_date = if let Some(ref date_str) = query.end_date {
        match DateTime::parse_from_rfc3339(date_str) {
            Ok(dt) => Some(dt.with_timezone(&Utc)),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "Invalid end_date format. Use ISO 8601 format.",
                    )),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    // Validate limit
    let limit = if query.limit > 100 {
        100
    } else if query.limit < 1 {
        50
    } else {
        query.limit
    };

    // Build filters
    let filters = audit::AuditFilters {
        action: query.action,
        resource_type: query.resource_type,
        resource_id: query.resource_id,
        actor_id: query.actor_id,
        start_date,
        end_date,
    };

    // Get logs and total count
    let logs = match audit::list_audit_logs(state.db(), org_id, &filters, limit, query.offset).await
    {
        Ok(logs) => logs,
        Err(e) => {
            tracing::error!("Failed to list audit logs: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to list audit logs")),
            )
                .into_response();
        }
    };

    let total = match audit::count_audit_logs(state.db(), org_id, &filters).await {
        Ok(count) => count,
        Err(e) => {
            tracing::error!("Failed to count audit logs: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to count audit logs")),
            )
                .into_response();
        }
    };

    Json(ListAuditLogsResponse {
        logs,
        total,
        limit,
        offset: query.offset,
    })
    .into_response()
}

/// Get a single audit log by ID
///
/// GET /api/organizations/:org_id/audit-logs/:log_id
///
/// Requires admin access to the organization.
pub async fn get_audit_log(
    State(state): State<AppState>,
    Path((org_id, log_id)): Path<(Uuid, Uuid)>,
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

    // Get the log
    let log = match audit::get_audit_log(state.db(), log_id).await {
        Ok(Some(log)) => {
            // Verify the log belongs to this organization
            if log.organization_id != Some(org_id) {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new("Audit log not found")),
                )
                    .into_response();
            }
            log
        }
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Audit log not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get audit log: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get audit log")),
            )
                .into_response();
        }
    };

    Json(log).into_response()
}

/// Export audit logs as CSV
///
/// GET /api/organizations/:org_id/audit-logs/export
///
/// Requires admin access to the organization.
/// Query parameters `start_date` and `end_date` are required (ISO 8601 format).
pub async fn export_audit_logs_csv(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    Query(query): Query<ExportAuditLogsQuery>,
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

    // Parse dates
    let start_date = match DateTime::parse_from_rfc3339(&query.start_date) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid start_date format. Use ISO 8601 format.",
                )),
            )
                .into_response();
        }
    };

    let end_date = match DateTime::parse_from_rfc3339(&query.end_date) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid end_date format. Use ISO 8601 format.",
                )),
            )
                .into_response();
        }
    };

    // Export logs as CSV
    match audit::export_audit_logs_csv(state.db(), org_id, start_date, end_date).await {
        Ok(csv) => (
            StatusCode::OK,
            [
                ("Content-Type", "text/csv"),
                (
                    "Content-Disposition",
                    &format!("attachment; filename=\"audit-logs-{}.csv\"", org_id),
                ),
            ],
            csv,
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to export audit logs: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to export audit logs")),
            )
                .into_response()
        }
    }
}
