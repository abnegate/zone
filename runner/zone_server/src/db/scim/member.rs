use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::group_reference::GroupReference;
use super::link::Link;
use crate::db::organization_members::OrgRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub active: bool,
    pub role: OrgRole,
    pub link: Option<Link>,
    pub groups: Vec<GroupReference>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
