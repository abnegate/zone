use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{Grant, Scope};
use crate::db::custom_roles::CustomRoleRow;

#[derive(Debug, Clone, Serialize)]
pub struct Role {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub scope: Scope,
    pub permissions: Grant,
    pub members: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<CustomRoleRow> for Role {
    fn from(row: CustomRoleRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            name: row.name,
            description: row.description,
            scope: row.scope,
            permissions: row.grant,
            members: row.members,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
