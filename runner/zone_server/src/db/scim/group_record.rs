use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::mapping::Mapping;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRecord {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub external_id: Option<String>,
    pub display_name: String,
    pub mapping: Mapping,
    pub members: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
