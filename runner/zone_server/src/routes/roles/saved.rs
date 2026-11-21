use serde::Serialize;

use super::role::Role;

/// The answer to creating or editing a role.
#[derive(Debug, Serialize)]
pub struct Saved {
    pub role: Role,
}
