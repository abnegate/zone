use serde::Deserialize;
use uuid::Uuid;

/// What a group maps to: exactly one of a built-in role or a custom role.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingRequest {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub custom_role_id: Option<Uuid>,
}
