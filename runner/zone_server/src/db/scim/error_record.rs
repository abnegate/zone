use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorRecord {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub token_id: Option<Uuid>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub scim_type: Option<String>,
    pub detail: String,
    pub occurred_at: DateTime<Utc>,
}
