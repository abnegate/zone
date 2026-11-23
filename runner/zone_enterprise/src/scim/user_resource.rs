use serde::Serialize;
use uuid::Uuid;

use crate::scim::email::Email;
use crate::scim::member_reference::MemberReference;
use crate::scim::meta::Meta;
use crate::scim::name::Name;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResource {
    pub schemas: Vec<&'static str>,
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub user_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Name>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub emails: Vec<Email>,
    pub active: bool,
    pub groups: Vec<MemberReference>,
    pub meta: Meta,
}
