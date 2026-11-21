use serde::Serialize;

use crate::auth::{Permission, Scope};

/// A permission a custom role may be composed from, as the catalogue lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: Permission,
    pub scope: Scope,
    pub composable: bool,
}

impl Entry {
    /// Every organization and workspace permission. Server permissions never go in a role.
    pub fn catalogue() -> Vec<Self> {
        Permission::ALL
            .into_iter()
            .filter(|permission| permission.scope() != Scope::Server)
            .map(|permission| Self {
                name: permission,
                scope: permission.scope(),
                composable: permission.composable(),
            })
            .collect()
    }
}
