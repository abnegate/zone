//! Common response types and utilities for API routes

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::audit::{AuditContext, log_action};
use crate::db::workspaces;

/// Macro to define a response struct with automatic timestamps.
///
/// This macro generates a struct with the specified fields plus a flattened
/// `Timestamps` field that provides `created_at` and `updated_at`.
///
/// # Example
/// ```ignore
/// response_struct! {
///     /// My response documentation
///     pub struct MyResponse {
///         pub id: Uuid,
///         pub name: String,
///     }
/// }
/// ```
///
/// Expands to:
/// ```ignore
/// #[derive(Debug, Serialize)]
/// pub struct MyResponse {
///     pub id: Uuid,
///     pub name: String,
///     #[serde(flatten)]
///     pub timestamps: Timestamps,
/// }
/// ```
#[macro_export]
macro_rules! response_struct {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, serde::Serialize)]
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $ty,
            )*
            #[serde(flatten)]
            pub timestamps: $crate::routes::common::Timestamps,
        }
    };
}

pub use response_struct;

/// Base timestamps that should be included in all entity responses.
/// Use `#[serde(flatten)]` to embed these fields in your response structs.
///
/// Includes `deleted_at` for soft delete support - only serialized when present.
///
/// # Example
/// ```ignore
/// #[derive(Serialize)]
/// struct MyResponse {
///     id: Uuid,
///     name: String,
///     #[serde(flatten)]
///     timestamps: Timestamps,
/// }
/// ```
#[derive(Debug, Clone, Serialize, Default)]
pub struct Timestamps {
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl Timestamps {
    /// Create timestamps from optional NaiveDateTime values
    pub fn from_naive(
        created_at: Option<NaiveDateTime>,
        updated_at: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            created_at: created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            updated_at: updated_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            deleted_at: None,
        }
    }

    /// Create timestamps from optional NaiveDateTime values including deleted_at
    pub fn from_naive_with_deleted(
        created_at: Option<NaiveDateTime>,
        updated_at: Option<NaiveDateTime>,
        deleted_at: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            created_at: created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            updated_at: updated_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            deleted_at: deleted_at.map(|dt| dt.and_utc().to_rfc3339()),
        }
    }

    /// Create timestamps from DateTime<Utc> values
    pub fn from_utc(created_at: DateTime<Utc>, updated_at: DateTime<Utc>) -> Self {
        Self {
            created_at: created_at.to_rfc3339(),
            updated_at: updated_at.to_rfc3339(),
            deleted_at: None,
        }
    }

    /// Create timestamps from DateTime<Utc> values including deleted_at
    pub fn from_utc_with_deleted(
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            created_at: created_at.to_rfc3339(),
            updated_at: updated_at.to_rfc3339(),
            deleted_at: deleted_at.map(|dt| dt.to_rfc3339()),
        }
    }

    /// Create timestamps from optional DateTime<Utc> values
    pub fn from_utc_opt(
        created_at: Option<DateTime<Utc>>,
        updated_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            created_at: created_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            updated_at: updated_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            deleted_at: None,
        }
    }

    /// Create timestamps from optional DateTime<Utc> values including deleted_at
    pub fn from_utc_opt_with_deleted(
        created_at: Option<DateTime<Utc>>,
        updated_at: Option<DateTime<Utc>>,
        deleted_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            created_at: created_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            updated_at: updated_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            deleted_at: deleted_at.map(|dt| dt.to_rfc3339()),
        }
    }

    /// Create timestamps with current time for both fields
    pub fn now() -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            created_at: now.clone(),
            updated_at: now,
            deleted_at: None,
        }
    }

    /// Set deleted_at timestamp
    pub fn with_deleted(mut self, deleted_at: Option<DateTime<Utc>>) -> Self {
        self.deleted_at = deleted_at.map(|dt| dt.to_rfc3339());
        self
    }

    /// Set deleted_at timestamp from NaiveDateTime
    pub fn with_deleted_naive(mut self, deleted_at: Option<NaiveDateTime>) -> Self {
        self.deleted_at = deleted_at.map(|dt| dt.and_utc().to_rfc3339());
        self
    }
}

/// Standard error response format
/// One recorded change: who did what to which resource, and in which tenant.
/// Either `organization_id` or `workspace_id` names the tenant; a workspace
/// alone is resolved to its organization when the entry is written.
#[derive(Debug)]
pub struct AuditEvent<'a> {
    pub organization_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub actor_id: Uuid,
    pub actor_email: &'a str,
    pub action: &'a str,
    pub resource_type: &'a str,
    pub resource_id: Option<Uuid>,
    pub old_values: Option<Value>,
    pub new_values: Option<Value>,
}

/// Write an audit entry. A failure to record one is logged and never fails the
/// request that caused it.
pub async fn audit(pool: &PgPool, event: AuditEvent<'_>) {
    let organization_id = match (event.organization_id, event.workspace_id) {
        (Some(organization_id), _) => Some(organization_id),
        (None, Some(workspace_id)) => match workspaces::get_workspace(pool, workspace_id).await {
            Ok(workspace) => workspace.map(|workspace| workspace.organization_id),
            Err(error) => {
                tracing::error!(%error, %workspace_id, "Failed to resolve the workspace's organization for an audit log");
                None
            }
        },
        (None, None) => None,
    };
    let context = AuditContext {
        org_id: organization_id,
        workspace_id: event.workspace_id,
        actor_id: Some(event.actor_id),
        actor_email: Some(event.actor_email.to_string()),
        ip_address: None,
        user_agent: None,
    };
    if let Err(error) = log_action(
        pool,
        &context,
        event.action,
        event.resource_type,
        event.resource_id,
        event.old_values,
        event.new_values,
    )
    .await
    {
        tracing::error!(%error, action = event.action, "Failed to record audit log");
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}
