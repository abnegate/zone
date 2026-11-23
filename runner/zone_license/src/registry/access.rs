use serde::Serialize;

use crate::registry::action::Action;
use crate::registry::resource_type::ResourceType;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Access {
    #[serde(rename = "type")]
    pub kind: ResourceType,
    pub name: String,
    pub actions: Vec<Action>,
}
