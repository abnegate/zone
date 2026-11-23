use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::custom_roles::Summary;
use crate::db::scim::{GroupRecord, Mapping};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupResponse {
    pub id: Uuid,
    pub display_name: String,
    pub external_id: Option<String>,
    pub members: i64,
    pub role: Option<String>,
    pub custom_role: Option<Summary>,
    pub updated_at: DateTime<Utc>,
}

impl From<GroupRecord> for GroupResponse {
    fn from(record: GroupRecord) -> Self {
        let (role, custom_role) = match record.mapping {
            Mapping::Unmapped => (None, None),
            Mapping::BuiltIn(role) => (Some(role.as_str().to_owned()), None),
            Mapping::Custom(summary) => (None, Some(summary)),
        };
        Self {
            id: record.id,
            display_name: record.display_name,
            external_id: record.external_id,
            members: record.members,
            role,
            custom_role,
            updated_at: record.updated_at,
        }
    }
}
