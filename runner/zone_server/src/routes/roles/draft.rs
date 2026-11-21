use serde::Deserialize;

use crate::auth::Scope;

/// A new custom role. The scope's base permission is added whether named or not.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub scope: Scope,
    pub permissions: Vec<String>,
}
