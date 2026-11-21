//! Every organization and workspace route, called by a custom-role holder
//! with exactly the permission the route checks and by one without it.
//!
//! A holder with the permission is never refused with 403; a route that only
//! reads answers it outright. A holder of only the base permission is refused
//! with 403. A route no custom role may reach, because it checks a permission
//! that isn't composable, refuses a holder of every composable permission.
//! Each route is called with the smallest body it accepts, and every failing
//! route is reported together.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;
use zone_server::auth::{Permission, Scope};
use zone_server::db::organization_members::OrgRole;
use zone_server::db::projects;
use zone_server::db::workspace_members::WorkspaceRole;

use super::{
    Call, composable, describe, licensed, organization_holder, seat_organization, seat_workspace,
    user, workspace_holder,
};
use crate::common::policy::Fixture;
use crate::common::sso::Account;

const REFUSED_URL: &str = "http://127.0.0.1:1/mcp";

const EXPORTED_RANGE: &str = "start_date=2020-01-01&end_date=2020-12-31";

/// Checked only while merging a GitHub pull request, which these tests have none of.
const UNROUTED: [Permission; 1] = [Permission::PullRequestsOverride];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    ReadOrganization,
    ListMembers,
    ListWorkspaces,
    ReadGitHub,
    ListRepositories,
    ListMcpServers,
    ReadOrganizationPolicy,
    ListAgents,
    ListRoles,
    UpdateOrganization,
    DeleteOrganization,
    AddMember,
    ChangeMember,
    RemoveMember,
    CreateInvitation,
    ListInvitations,
    RevokeInvitation,
    CreateWorkspace,
    ListDevices,
    ReadDevicePolicy,
    UpdateDevice,
    SetDevicePolicy,
    RegisterGitHubApp,
    DeleteGitHubApp,
    InstallGitHubApp,
    SyncGitHubInstallations,
    DeleteGitHubInstallation,
    CreateMcpServer,
    UpdateMcpServer,
    DeleteMcpServer,
    ReadAiSettings,
    UpdateAiSettings,
    ClearAiSettings,
    SignOutAgent,
    SignOutAgentLogin,
    UpdateOrganizationPolicy,
    ListAuditLogs,
    ExportAuditLogs,
    ReadAuditLog,
    ListAuditDestinations,
    CreateAuditDestination,
    ReadAuditDestination,
    ReadAuditRetention,
    UpdateAuditRetention,
    ListAuditExports,
    CreateRole,
    ReadWorkspace,
    ListWorkspaceMembers,
    ReadHostDirectories,
    ReadAnalytics,
    ListPullRequests,
    ListReleases,
    ReadTheme,
    ListTasks,
    ReadWorkspaceAiSettings,
    ReadWorkspaceMcpServers,
    ReadWorkspacePolicy,
    UpdateHostDirectories,
    UpdateTheme,
    ClearTheme,
    CreateTask,
    CreateChat,
    UpdateWorkspace,
    DeleteWorkspace,
    AddWorkspaceMember,
    ChangeWorkspaceMember,
    RemoveWorkspaceMember,
    UpdateWorkspaceAiSettings,
    ClearWorkspaceAiSettings,
    UpdateWorkspaceMcpServers,
    UpdateWorkspacePolicy,
    DeleteProjectSync,
    RotateProjectSyncSecret,
}

