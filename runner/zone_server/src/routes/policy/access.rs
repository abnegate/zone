use sqlx::PgConnection;
use uuid::Uuid;

use super::refusal::Refusal;
use crate::auth::Permission;
use crate::db::organization_members;
use crate::db::workspace_members;
use crate::db::workspaces;

const ORGANIZATION_MISSING: &str = "Organization not found";
const WORKSPACE_MISSING: &str = "Workspace not found";

/// Every caller who may see a layer may read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

impl Access {
    pub fn writable(self) -> bool {
        self == Self::Write
    }

    pub fn write(self, needed: Permission) -> Result<(), Refusal> {
        match self {
            Self::Write => Ok(()),
            Self::Read => Err(Refusal::Forbidden(needed)),
        }
    }
}

/// The organization's own policy: every active member reads it, and whoever holds the policies
/// permission changes it. Someone outside the organization is told it doesn't exist.
pub async fn organization(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
) -> Result<Access, Refusal> {
    let membership = organization_members::lock_membership(connection, organization, user).await?;
    match membership.map(|membership| membership.grant) {
        Some(grant) if grant.allows(Permission::PoliciesUpdate) => Ok(Access::Write),
        Some(grant) if grant.allows(Permission::OrganizationRead) => Ok(Access::Read),
        _ => Err(Refusal::Missing(ORGANIZATION_MISSING)),
    }
}

/// A workspace's policy: its own members read it and whoever holds its policies permission changes
/// it, and so does whoever holds the policies permission of the organization it belongs to. The
/// workspace is locked to the organization first, so a nested path never mixes two tenants'
/// memberships.
pub async fn workspace(
    connection: &mut PgConnection,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
) -> Result<Access, Refusal> {
    if !workspaces::lock_owned(connection, organization, workspace).await? {
        return Err(Refusal::Missing(WORKSPACE_MISSING));
    }
    if self::organization(connection, organization, user).await? == Access::Write {
        return Ok(Access::Write);
    }
    let membership = workspace_members::lock_membership(connection, workspace, user).await?;
    match membership.map(|membership| membership.grant) {
        Some(grant) if grant.allows(Permission::WorkspacePoliciesUpdate) => Ok(Access::Write),
        Some(grant) if grant.allows(Permission::WorkspaceRead) => Ok(Access::Read),
        _ => Err(Refusal::Missing(WORKSPACE_MISSING)),
    }
}

#[cfg(test)]
mod tests;
