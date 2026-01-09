//! Audit logging database operations
//!
//! This module provides functionality for logging and querying audit events
//! across the system. Audit logs track important actions like user logins,
//! permission changes, and resource modifications.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// An audit log entry representing a tracked action
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLog {
    pub id: Uuid,
    pub organization_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_values: Option<serde_json::Value>,
    pub new_values: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Context information for an audit log entry
#[derive(Debug, Clone)]
pub struct AuditContext {
    pub org_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// Filters for querying audit logs
#[derive(Debug, Clone, Default)]
pub struct AuditFilters {
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

/// Standard audit action types
pub mod actions {
    pub const USER_LOGIN: &str = "user.login";
    pub const USER_LOGOUT: &str = "user.logout";
    pub const USER_CREATED: &str = "user.created";
    pub const USER_UPDATED: &str = "user.updated";
    pub const USER_DELETED: &str = "user.deleted";

    pub const MEMBER_ADDED: &str = "member.added";
    pub const MEMBER_REMOVED: &str = "member.removed";
    pub const MEMBER_ROLE_CHANGED: &str = "member.role_changed";

    pub const WORKSPACE_CREATED: &str = "workspace.created";
    pub const WORKSPACE_UPDATED: &str = "workspace.updated";
    pub const WORKSPACE_DELETED: &str = "workspace.deleted";

    pub const PROJECT_CREATED: &str = "project.created";
    pub const PROJECT_UPDATED: &str = "project.updated";
    pub const PROJECT_DELETED: &str = "project.deleted";

    pub const SETTINGS_UPDATED: &str = "settings.updated";

    pub const INVITATION_SENT: &str = "invitation.sent";
    pub const INVITATION_ACCEPTED: &str = "invitation.accepted";
    pub const INVITATION_REVOKED: &str = "invitation.revoked";

    pub const SUBSCRIPTION_CHANGED: &str = "subscription.changed";
}

/// Log an action to the audit trail
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `ctx` - Audit context containing actor and request information
/// * `action` - The action being performed (e.g., "user.login")
/// * `resource_type` - The type of resource being acted upon
/// * `resource_id` - Optional ID of the specific resource
/// * `old_values` - Optional JSON of values before the change
/// * `new_values` - Optional JSON of values after the change
///
/// # Returns
/// The UUID of the created audit log entry
pub async fn log_action(
    pool: &PgPool,
    ctx: &AuditContext,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    old_values: Option<serde_json::Value>,
    new_values: Option<serde_json::Value>,
) -> DbResult<Uuid> {
    // Parse IP address to std::net::IpAddr, then convert to sqlx IpNetwork
    let ip_network: Option<sqlx::types::ipnetwork::IpNetwork> = ctx
        .ip_address
        .as_deref()
        .and_then(|ip_str| ip_str.parse::<std::net::IpAddr>().ok())
        .map(sqlx::types::ipnetwork::IpNetwork::from);

    let record: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO audit_logs (
            organization_id,
            workspace_id,
            actor_id,
            actor_email,
            action,
            resource_type,
            resource_id,
            old_values,
            new_values,
            ip_address,
            user_agent
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id
        "#,
    )
    .bind(ctx.org_id)
    .bind(ctx.workspace_id)
    .bind(ctx.actor_id)
    .bind(&ctx.actor_email)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(old_values)
    .bind(new_values)
    .bind(ip_network)
    .bind(&ctx.user_agent)
    .fetch_one(pool)
    .await?;

    Ok(record.0)
}

/// List audit logs for an organization with optional filters
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `org_id` - Organization ID to filter logs by
/// * `filters` - Additional filters to apply
/// * `limit` - Maximum number of logs to return
/// * `offset` - Number of logs to skip for pagination
///
/// # Returns
/// Vector of audit log entries, ordered by created_at DESC
pub async fn list_audit_logs(
    pool: &PgPool,
    org_id: Uuid,
    filters: &AuditFilters,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<AuditLog>> {
    let mut query = String::from(
        r#"
        SELECT
            id,
            organization_id,
            workspace_id,
            actor_id,
            actor_email,
            action,
            resource_type,
            resource_id,
            old_values,
            new_values,
            ip_address::text as ip_address,
            user_agent,
            created_at
        FROM audit_logs
        WHERE organization_id = $1
        "#,
    );

    let mut param_count = 1;
    let mut conditions = Vec::new();

    if filters.action.is_some() {
        param_count += 1;
        conditions.push(format!("action = ${}", param_count));
    }

    if filters.resource_type.is_some() {
        param_count += 1;
        conditions.push(format!("resource_type = ${}", param_count));
    }

    if filters.resource_id.is_some() {
        param_count += 1;
        conditions.push(format!("resource_id = ${}", param_count));
    }

    if filters.actor_id.is_some() {
        param_count += 1;
        conditions.push(format!("actor_id = ${}", param_count));
    }

    if filters.start_date.is_some() {
        param_count += 1;
        conditions.push(format!("created_at >= ${}", param_count));
    }

    if filters.end_date.is_some() {
        param_count += 1;
        conditions.push(format!("created_at <= ${}", param_count));
    }

    if !conditions.is_empty() {
        query.push_str(" AND ");
        query.push_str(&conditions.join(" AND "));
    }

    query.push_str(" ORDER BY created_at DESC");
    param_count += 1;
    query.push_str(&format!(" LIMIT ${}", param_count));
    param_count += 1;
    query.push_str(&format!(" OFFSET ${}", param_count));

    let mut query_builder = sqlx::query_as::<_, AuditLog>(&query);
    query_builder = query_builder.bind(org_id);

    if let Some(ref action) = filters.action {
        query_builder = query_builder.bind(action);
    }
    if let Some(ref resource_type) = filters.resource_type {
        query_builder = query_builder.bind(resource_type);
    }
    if let Some(resource_id) = filters.resource_id {
        query_builder = query_builder.bind(resource_id);
    }
    if let Some(actor_id) = filters.actor_id {
        query_builder = query_builder.bind(actor_id);
    }
    if let Some(start_date) = filters.start_date {
        query_builder = query_builder.bind(start_date);
    }
    if let Some(end_date) = filters.end_date {
        query_builder = query_builder.bind(end_date);
    }

    query_builder = query_builder.bind(limit);
    query_builder = query_builder.bind(offset);

    let logs = query_builder.fetch_all(pool).await?;

    Ok(logs)
}

/// Get a single audit log by ID
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `log_id` - UUID of the audit log to retrieve
///
/// # Returns
/// The audit log if found, None otherwise
pub async fn get_audit_log(pool: &PgPool, log_id: Uuid) -> DbResult<Option<AuditLog>> {
    let log = sqlx::query_as::<_, AuditLog>(
        r#"
        SELECT
            id,
            organization_id,
            workspace_id,
            actor_id,
            actor_email,
            action,
            resource_type,
            resource_id,
            old_values,
            new_values,
            ip_address::text as ip_address,
            user_agent,
            created_at
        FROM audit_logs
        WHERE id = $1
        "#,
    )
    .bind(log_id)
    .fetch_optional(pool)
    .await?;

    Ok(log)
}

/// Count audit logs matching the given filters
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `org_id` - Organization ID to filter logs by
/// * `filters` - Additional filters to apply
///
/// # Returns
/// The total count of matching audit logs
pub async fn count_audit_logs(
    pool: &PgPool,
    org_id: Uuid,
    filters: &AuditFilters,
) -> DbResult<i64> {
    let mut query = String::from(
        r#"
        SELECT COUNT(*)
        FROM audit_logs
        WHERE organization_id = $1
        "#,
    );

    let mut param_count = 1;
    let mut conditions = Vec::new();

    if filters.action.is_some() {
        param_count += 1;
        conditions.push(format!("action = ${}", param_count));
    }

    if filters.resource_type.is_some() {
        param_count += 1;
        conditions.push(format!("resource_type = ${}", param_count));
    }

    if filters.resource_id.is_some() {
        param_count += 1;
        conditions.push(format!("resource_id = ${}", param_count));
    }

    if filters.actor_id.is_some() {
        param_count += 1;
        conditions.push(format!("actor_id = ${}", param_count));
    }

    if filters.start_date.is_some() {
        param_count += 1;
        conditions.push(format!("created_at >= ${}", param_count));
    }

    if filters.end_date.is_some() {
        param_count += 1;
        conditions.push(format!("created_at <= ${}", param_count));
    }

    if !conditions.is_empty() {
        query.push_str(" AND ");
        query.push_str(&conditions.join(" AND "));
    }

    let mut query_builder = sqlx::query_scalar::<_, i64>(&query);
    query_builder = query_builder.bind(org_id);

    if let Some(ref action) = filters.action {
        query_builder = query_builder.bind(action);
    }
    if let Some(ref resource_type) = filters.resource_type {
        query_builder = query_builder.bind(resource_type);
    }
    if let Some(resource_id) = filters.resource_id {
        query_builder = query_builder.bind(resource_id);
    }
    if let Some(actor_id) = filters.actor_id {
        query_builder = query_builder.bind(actor_id);
    }
    if let Some(start_date) = filters.start_date {
        query_builder = query_builder.bind(start_date);
    }
    if let Some(end_date) = filters.end_date {
        query_builder = query_builder.bind(end_date);
    }

    let count = query_builder.fetch_one(pool).await?;

    Ok(count)
}

/// Escapes a CSV field to prevent CSV injection attacks
///
/// This function protects against formula injection by:
/// 1. Wrapping fields in quotes if they contain special characters
/// 2. Escaping existing quotes by doubling them
/// 3. Prefixing formula characters (=, +, -, @) with a single quote
fn escape_csv_field(field: &str) -> String {
    // Check if field starts with formula characters
    let needs_formula_escape = field.starts_with('=')
        || field.starts_with('+')
        || field.starts_with('-')
        || field.starts_with('@');

    // Escape the field by wrapping in quotes if it contains special chars

    if field.contains(',') || field.contains('"') || field.contains('\n') || needs_formula_escape {
        // If it starts with a formula character, prefix with single quote to neutralize
        let safe_field = if needs_formula_escape {
            format!("'{}", field)
        } else {
            field.to_string()
        };
        // Escape quotes by doubling them and wrap in quotes
        format!("\"{}\"", safe_field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Export audit logs as CSV format
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `org_id` - Organization ID to export logs for
/// * `start_date` - Start date for the export range
/// * `end_date` - End date for the export range
///
/// # Returns
/// CSV string containing the audit logs
pub async fn export_audit_logs_csv(
    pool: &PgPool,
    org_id: Uuid,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
) -> DbResult<String> {
    let filters = AuditFilters {
        start_date: Some(start_date),
        end_date: Some(end_date),
        ..Default::default()
    };

    let logs = list_audit_logs(pool, org_id, &filters, 10000, 0).await?;

    let mut csv = String::from(
        "id,organization_id,workspace_id,actor_id,actor_email,action,resource_type,resource_id,old_values,new_values,ip_address,user_agent,created_at\n",
    );

    for log in logs {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            log.id,
            log.organization_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            log.workspace_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            log.actor_id.map(|id| id.to_string()).unwrap_or_default(),
            escape_csv_field(log.actor_email.as_deref().unwrap_or("")),
            escape_csv_field(&log.action),
            escape_csv_field(&log.resource_type),
            log.resource_id.map(|id| id.to_string()).unwrap_or_default(),
            escape_csv_field(
                &log.old_values
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            ),
            escape_csv_field(
                &log.new_values
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            ),
            escape_csv_field(log.ip_address.as_deref().unwrap_or("")),
            escape_csv_field(log.user_agent.as_deref().unwrap_or("")),
            log.created_at.to_rfc3339(),
        ));
    }

    Ok(csv)
}