const ORGANIZATION_ROUTES: [Route; 46] = [
    Route::ReadOrganization,
    Route::ListMembers,
    Route::ListWorkspaces,
    Route::ReadGitHub,
    Route::ListRepositories,
    Route::ListMcpServers,
    Route::ReadOrganizationPolicy,
    Route::ListAgents,
    Route::ListRoles,
    Route::UpdateOrganization,
    Route::DeleteOrganization,
    Route::AddMember,
    Route::ChangeMember,
    Route::RemoveMember,
    Route::CreateInvitation,
    Route::ListInvitations,
    Route::RevokeInvitation,
    Route::CreateWorkspace,
    Route::ListDevices,
    Route::ReadDevicePolicy,
    Route::UpdateDevice,
    Route::SetDevicePolicy,
    Route::RegisterGitHubApp,
    Route::DeleteGitHubApp,
    Route::InstallGitHubApp,
    Route::SyncGitHubInstallations,
    Route::DeleteGitHubInstallation,
    Route::CreateMcpServer,
    Route::UpdateMcpServer,
    Route::DeleteMcpServer,
    Route::ReadAiSettings,
    Route::UpdateAiSettings,
    Route::ClearAiSettings,
    Route::SignOutAgent,
    Route::SignOutAgentLogin,
    Route::UpdateOrganizationPolicy,
    Route::ListAuditLogs,
    Route::ExportAuditLogs,
    Route::ReadAuditLog,
    Route::ListAuditDestinations,
    Route::CreateAuditDestination,
    Route::ReadAuditDestination,
    Route::ReadAuditRetention,
    Route::UpdateAuditRetention,
    Route::ListAuditExports,
    Route::CreateRole,
];

const WORKSPACE_ROUTES: [Route; 27] = [
    Route::ReadWorkspace,
    Route::ListWorkspaceMembers,
    Route::ReadHostDirectories,
    Route::ReadAnalytics,
    Route::ListPullRequests,
    Route::ListReleases,
    Route::ReadTheme,
    Route::ListTasks,
    Route::ReadWorkspaceAiSettings,
    Route::ReadWorkspaceMcpServers,
    Route::ReadWorkspacePolicy,
    Route::UpdateHostDirectories,
    Route::UpdateTheme,
    Route::ClearTheme,
    Route::CreateTask,
    Route::CreateChat,
    Route::UpdateWorkspace,
    Route::DeleteWorkspace,
    Route::AddWorkspaceMember,
    Route::ChangeWorkspaceMember,
    Route::RemoveWorkspaceMember,
    Route::UpdateWorkspaceAiSettings,
    Route::ClearWorkspaceAiSettings,
    Route::UpdateWorkspaceMcpServers,
    Route::UpdateWorkspacePolicy,
    Route::DeleteProjectSync,
    Route::RotateProjectSyncSecret,
];

