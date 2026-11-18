//! Agent and model policies for an organization and each of its workspaces. Members read them;
//! changing them takes an administrator and an Enterprise license, and every change is
//! audited.

pub mod access;
pub mod asked;
pub mod organization;
pub mod refusal;
pub mod standing;
pub mod workspace;

use uuid::Uuid;
use zone_enterprise::policy::Rules;

use crate::auth::AuthUser;
use crate::db::policies::Stored;
use refusal::Refusal;

fn caller(auth: &AuthUser) -> Result<Uuid, Refusal> {
    auth.0.user_id().map_err(|_| Refusal::Unauthenticated)
}

fn rules(stored: Option<&Stored>) -> Result<Rules, Refusal> {
    Ok(stored
        .map(|stored| serde_json::from_value(stored.rules.clone()))
        .transpose()?
        .unwrap_or_default())
}

fn serialized(rules: &Rules) -> Result<serde_json::Value, Refusal> {
    Ok(serde_json::to_value(rules)?)
}
