use serde::Serialize;
use uuid::Uuid;

use crate::scim::member_reference::MemberReference;
use crate::scim::meta::Meta;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupResource {
    pub schemas: Vec<&'static str>,
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<MemberReference>>,
    pub meta: Meta,
}
