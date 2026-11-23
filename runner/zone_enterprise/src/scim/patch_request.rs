use serde::Deserialize;

use crate::scim::operation::Operation;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PatchRequest {
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(rename = "Operations", alias = "operations")]
    pub operations: Vec<Operation>,
}
