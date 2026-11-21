use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::auth::{Grant, Scope};
use crate::db::custom_roles::{self, Creation, Definition};
use crate::db::organization_members::OrgRole;
use crate::db::workspace_members::WorkspaceRole;

struct Tenant {
    pool: PgPool,
    organization: Uuid,
    workspace: Uuid,
}

impl Tenant {
    async fn new() -> Self {
        let url = std::env::var("TEST_DATABASE_URL").expect("isolated test database");
        let pool = PgPool::connect(&url)
            .await
            .expect("the test database accepts connections");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Policy', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Policy', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(&pool)
        .await
        .expect("a workspace");
        Self {
            pool,
            organization,
            workspace,
        }
    }

    async fn user(&self) -> Uuid {
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash) VALUES ($1, $1::text || '@zone.test', 'test-only')",
        )
        .bind(user)
        .execute(&self.pool)
        .await
        .expect("a user");
        user
    }

    async fn organization_member(&self, role: OrgRole) -> Uuid {
        let user = self.user().await;
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(self.organization)
        .bind(user)
        .bind(role.as_str())
        .execute(&self.pool)
        .await
        .expect("an organization membership");
        user
    }

    async fn workspace_member(&self, user: Uuid, role: WorkspaceRole) {
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(self.workspace)
        .bind(user)
        .bind(role.as_str())
        .execute(&self.pool)
        .await
        .expect("a workspace membership");
    }

    async fn custom_role(&self, scope: Scope, permission: Permission) -> Uuid {
        let grant =
            Grant::compose(scope, &[permission.name().to_owned()]).expect("a composable grant");
        let name = Uuid::new_v4().to_string();
        let definition = Definition {
            name: &name,
            description: None,
            scope,
            grant: &grant,
        };
        let creator = self.user().await;
        match custom_roles::create(&self.pool, self.organization, &definition, creator)
            .await
            .expect("a custom role")
        {
            Creation::Created(role) => role.id,
            Creation::NameTaken => panic!("a fresh name is never taken"),
        }
    }

    async fn hold_organization_role(&self, user: Uuid, role: Uuid) {
        sqlx::query(
            "UPDATE organization_members SET custom_role_id = $3 WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(self.organization)
        .bind(user)
        .bind(role)
        .execute(&self.pool)
        .await
        .expect("an organization custom role");
    }

    async fn hold_workspace_role(&self, user: Uuid, role: Uuid) {
        sqlx::query(
            "UPDATE workspace_members SET custom_role_id = $3 WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(self.workspace)
        .bind(user)
        .bind(role)
        .execute(&self.pool)
        .await
        .expect("a workspace custom role");
    }

    async fn organization_access(&self, user: Uuid) -> Result<Access, Refusal> {
        let mut connection = self.pool.acquire().await.expect("a connection");
        organization(&mut connection, self.organization, user).await
    }

    async fn workspace_access(&self, user: Uuid) -> Result<Access, Refusal> {
        let mut connection = self.pool.acquire().await.expect("a connection");
        workspace(&mut connection, self.organization, self.workspace, user).await
    }
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
        .custom_role(Scope::Organization, Permission::PoliciesUpdate)
        .await;
    let auditing = tenant
        .custom_role(Scope::Organization, Permission::AuditLogsRead)
        .await;
    tenant.hold_organization_role(editor, editing).await;
    tenant.hold_organization_role(auditor, auditing).await;

    for (user, expected) in [
        (admin, Access::Write),
        (editor, Access::Write),
        (member, Access::Read),
        (auditor, Access::Read),
    ] {
        let access = tenant.organization_access(user).await;
        assert!(
            matches!(access, Ok(granted) if granted == expected),
            "{access:?}"
        );
    }
    let missing = tenant.organization_access(outsider).await;
    assert!(
        matches!(missing, Err(Refusal::Missing(ORGANIZATION_MISSING))),
        "{missing:?}"
    );
}

#[tokio::test]
async fn the_workspace_policy_cascades_from_the_organization_policies_permission() {
    let tenant = Tenant::new().await;
    let organization_editor = tenant.organization_member(OrgRole::Member).await;
    let editing = tenant
        .custom_role(Scope::Organization, Permission::PoliciesUpdate)
        .await;
    tenant
        .hold_organization_role(organization_editor, editing)
        .await;
    let organization_admin = tenant.organization_member(OrgRole::Admin).await;
    let unseated = tenant.organization_member(OrgRole::Member).await;

    for user in [organization_editor, organization_admin] {
        let access = tenant.workspace_access(user).await;
        assert!(matches!(access, Ok(Access::Write)), "{access:?}");
    }
    let missing = tenant.workspace_access(unseated).await;
    assert!(
        matches!(missing, Err(Refusal::Missing(WORKSPACE_MISSING))),
        "{missing:?}"
    );
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
        .custom_role(Scope::Workspace, Permission::WorkspacePoliciesUpdate)
        .await;
    let writing = tenant
        .custom_role(Scope::Workspace, Permission::WorkspaceWrite)
        .await;
    tenant.hold_workspace_role(editor, editing).await;
    tenant.hold_workspace_role(writer, writing).await;

    for (user, expected) in [
        (workspace_admin, Access::Write),
        (editor, Access::Write),
        (viewer, Access::Read),
        (writer, Access::Read),
    ] {
        let access = tenant.workspace_access(user).await;
        assert!(
            matches!(access, Ok(granted) if granted == expected),
            "{access:?}"
        );
    }
}