impl Route {
    fn permission(self) -> Permission {
        match self {
            Self::ReadOrganization
            | Self::ListMembers
            | Self::ListWorkspaces
            | Self::ReadGitHub
            | Self::ListRepositories
            | Self::ListMcpServers
            | Self::ReadOrganizationPolicy
            | Self::ListAgents
            | Self::ListRoles => Permission::OrganizationRead,
            Self::UpdateOrganization => Permission::OrganizationUpdate,
            Self::DeleteOrganization => Permission::OrganizationDelete,
            Self::AddMember => Permission::MembersCreate,
            Self::ChangeMember => Permission::MembersUpdate,
            Self::RemoveMember => Permission::MembersDelete,
            Self::CreateInvitation => Permission::InvitationsCreate,
            Self::ListInvitations => Permission::InvitationsRead,
            Self::RevokeInvitation => Permission::InvitationsDelete,
            Self::CreateWorkspace => Permission::WorkspacesCreate,
            Self::ListDevices | Self::ReadDevicePolicy => Permission::DevicesRead,
            Self::UpdateDevice | Self::SetDevicePolicy => Permission::DevicesUpdate,
            Self::RegisterGitHubApp
            | Self::DeleteGitHubApp
            | Self::InstallGitHubApp
            | Self::SyncGitHubInstallations
            | Self::DeleteGitHubInstallation => Permission::GitHubUpdate,
            Self::CreateMcpServer | Self::UpdateMcpServer | Self::DeleteMcpServer => {
                Permission::McpServersUpdate
            }
            Self::ReadAiSettings => Permission::AiSettingsRead,
            Self::UpdateAiSettings | Self::ClearAiSettings => Permission::AiSettingsUpdate,
            Self::SignOutAgent | Self::SignOutAgentLogin => Permission::AgentLoginsUpdate,
            Self::UpdateOrganizationPolicy => Permission::PoliciesUpdate,
            Self::ListAuditLogs | Self::ExportAuditLogs | Self::ReadAuditLog => {
                Permission::AuditLogsRead
            }
            Self::ListAuditDestinations
            | Self::CreateAuditDestination
            | Self::ReadAuditDestination
            | Self::ReadAuditRetention
            | Self::UpdateAuditRetention
            | Self::ListAuditExports => Permission::AuditUpdate,
            Self::CreateRole => Permission::RolesUpdate,
            Self::ReadWorkspace
            | Self::ListWorkspaceMembers
            | Self::ReadHostDirectories
            | Self::ReadAnalytics
            | Self::ListPullRequests
            | Self::ListReleases
            | Self::ReadTheme
            | Self::ListTasks
            | Self::ReadWorkspaceAiSettings
            | Self::ReadWorkspaceMcpServers
            | Self::ReadWorkspacePolicy => Permission::WorkspaceRead,
            Self::UpdateHostDirectories
            | Self::UpdateTheme
            | Self::ClearTheme
            | Self::CreateTask
            | Self::CreateChat => Permission::WorkspaceWrite,
            Self::UpdateWorkspace => Permission::WorkspaceUpdate,
            Self::DeleteWorkspace => Permission::WorkspaceDelete,
            Self::AddWorkspaceMember => Permission::WorkspaceMembersCreate,
            Self::ChangeWorkspaceMember => Permission::WorkspaceMembersUpdate,
            Self::RemoveWorkspaceMember => Permission::WorkspaceMembersDelete,
            Self::UpdateWorkspaceAiSettings | Self::ClearWorkspaceAiSettings => {
                Permission::WorkspaceAiSettingsUpdate
            }
            Self::UpdateWorkspaceMcpServers => Permission::WorkspaceMcpServersUpdate,
            Self::UpdateWorkspacePolicy => Permission::WorkspacePoliciesUpdate,
            Self::DeleteProjectSync | Self::RotateProjectSyncSecret => {
                Permission::ProjectSyncsUpdate
            }
        }
    }

