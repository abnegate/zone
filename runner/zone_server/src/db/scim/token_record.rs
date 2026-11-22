use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TokenRecord {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub hint: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}
