use chrono::{DateTime, Utc};
use serde::Serialize;

pub const USER_RESOURCE: &str = "User";
pub const GROUP_RESOURCE: &str = "Group";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub resource_type: &'static str,
    pub created: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
    pub location: String,
}

impl Meta {
    pub fn user(created: DateTime<Utc>, last_modified: DateTime<Utc>, location: String) -> Self {
        Self {
            resource_type: USER_RESOURCE,
            created,
            last_modified,
            location,
        }
    }

    pub fn group(created: DateTime<Utc>, last_modified: DateTime<Utc>, location: String) -> Self {
        Self {
            resource_type: GROUP_RESOURCE,
            created,
            last_modified,
            location,
        }
    }
}
