//! The permission catalogue: every capability the server checks.
//!
//! Server permissions are granted through global roles and carried in claims.
//! Organization and workspace permissions are granted through memberships,
//! either by a built-in role or by a custom role.

use serde::{Serialize, Serializer};
use sqlx::PgPool;
use uuid::Uuid;

use super::scope::Scope;
use crate::db::{DbResult, users};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    ModelsCreate,
    ModelsDelete,
    PullRequestsUpdate,
    ReleasesCreate,
    OrganizationRead,
    OrganizationUpdate,
    OrganizationDelete,
    MembersCreate,
    MembersUpdate,
    MembersDelete,
    AdministratorsUpdate,
    InvitationsCreate,
    InvitationsRead,
    InvitationsDelete,
    WorkspacesCreate,
    DevicesRead,
    DevicesUpdate,
    GitHubUpdate,
    McpServersUpdate,
    AiSettingsRead,
    AiSettingsUpdate,
    AgentLoginsUpdate,
    PoliciesUpdate,
    AuditLogsRead,
    AuditUpdate,
    SpendRead,
    SpendUpdate,
    RolesUpdate,
    WorkspaceRead,
    WorkspaceWrite,
    WorkspaceUpdate,
    WorkspaceDelete,
    WorkspaceMembersCreate,
    WorkspaceMembersUpdate,
    WorkspaceMembersDelete,
    WorkspaceAdministratorsUpdate,
    WorkspaceAiSettingsUpdate,
    WorkspaceMcpServersUpdate,
    WorkspacePoliciesUpdate,
    ProjectSyncsUpdate,
    PullRequestsOverride,
}

const ORGANIZATION_ADMIN_REQUIRED: &str = "Organization admin access required";
const WORKSPACE_ADMIN_REQUIRED: &str = "Workspace admin access required";
pub const SPEND_REFUSED: &str =
    "Only organization owners and admins can see spend or change budgets";

/// Each pair is a permission and the one a role must hold alongside it.
const REQUIREMENTS: [(Permission, Permission); 1] =
    [(Permission::SpendUpdate, Permission::SpendRead)];

