//! The Community audit log: organization admins list, read and export their organization's
//! audit logs. The Enterprise export, streaming and retention routes in the submodules are open
//! to organization owners and admins for their organization, and to server admins for the
//! server's own audit rows.

pub mod access;
pub mod destinations;
pub mod exports;
pub mod refusal;
pub mod retention;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
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

const DEFAULT_LIMIT: i64 = 50;
const MAXIMUM_LIMIT: i64 = 100;
const DATE_FORMAT: &str = "%Y-%m-%d";
const UNAUTHENTICATED: &str = "Invalid user ID in token";
const ADMIN_CHECK_FAILED: &str = "Failed to check admin status";
const ADMIN_REQUIRED: &str = "Admin access required";
const START_DATE_INVALID: &str = "Invalid start_date format. Use ISO 8601 format.";
const END_DATE_INVALID: &str = "Invalid end_date format. Use ISO 8601 format.";
const LOG_MISSING: &str = "Audit log not found";

#[derive(Debug, Deserialize)]
pub struct ListAuditLogsQuery {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub resource_id: Option<Uuid>,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    DEFAULT_LIMIT
}

/// A bound given either as an RFC 3339 instant or as a calendar date; a date
/// on its own means the start of that day.
pub(crate) fn parse_start(value: &str) -> Option<DateTime<Utc>> {
    parse_bound(value, NaiveTime::MIN)
}

/// A bound given either as an RFC 3339 instant or as a calendar date; a date
/// on its own means the end of that day.
pub(crate) fn parse_end(value: &str) -> Option<DateTime<Utc>> {
    parse_bound(value, NaiveTime::from_hms_opt(23, 59, 59)?)
}

fn parse_bound(value: &str, time: NaiveTime) -> Option<DateTime<Utc>> {
    if let Ok(instant) = DateTime::parse_from_rfc3339(value) {
        return Some(instant.with_timezone(&Utc));
    }
    let date = NaiveDate::parse_from_str(value, DATE_FORMAT).ok()?;
    Some(date.and_time(time).and_utc())
}

#[derive(Debug, Deserialize)]
pub struct ExportAuditLogsQuery {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Serialize)]
pub struct ListAuditLogsResponse {
    pub logs: Vec<audit::AuditLog>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// GET /api/organizations/{org_id}/audit-logs
pub async fn list_audit_logs(
    State(state): State<AppState>,
    Path(organization): Path<Uuid>,
    Query(query): Query<ListAuditLogsQuery>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    let user = match Uuid::parse_str(&claims.sub) {
        Ok(user) => user,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new(UNAUTHENTICATED)),
            )
                .into_response();
        }
    };

    let admin = match organization_members::is_admin(state.db(), organization, user).await {
        Ok(admin) => admin,
        Err(error) => {
            tracing::error!(%error, "Failed to check admin status");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(ADMIN_CHECK_FAILED)),
            )
                .into_response();
        }
    };

    if !admin {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(ADMIN_REQUIRED)),
        )
            .into_response();
    }

    let start_date = match query.start_date.as_deref().map(parse_start) {
        Some(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(START_DATE_INVALID)),
            )
                .into_response();
        }
        bound => bound.flatten(),
    };

    let end_date = match query.end_date.as_deref().map(parse_end) {
        Some(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(END_DATE_INVALID)),
            )
                .into_response();
        }
        bound => bound.flatten(),
    };

    let limit = if query.limit > MAXIMUM_LIMIT {
        MAXIMUM_LIMIT
    } else if query.limit < 1 {
        DEFAULT_LIMIT
    } else {
        query.limit
    };

    let filters = audit::AuditFilters {
        action: query.action,
        resource_type: query.resource_type,
        resource_id: query.resource_id,
        actor_id: query.actor_id,
        start_date,
        end_date,
    };

    let logs = match audit::list_audit_logs(state.db(), organization, &filters, limit, query.offset)
        .await
    {
        Ok(logs) => logs,
        Err(error) => {
            tracing::error!(%error, "Failed to list audit logs");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to list audit logs")),
            )
                .into_response();
        }
    };

    let total = match audit::count_audit_logs(state.db(), organization, &filters).await {
        Ok(count) => count,
        Err(error) => {
            tracing::error!(%error, "Failed to count audit logs");
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

/// GET /api/organizations/{org_id}/audit-logs/{log_id}
pub async fn get_audit_log(
    State(state): State<AppState>,
    Path((organization, id)): Path<(Uuid, Uuid)>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    let user = match Uuid::parse_str(&claims.sub) {
        Ok(user) => user,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new(UNAUTHENTICATED)),
            )
                .into_response();
        }
    };

    let admin = match organization_members::is_admin(state.db(), organization, user).await {
        Ok(admin) => admin,
        Err(error) => {
            tracing::error!(%error, "Failed to check admin status");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(ADMIN_CHECK_FAILED)),
            )
                .into_response();
        }
    };

    if !admin {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(ADMIN_REQUIRED)),
        )
            .into_response();
    }

    let log = match audit::get_audit_log(state.db(), id).await {
        Ok(Some(log)) if log.organization_id == Some(organization) => log,
        Ok(_) => {
            return (StatusCode::NOT_FOUND, Json(ErrorResponse::new(LOG_MISSING))).into_response();
        }
        Err(error) => {
            tracing::error!(%error, "Failed to get audit log");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to get audit log")),
            )
                .into_response();
        }
    };

    Json(log).into_response()
}

/// GET /api/organizations/{org_id}/audit-logs/export
pub async fn export_audit_logs_csv(
    State(state): State<AppState>,
    Path(organization): Path<Uuid>,
    Query(query): Query<ExportAuditLogsQuery>,
    AuthUser(claims): AuthUser,
) -> impl IntoResponse {
    let user = match Uuid::parse_str(&claims.sub) {
        Ok(user) => user,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new(UNAUTHENTICATED)),
            )
                .into_response();
        }
    };

    let admin = match organization_members::is_admin(state.db(), organization, user).await {
        Ok(admin) => admin,
        Err(error) => {
            tracing::error!(%error, "Failed to check admin status");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(ADMIN_CHECK_FAILED)),
            )
                .into_response();
        }
    };

    if !admin {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(ADMIN_REQUIRED)),
        )
            .into_response();
    }

    let Some(start_date) = parse_start(&query.start_date) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(START_DATE_INVALID)),
        )
            .into_response();
    };

    let Some(end_date) = parse_end(&query.end_date) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(END_DATE_INVALID)),
        )
            .into_response();
    };

    match audit::export_audit_logs_csv(state.db(), organization, start_date, end_date).await {
        Ok(csv) => (
            StatusCode::OK,
            [
                ("Content-Type", "text/csv"),
                (
                    "Content-Disposition",
                    &format!("attachment; filename=\"audit-logs-{organization}.csv\""),
                ),
            ],
            csv,
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "Failed to export audit logs");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to export audit logs")),
            )
                .into_response()
        }
    }
}
