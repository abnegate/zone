//! Where a permission applies: the whole server, one organization, or one
//! workspace.

use serde::{Deserialize, Serialize};

use super::permission::Permission;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Server,
    Organization,
    Workspace,
}

impl Scope {
    pub const ALL: [Self; 3] = [Self::Server, Self::Organization, Self::Workspace];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Organization => "organization",
            Self::Workspace => "workspace",
        }
    }

    /// The permission every membership of this scope holds, custom role or not.
    pub const fn base(self) -> Option<Permission> {
        match self {
            Self::Server => None,
            Self::Organization => Some(Permission::OrganizationRead),
            Self::Workspace => Some(Permission::WorkspaceRead),
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| scope.name() == name)
    }
}
