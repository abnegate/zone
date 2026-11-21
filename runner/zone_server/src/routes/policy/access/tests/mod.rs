use uuid::Uuid;

use super::*;
use crate::auth::Scope;
use crate::db::organization_members::OrgRole;
use crate::db::testing::Tenant;
use crate::db::workspace_members::WorkspaceRole;

async fn organization_access(tenant: &Tenant, user: Uuid) -> Result<Access, Refusal> {
    let mut connection = tenant.pool.acquire().await.expect("a connection");
    organization(&mut connection, tenant.organization, user).await
}

async fn workspace_access(tenant: &Tenant, user: Uuid) -> Result<Access, Refusal> {
    let mut connection = tenant.pool.acquire().await.expect("a connection");
    workspace(&mut connection, tenant.organization, tenant.workspace, user).await
}

#[tokio::test]
async fn the_policies_permission_writes_the_organization_policy() {
    let tenant = Tenant::new().await;
    let admin = tenant.organization_member(OrgRole::Admin).await;
    let member = tenant.organization_member(OrgRole::Member).await;
    let editor = tenant.organization_member(OrgRole::Member).await;
    let auditor = tenant.organization_member(OrgRole::Member).await;
    let outsider = tenant.user().await;
    let editing = tenant
        .custom_role(Scope::Organization, &[Permission::PoliciesUpdate])
        .await;
    let auditing = tenant
        .custom_role(Scope::Organization, &[Permission::AuditLogsRead])
        .await;
    tenant.hold_organization_role(editor, editing).await;
    tenant.hold_organization_role(auditor, auditing).await;

    for (user, expected) in [
        (admin, Access::Write),
        (editor, Access::Write),
        (member, Access::Read),
        (auditor, Access::Read),
    ] {
        let access = organization_access(&tenant, user).await;
        assert!(
            matches!(access, Ok(granted) if granted == expected),
            "{access:?}"
        );
    }
    let missing = organization_access(&tenant, outsider).await;
    assert!(
        matches!(missing, Err(Refusal::Missing(ORGANIZATION_MISSING))),
        "{missing:?}"
    );
    tenant.cleanup().await;
}

#[tokio::test]
async fn the_workspace_policy_cascades_from_the_organization_policies_permission() {
    let tenant = Tenant::new().await;
    let organization_editor = tenant.organization_member(OrgRole::Member).await;
    let editing = tenant
        .custom_role(Scope::Organization, &[Permission::PoliciesUpdate])
        .await;
    tenant
        .hold_organization_role(organization_editor, editing)
        .await;
    let organization_admin = tenant.organization_member(OrgRole::Admin).await;
    let unseated = tenant.organization_member(OrgRole::Member).await;

    for user in [organization_editor, organization_admin] {
        let access = workspace_access(&tenant, user).await;
        assert!(matches!(access, Ok(Access::Write)), "{access:?}");
    }
    let missing = workspace_access(&tenant, unseated).await;
    assert!(
        matches!(missing, Err(Refusal::Missing(WORKSPACE_MISSING))),
        "{missing:?}"
    );
    tenant.cleanup().await;
}

#[tokio::test]
async fn the_workspace_policies_permission_writes_the_workspace_policy() {
    let tenant = Tenant::new().await;
    let workspace_admin = tenant.organization_member(OrgRole::Member).await;
    let viewer = tenant.organization_member(OrgRole::Member).await;
    let editor = tenant.organization_member(OrgRole::Member).await;
    let writer = tenant.organization_member(OrgRole::Member).await;
    tenant
        .workspace_member(workspace_admin, WorkspaceRole::Admin)
        .await;
    for user in [viewer, editor, writer] {
        tenant.workspace_member(user, WorkspaceRole::Viewer).await;
    }
    let editing = tenant
        .custom_role(Scope::Workspace, &[Permission::WorkspacePoliciesUpdate])
        .await;
    let writing = tenant
        .custom_role(Scope::Workspace, &[Permission::WorkspaceWrite])
        .await;
    tenant.hold_workspace_role(editor, editing).await;
    tenant.hold_workspace_role(writer, writing).await;

    for (user, expected) in [
        (workspace_admin, Access::Write),
        (editor, Access::Write),
        (viewer, Access::Read),
        (writer, Access::Read),
    ] {
        let access = workspace_access(&tenant, user).await;
        assert!(
            matches!(access, Ok(granted) if granted == expected),
            "{access:?}"
        );
    }
    tenant.cleanup().await;
}