    /// The call, with fresh targets for a route that changes or removes one.
    async fn call(self, fixture: &Fixture, project: Uuid) -> Call {
        let organization = format!("/api/organizations/{}", fixture.organization);
        let workspace = format!("/api/workspaces/{}", fixture.workspace);
        let nested = format!("{organization}/workspaces/{}", fixture.workspace);
        let absent = Uuid::new_v4();
        match self {
            Self::ReadOrganization => Call::get(organization),
            Self::ListMembers => Call::get(format!("{organization}/members")),
            Self::ListWorkspaces => Call::get(format!("{organization}/workspaces")),
            Self::ReadGitHub => Call::get(format!("{organization}/github")),
            Self::ListRepositories => Call::get(format!("{organization}/github/repositories")),
            Self::ListMcpServers => Call::get(format!("{organization}/mcp-servers")),
            Self::ReadOrganizationPolicy => Call::get(format!("{organization}/agent-policy")),
            Self::ListAgents => Call::get(format!("{organization}/agents")),
            Self::ListRoles => Call::get(format!("{organization}/roles")),
            Self::UpdateOrganization => Call::patch(
                organization,
                json!({ "description": "Edited by a custom role" }),
            ),
            Self::DeleteOrganization => Call::delete(organization),
            Self::AddMember => Call::post(
                format!("{organization}/members"),
                json!({ "user_id": user(fixture.pool()).await, "role": OrgRole::Member.as_str() }),
            ),
            Self::ChangeMember => Call::patch(
                format!(
                    "{organization}/members/{}",
                    organization_member(fixture).await
                ),
                json!({ "role": OrgRole::Member.as_str() }),
            ),
            Self::RemoveMember => Call::delete(format!(
                "{organization}/members/{}",
                organization_member(fixture).await
            )),
            Self::CreateInvitation => Call::post(
                format!("{organization}/invitations"),
                json!({
                    "email": format!("{}@example.test", Uuid::new_v4()),
                    "org_role": OrgRole::Member.as_str(),
                }),
            ),
            Self::ListInvitations => Call::get(format!("{organization}/invitations")),
            Self::RevokeInvitation => Call::delete(format!("{organization}/invitations/{absent}")),
            Self::CreateWorkspace => Call::post(
                format!("{organization}/workspaces"),
                json!({ "name": "Created by a custom role", "slug": absent.to_string() }),
            ),
            Self::ListDevices => Call::get(format!("{organization}/devices")),
            Self::ReadDevicePolicy => Call::get(format!("{organization}/device-policy")),
            Self::UpdateDevice => Call::patch(
                format!("{organization}/devices/{absent}"),
                json!({ "name": "Renamed by a custom role" }),
            ),
            Self::SetDevicePolicy => Call::put(
                format!("{organization}/device-policy"),
                json!({ "mode": "open" }),
            ),
            Self::RegisterGitHubApp => Call::post(format!("{organization}/github/apps"), json!({})),
            Self::DeleteGitHubApp => Call::delete(format!("{organization}/github/apps/{absent}")),
            Self::InstallGitHubApp => Call::post(
                format!("{organization}/github/apps/{absent}/installations"),
                json!({}),
            ),
            Self::SyncGitHubInstallations => Call::post(
                format!("{organization}/github/installations/sync"),
                json!({}),
            ),
            Self::DeleteGitHubInstallation => {
                Call::delete(format!("{organization}/github/installations/{absent}"))
            }
            Self::CreateMcpServer => {
                Call::post(format!("{organization}/mcp-servers"), mcp_server())
            }
            Self::UpdateMcpServer => {
                Call::put(format!("{organization}/mcp-servers/{absent}"), mcp_server())
            }
            Self::DeleteMcpServer => Call::delete(format!("{organization}/mcp-servers/{absent}")),
            Self::ReadAiSettings => Call::get(format!("{organization}/settings/ai")),
            Self::UpdateAiSettings => Call::put(
                format!("{organization}/settings/ai"),
                json!({
                    "provider": "self_hosted",
                    "litellm_host": "http://localhost:4000",
                    "litellm_key": "sk-custom-role",
                }),
            ),
            Self::ClearAiSettings => Call::delete(format!("{organization}/settings/ai")),
            Self::SignOutAgent => Call::delete(format!("{organization}/agents/claude/login")),
            Self::SignOutAgentLogin => {
                Call::delete(format!("{organization}/agents/codex/logins/{absent}"))
            }
            Self::UpdateOrganizationPolicy => Call::patch(
                format!("{organization}/agent-policy"),
                json!({ "agents": [] }),
            ),
            Self::ListAuditLogs => Call::get(format!("{organization}/audit-logs")),
            Self::ExportAuditLogs => {
                Call::get(format!("{organization}/audit-logs/export?{EXPORTED_RANGE}"))
            }
            Self::ReadAuditLog => Call::get(format!("{organization}/audit-logs/{absent}")),
            Self::ListAuditDestinations => Call::get(format!("{organization}/audit-destinations")),
            Self::CreateAuditDestination => Call::post(
                format!("{organization}/audit-destinations"),
                json!({ "name": "Probe", "settings": {} }),
            ),
            Self::ReadAuditDestination => {
                Call::get(format!("{organization}/audit-destinations/{absent}"))
            }
            Self::ReadAuditRetention => Call::get(format!("{organization}/audit-retention")),
            Self::UpdateAuditRetention => Call::put(
                format!("{organization}/audit-retention"),
                json!({ "days": null }),
            ),
            Self::ListAuditExports => Call::get(format!("{organization}/audit-exports")),
            Self::CreateRole => Call::post(
                format!("{organization}/roles"),
                json!({ "name": format!("Role {absent}"), "scope": Scope::Workspace.name(), "permissions": [] }),
            ),
            Self::ReadWorkspace => Call::get(workspace),
            Self::ListWorkspaceMembers => Call::get(format!("{workspace}/members")),
            Self::ReadHostDirectories => Call::get(format!("{workspace}/host-directories")),
            Self::ReadAnalytics => Call::get(format!("{workspace}/analytics")),
            Self::ListPullRequests => Call::get(format!("{workspace}/pull-requests")),
            Self::ListReleases => Call::get(format!("{workspace}/releases")),
            Self::ReadTheme => Call::get(format!("{workspace}/theme")),
            Self::ListTasks => Call::get(format!("{workspace}/tasks")),
            Self::ReadWorkspaceAiSettings => Call::get(format!("{nested}/settings/ai")),
            Self::ReadWorkspaceMcpServers => Call::get(format!("{nested}/mcp-servers")),
            Self::ReadWorkspacePolicy => Call::get(format!("{nested}/agent-policy")),
            Self::UpdateHostDirectories => Call::put(
                format!("{workspace}/host-directories"),
                json!({ "directories": [] }),
            ),
            Self::UpdateTheme => Call::put(
                format!("{workspace}/theme"),
                json!({ "primary_color_light": "#112233" }),
            ),
            Self::ClearTheme => Call::delete(format!("{workspace}/theme")),
            Self::CreateTask => Call::post(
                format!("{workspace}/tasks"),
                json!({ "description": "Written by a custom role" }),
            ),
            Self::CreateChat => Call::post(
                "/api/chats",
                json!({
                    "workspace_id": fixture.workspace,
                    "title": "Started by a custom role",
                    "model_name": "probe",
                }),
            ),
            Self::UpdateWorkspace => Call::patch(
                workspace,
                json!({ "description": "Edited by a custom role" }),
            ),
            Self::DeleteWorkspace => Call::delete(workspace),
            Self::AddWorkspaceMember => Call::post(
                format!("{workspace}/members"),
                json!({
                    "user_id": organization_member(fixture).await,
                    "role": WorkspaceRole::Viewer.as_str(),
                }),
            ),
            Self::ChangeWorkspaceMember => Call::patch(
                format!("{workspace}/members/{}", workspace_viewer(fixture).await),
                json!({ "role": WorkspaceRole::Viewer.as_str() }),
            ),
            Self::RemoveWorkspaceMember => Call::delete(format!(
                "{workspace}/members/{}",
                workspace_viewer(fixture).await
            )),
            Self::UpdateWorkspaceAiSettings => Call::put(
                format!("{nested}/settings/ai"),
                json!({
                    "provider": "anthropic",
                    "anthropic_api_key": "custom-role-key",
                    "model_fast": "claude-3-haiku-20240307",
                }),
            ),
            Self::ClearWorkspaceAiSettings => Call::delete(format!("{nested}/settings/ai")),
            Self::UpdateWorkspaceMcpServers => {
                Call::put(format!("{nested}/mcp-servers"), json!({ "servers": {} }))
            }
            Self::UpdateWorkspacePolicy => {
                Call::patch(format!("{nested}/agent-policy"), json!({ "agents": [] }))
            }
            Self::DeleteProjectSync => {
                Call::delete(format!("/api/projects/{project}/sync/{absent}"))
            }
            Self::RotateProjectSyncSecret => Call::put(
                format!("/api/projects/{project}/sync/{absent}/webhook-secret"),
                json!({}),
            ),
        }
    }
}

