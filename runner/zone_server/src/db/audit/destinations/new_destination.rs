use abnegate_secret::SecretValue;
use serde_json::Value;
use uuid::Uuid;
use zone_enterprise::audit::Kind;

use crate::db::audit::scope::Scope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDestination {
    pub scope: Scope,
    pub kind: Kind,
    pub name: String,
    pub enabled: bool,
    pub settings: Value,
    pub secrets: SecretValue,
    pub created_by: Uuid,
}