impl Permission {
    pub const ALL: [Self; 41] = [
        Self::ModelsCreate,
        Self::ModelsDelete,
        Self::PullRequestsUpdate,
        Self::ReleasesCreate,
        Self::OrganizationRead,
        Self::OrganizationUpdate,
        Self::OrganizationDelete,
        Self::MembersCreate,
        Self::MembersUpdate,
        Self::MembersDelete,
        Self::AdministratorsUpdate,
        Self::InvitationsCreate,
        Self::InvitationsRead,
        Self::InvitationsDelete,
        Self::WorkspacesCreate,
        Self::DevicesRead,
        Self::DevicesUpdate,
        Self::GitHubUpdate,
        Self::McpServersUpdate,
        Self::AiSettingsRead,
        Self::AiSettingsUpdate,
        Self::AgentLoginsUpdate,
        Self::PoliciesUpdate,
        Self::AuditLogsRead,
        Self::AuditUpdate,
        Self::SpendRead,
        Self::SpendUpdate,
        Self::RolesUpdate,
        Self::WorkspaceRead,
        Self::WorkspaceWrite,
        Self::WorkspaceUpdate,
        Self::WorkspaceDelete,
        Self::WorkspaceMembersCreate,
        Self::WorkspaceMembersUpdate,
        Self::WorkspaceMembersDelete,
        Self::WorkspaceAdministratorsUpdate,
        Self::WorkspaceAiSettingsUpdate,
        Self::WorkspaceMcpServersUpdate,
        Self::WorkspacePoliciesUpdate,
        Self::ProjectSyncsUpdate,
        Self::PullRequestsOverride,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::ModelsCreate => "models:create",
            Self::ModelsDelete => "models:delete",
            Self::PullRequestsUpdate => "pull_requests:update",
            Self::ReleasesCreate => "releases:create",
            Self::OrganizationRead => "organization:read",
            Self::OrganizationUpdate => "organization:update",
            Self::OrganizationDelete => "organization:delete",
            Self::MembersCreate => "members:create",
            Self::MembersUpdate => "members:update",
            Self::MembersDelete => "members:delete",
            Self::AdministratorsUpdate => "administrators:update",
            Self::InvitationsCreate => "invitations:create",
            Self::InvitationsRead => "invitations:read",
            Self::InvitationsDelete => "invitations:delete",
            Self::WorkspacesCreate => "workspaces:create",
            Self::DevicesRead => "devices:read",
            Self::DevicesUpdate => "devices:update",
            Self::GitHubUpdate => "github:update",
            Self::McpServersUpdate => "mcp_servers:update",
            Self::AiSettingsRead => "ai_settings:read",
            Self::AiSettingsUpdate => "ai_settings:update",
            Self::AgentLoginsUpdate => "agent_logins:update",
            Self::PoliciesUpdate => "policies:update",
            Self::AuditLogsRead => "audit_logs:read",
            Self::AuditUpdate => "audit:update",
            Self::SpendRead => "spend:read",
            Self::SpendUpdate => "spend:update",
            Self::RolesUpdate => "roles:update",
            Self::WorkspaceRead => "workspace:read",
            Self::WorkspaceWrite => "workspace:write",
            Self::WorkspaceUpdate => "workspace:update",
            Self::WorkspaceDelete => "workspace:delete",
            Self::WorkspaceMembersCreate => "workspace_members:create",
            Self::WorkspaceMembersUpdate => "workspace_members:update",
            Self::WorkspaceMembersDelete => "workspace_members:delete",
            Self::WorkspaceAdministratorsUpdate => "workspace_administrators:update",
            Self::WorkspaceAiSettingsUpdate => "workspace_ai_settings:update",
            Self::WorkspaceMcpServersUpdate => "workspace_mcp_servers:update",
            Self::WorkspacePoliciesUpdate => "workspace_policies:update",
            Self::ProjectSyncsUpdate => "project_syncs:update",
            Self::PullRequestsOverride => "pull_requests:override",
        }
    }

    pub const fn scope(self) -> Scope {
        match self {
            Self::ModelsCreate
            | Self::ModelsDelete
            | Self::PullRequestsUpdate
            | Self::ReleasesCreate => Scope::Server,
            Self::OrganizationRead
            | Self::OrganizationUpdate
            | Self::OrganizationDelete
            | Self::MembersCreate
            | Self::MembersUpdate
            | Self::MembersDelete
            | Self::AdministratorsUpdate
            | Self::InvitationsCreate
            | Self::InvitationsRead
            | Self::InvitationsDelete
            | Self::WorkspacesCreate
            | Self::DevicesRead
            | Self::DevicesUpdate
            | Self::GitHubUpdate
            | Self::McpServersUpdate
            | Self::AiSettingsRead
            | Self::AiSettingsUpdate
            | Self::AgentLoginsUpdate
            | Self::PoliciesUpdate
            | Self::AuditLogsRead
            | Self::AuditUpdate
            | Self::SpendRead
            | Self::SpendUpdate
            | Self::RolesUpdate => Scope::Organization,
            Self::WorkspaceRead
            | Self::WorkspaceWrite
            | Self::WorkspaceUpdate
            | Self::WorkspaceDelete
            | Self::WorkspaceMembersCreate
            | Self::WorkspaceMembersUpdate
            | Self::WorkspaceMembersDelete
            | Self::WorkspaceAdministratorsUpdate
            | Self::WorkspaceAiSettingsUpdate
            | Self::WorkspaceMcpServersUpdate
            | Self::WorkspacePoliciesUpdate
            | Self::ProjectSyncsUpdate
            | Self::PullRequestsOverride => Scope::Workspace,
        }
    }

    /// Whether a custom role may carry this permission. The scope bases are
    /// not composable: every custom role holds its scope's base implicitly.
    pub const fn composable(self) -> bool {
        !matches!(
            self,
            Self::ModelsCreate
                | Self::ModelsDelete
                | Self::PullRequestsUpdate
                | Self::ReleasesCreate
                | Self::OrganizationRead
                | Self::OrganizationDelete
                | Self::AdministratorsUpdate
                | Self::RolesUpdate
                | Self::WorkspaceRead
                | Self::WorkspaceDelete
                | Self::WorkspaceAdministratorsUpdate
        )
    }

    /// The permission a custom role must also hold to carry this one.
    pub fn requires(self) -> Option<Self> {
        REQUIREMENTS
            .into_iter()
            .find(|(permission, _)| *permission == self)
            .map(|(_, needs)| needs)
    }

    pub const fn index(self) -> u16 {
        self as u16
    }

    /// Out of range is a programming error: it panics, and at compile time
    /// wherever it is evaluated in a const context.
    pub const fn at(index: u16) -> Self {
        Self::ALL[index as usize]
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|permission| permission.name() == name)
    }

    pub fn refusal(self) -> String {
        match self {
            Self::ModelsCreate => {
                self.server_refusal("Installing or training models on this server")
            }
            Self::ModelsDelete => self.server_refusal("Removing models from this server"),
            Self::PullRequestsUpdate => self.server_refusal("Merging pull requests"),
            Self::ReleasesCreate => self.server_refusal("Creating releases"),
            Self::OrganizationRead => "Not a member of this organization".to_string(),
            Self::OrganizationUpdate
            | Self::MembersCreate
            | Self::MembersUpdate
            | Self::MembersDelete
            | Self::InvitationsCreate
            | Self::InvitationsRead
            | Self::InvitationsDelete
            | Self::WorkspacesCreate
            | Self::DevicesRead
            | Self::DevicesUpdate
            | Self::GitHubUpdate => ORGANIZATION_ADMIN_REQUIRED.to_string(),
            Self::OrganizationDelete => "Organization owner access required".to_string(),
            Self::SpendRead | Self::SpendUpdate => SPEND_REFUSED.to_string(),
            Self::WorkspaceRead => "Not a member of this workspace".to_string(),
            Self::WorkspaceWrite => "Workspace write access required".to_string(),
            Self::WorkspaceUpdate
            | Self::WorkspaceMembersCreate
            | Self::WorkspaceMembersUpdate
            | Self::WorkspaceMembersDelete => WORKSPACE_ADMIN_REQUIRED.to_string(),
            Self::WorkspaceDelete => "Workspace owner access required".to_string(),
            Self::AdministratorsUpdate
            | Self::McpServersUpdate
            | Self::AiSettingsRead
            | Self::AiSettingsUpdate
            | Self::AgentLoginsUpdate
            | Self::PoliciesUpdate
            | Self::AuditLogsRead
            | Self::AuditUpdate
            | Self::RolesUpdate => format!(
                "This needs the {} permission in this organization.",
                self.name()
            ),
            Self::WorkspaceAdministratorsUpdate
            | Self::WorkspaceAiSettingsUpdate
            | Self::WorkspaceMcpServersUpdate
            | Self::WorkspacePoliciesUpdate
            | Self::ProjectSyncsUpdate
            | Self::PullRequestsOverride => format!(
                "This needs the {} permission in this workspace.",
                self.name()
            ),
        }
    }

    fn server_refusal(self, action: &str) -> String {
        format!(
            "{action} requires the {} permission. Ask a server admin for access.",
            self.name()
        )
    }
}

