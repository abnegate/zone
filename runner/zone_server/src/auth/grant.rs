//! The set of permissions a membership holds in its organization or workspace.

use std::collections::BTreeSet;

use serde::{Serialize, Serializer};

use super::{composition::Composition, permission::Permission, scope::Scope};
use crate::db::{organization_members::OrgRole, workspace_members::WorkspaceRole};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Grant(BTreeSet<Permission>);

const ORGANIZATION_RANKS: [OrgRole; 3] = [OrgRole::Owner, OrgRole::Admin, OrgRole::Member];
const WORKSPACE_RANKS: [WorkspaceRole; 4] = [
    WorkspaceRole::Owner,
    WorkspaceRole::Admin,
    WorkspaceRole::Member,
    WorkspaceRole::Viewer,
];

impl Grant {
    pub fn organization(role: OrgRole) -> Self {
        Self(
            Permission::ALL
                .into_iter()
                .filter(|permission| {
                    organization_threshold(*permission).is_some_and(|threshold| role >= threshold)
                })
                .collect(),
        )
    }

    pub fn workspace(role: WorkspaceRole) -> Self {
        Self(
            Permission::ALL
                .into_iter()
                .filter(|permission| {
                    workspace_threshold(*permission).is_some_and(|threshold| role >= threshold)
                })
                .collect(),
        )
    }

    /// Validates names chosen for a custom role of `scope`. The scope's base
    /// may be named or left out; it is always held.
    pub fn compose(scope: Scope, names: &[String]) -> Result<Self, Composition> {
        let mut permissions: BTreeSet<Permission> = scope.base().into_iter().collect();
        for name in names {
            let permission =
                Permission::parse(name).ok_or_else(|| Composition::Unknown(name.clone()))?;
            if permission.scope() != scope {
                return Err(Composition::OutOfScope(permission));
            }
            if Some(permission) != scope.base() && !permission.composable() {
                return Err(Composition::NotComposable(permission));
            }
            permissions.insert(permission);
        }
        if let Some((permission, needs)) = unmet(&permissions) {
            return Err(Composition::Requires { permission, needs });
        }
        Ok(Self(permissions))
    }

    /// Rebuilds a custom role's grant from stored names, dropping any that are
    /// unknown, of another scope, not composable, or without the permission
    /// they require, so a stored row can never grant more than a composed one.
    pub fn stored<I, S>(scope: Scope, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut permissions: BTreeSet<Permission> = names
            .into_iter()
            .filter_map(|name| Permission::parse(name.as_ref()))
            .filter(|permission| permission.scope() == scope && permission.composable())
            .collect();
        permissions.extend(scope.base());
        while let Some((permission, _)) = unmet(&permissions) {
            permissions.remove(&permission);
        }
        Self(permissions)
    }

    pub fn allows(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }

    pub fn covers(&self, other: &Grant) -> bool {
        other.0.is_subset(&self.0)
    }

    pub fn organization_rank(&self) -> OrgRole {
        ORGANIZATION_RANKS
            .into_iter()
            .find(|role| self.covers(&Self::organization(*role)))
            .unwrap_or(OrgRole::Member)
    }

    pub fn workspace_rank(&self) -> WorkspaceRole {
        WORKSPACE_RANKS
            .into_iter()
            .find(|role| self.covers(&Self::workspace(*role)))
            .unwrap_or(WorkspaceRole::Viewer)
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> =
            self.0.iter().map(|permission| permission.name()).collect();
        names.sort_unstable();
        names
    }

    pub fn permissions(&self) -> impl Iterator<Item = Permission> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Grant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.names())
    }
}

fn unmet(permissions: &BTreeSet<Permission>) -> Option<(Permission, Permission)> {
    permissions.iter().find_map(|permission| {
        permission
            .requires()
            .filter(|needs| !permissions.contains(needs))
            .map(|needs| (*permission, needs))
    })
}

const fn organization_threshold(permission: Permission) -> Option<OrgRole> {
    match permission {
        Permission::OrganizationRead => Some(OrgRole::Member),
        Permission::OrganizationDelete | Permission::AdministratorsUpdate => Some(OrgRole::Owner),
        _ if matches!(permission.scope(), Scope::Organization) => Some(OrgRole::Admin),
        _ => None,
    }
}

const fn workspace_threshold(permission: Permission) -> Option<WorkspaceRole> {
    match permission {
        Permission::WorkspaceRead => Some(WorkspaceRole::Viewer),
        Permission::WorkspaceWrite => Some(WorkspaceRole::Member),
        Permission::WorkspaceDelete | Permission::WorkspaceAdministratorsUpdate => {
            Some(WorkspaceRole::Owner)
        }
        _ if matches!(permission.scope(), Scope::Workspace) => Some(WorkspaceRole::Admin),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
