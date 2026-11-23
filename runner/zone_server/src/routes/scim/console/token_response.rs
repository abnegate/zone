use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::scim::TokenRecord;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenResponse {
    pub id: Uuid,
    pub name: String,
    pub hint: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<TokenRecord> for TokenResponse {
    fn from(record: TokenRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            hint: record.hint,
            created_by: record.created_by,
            created_at: record.created_at,
            rotated_at: record.rotated_at,
            last_used_at: record.last_used_at,
        }
    }
}