fn mcp_server() -> serde_json::Value {
    json!({
        "name": format!("probe-{}", Uuid::new_v4().simple()),
        "transport": "http",
        "url": REFUSED_URL,
    })
}

async fn organization_member(fixture: &Fixture) -> Uuid {
    let member = user(fixture.pool()).await;
    seat_organization(
        fixture.pool(),
        fixture.organization,
        member,
        OrgRole::Member,
    )
    .await;
    member
}

async fn workspace_viewer(fixture: &Fixture) -> Uuid {
    let member = organization_member(fixture).await;
    seat_workspace(
        fixture.pool(),
        fixture.workspace,
        member,
        WorkspaceRole::Viewer,
    )
    .await;
    member
}

async fn holder(fixture: &Fixture, scope: Scope, permissions: &[Permission]) -> Account {
    match scope {
        Scope::Workspace => workspace_holder(fixture, permissions).await,
        _ => organization_holder(fixture, permissions).await,
    }
}

/// Calls every route as the holders it names, returning each departure from
/// the contract.
async fn departures(fixture: &Fixture, scope: Scope, routes: &[Route]) -> Vec<String> {
    let project = projects::create_project(fixture.pool(), "Probe", None, Some(fixture.workspace))
        .await
        .expect("a project")
        .id;
    let theme = Route::UpdateTheme.call(fixture, project).await;
    let response = theme.send(fixture, &fixture.owner.token).await;
    assert!(
        response.status.is_success(),
        "{}",
        describe(&theme, &response)
    );
    let base = holder(fixture, scope, &[]).await;
    let everything = holder(fixture, scope, &composable(scope)).await;
    let mut holders: HashMap<Permission, Account> = HashMap::new();
    let mut departures = Vec::new();

    for route in routes {
        let permission = route.permission();
        assert_eq!(
            permission.scope(),
            scope,
            "{route:?} is listed under the wrong scope"
        );

        if Some(permission) == scope.base() {
            let call = route.call(fixture, project).await;
            let response = call.send(fixture, &base.token).await;
            if !response.status.is_success() {
                departures.push(format!(
                    "{route:?}: a holder of {} only is refused: {}",
                    permission.name(),
                    describe(&call, &response)
                ));
            }
            continue;
        }

        if !permission.composable() {
            let call = route.call(fixture, project).await;
            let response = call.send(fixture, &everything.token).await;
            if response.status != StatusCode::FORBIDDEN {
                departures.push(format!(
                    "{route:?}: a holder of every composable permission is not refused {}: {}",
                    permission.name(),
                    describe(&call, &response)
                ));
            }
            continue;
        }

        if let Entry::Vacant(slot) = holders.entry(permission) {
            slot.insert(holder(fixture, scope, &[permission]).await);
        }
        let granted = &holders[&permission];

        let call = route.call(fixture, project).await;
        let response = call.send(fixture, &granted.token).await;
        if response.status == StatusCode::FORBIDDEN {
            departures.push(format!(
                "{route:?}: a holder of {} is refused: {}",
                permission.name(),
                describe(&call, &response)
            ));
        }

        let call = route.call(fixture, project).await;
        let response = call.send(fixture, &base.token).await;
        if response.status != StatusCode::FORBIDDEN {
            departures.push(format!(
                "{route:?}: a holder without {} is not refused: {}",
                permission.name(),
                describe(&call, &response)
            ));
        }
    }

    departures
}

#[tokio::test]
async fn every_organization_route_admits_its_permission_and_refuses_without_it() {
    let fixture = licensed().await;

    let departures = departures(&fixture, Scope::Organization, &ORGANIZATION_ROUTES).await;

    fixture.cleanup().await;
    assert!(departures.is_empty(), "{}", departures.join("\n"));
}

#[tokio::test]
async fn every_workspace_route_admits_its_permission_and_refuses_without_it() {
    let fixture = licensed().await;

    let departures = departures(&fixture, Scope::Workspace, &WORKSPACE_ROUTES).await;

    fixture.cleanup().await;
    assert!(departures.is_empty(), "{}", departures.join("\n"));
}

#[test]
fn the_route_lists_cover_every_composable_permission() {
    for scope in [Scope::Organization, Scope::Workspace] {
        let routes: &[Route] = match scope {
            Scope::Workspace => &WORKSPACE_ROUTES,
            _ => &ORGANIZATION_ROUTES,
        };
        for permission in composable(scope) {
            assert!(
                UNROUTED.contains(&permission)
                    || routes.iter().any(|route| route.permission() == permission),
                "no route checks {}",
                permission.name()
            );
        }
    }
}