impl Serialize for Permission {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// Reads the grant from the database rather than a token, for callers such as
/// agent tools that act for a user without holding their claims. Only server
/// permissions live in global roles, so anything else is refused.
pub async fn holds(pool: &PgPool, user_id: Uuid, permission: Permission) -> DbResult<bool> {
    if !matches!(permission.scope(), Scope::Server) {
        return Ok(false);
    }
    Ok(users::get_user_with_permissions(pool, user_id)
        .await?
        .is_some_and(|user| {
            user.user.is_admin.unwrap_or(false)
                || user
                    .permissions
                    .iter()
                    .any(|name| name == permission.name())
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_seeded_permissions() {
        assert_eq!(Permission::ModelsCreate.name(), "models:create");
        assert_eq!(Permission::ModelsDelete.name(), "models:delete");
        assert_eq!(
            Permission::PullRequestsUpdate.name(),
            "pull_requests:update"
        );
        assert_eq!(Permission::ReleasesCreate.name(), "releases:create");
    }

    #[test]
    fn refusal_names_the_missing_permission() {
        let refusal = Permission::ModelsCreate.refusal();
        assert!(refusal.contains("models:create"), "{refusal}");
        assert!(refusal.contains("server admin"), "{refusal}");
        assert!(Permission::ModelsDelete.refusal().contains("models:delete"));
    }

    #[test]
    fn serializes_as_its_name() {
        assert_eq!(
            serde_json::to_string(&Permission::WorkspaceWrite).expect("a permission serializes"),
            "\"workspace:write\""
        );
    }
}
