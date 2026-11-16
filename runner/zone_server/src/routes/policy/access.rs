use sqlx::PgConnection;
use uuid::Uuid;

use super::refusal::Refusal;
use crate::db::organization_members::{self, OrgRole};
use crate::db::workspace_members::{self, WorkspaceRole};
use crate::db::workspaces;

const ORGANIZATION_MISSING: &str = "Organization not found";
const WORKSPACE_MISSING: &str = "Workspace not found";

/// What a caller may do with a policy layer. Every caller who may see a layer may read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

impl Access {
    pub fn writable(self) -> bool {
        self == Self::Write
    }

    /// Refuses a change unless the caller may write the layer.
    pub fn write(self, refused: &'static str) -> Result<(), Refusal> {
        match self {
            Self::Write => Ok(()),
            Self::Read => Err(Refusal::Forbidden(refused)),
        }
    }
}

/// The organization's own policy: every active member reads it, and its owners and admins change
/// it. Someone outside the organization is told it doesn't exist.
pub async fn organization(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
) -> Result<Access, Refusal> {
    match organization_members::lock_role(connection, organization, user).await? {
        Some(OrgRole::Owner | OrgRole::Admin) => Ok(Access::Write),
        Some(OrgRole::Member) => Ok(Access::Read),
        None => Err(Refusal::Missing(ORGANIZATION_MISSING)),
    }
}

/// A workspace's policy: its own members read it and its owners and admins change it, and so do
/// the owners and admins of the organization it belongs to. The workspace is locked to the
/// organization first, so a nested path never mixes two tenants' memberships.
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
    match workspace_members::lock_role(connection, workspace, user).await? {
        Some(WorkspaceRole::Owner | WorkspaceRole::Admin) => Ok(Access::Write),
        Some(WorkspaceRole::Member | WorkspaceRole::Viewer) => Ok(Access::Read),
        None => Err(Refusal::Missing(WORKSPACE_MISSING)),
    }
}
