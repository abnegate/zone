use abnegate_secret::SecretValue;
use serde_json::Value;
use uuid::Uuid;

use crate::db::audit::scope::Scope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDestination {
    pub scope: Scope,
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    pub settings: Value,
    pub secrets: SecretValue,
    pub created_by: Uuid,
}
