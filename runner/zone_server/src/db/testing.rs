//! A throwaway organization with one workspace, for tests that judge callers
//! against their memberships.

use std::sync::Mutex;

use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{Permission, Scope};
use crate::db::custom_roles;
use crate::db::organization_members::OrgRole;
use crate::db::workspace_members::WorkspaceRole;

pub(crate) struct Tenant {
    pub pool: PgPool,
    pub organization: Uuid,
    pub workspace: Uuid,
    users: Mutex<Vec<Uuid>>,
}

impl Tenant {
    pub async fn new() -> Self {
        let url = std::env::var("TEST_DATABASE_URL").expect("isolated test database");
        let pool = PgPool::connect(&url)
            .await
            .expect("the test database accepts connections");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Tenant', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) \
             VALUES ($1, $2, 'Tenant', $1::text)",
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
            users: Mutex::new(Vec::new()),
        }
    }

    pub async fn user(&self) -> Uuid {
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash) \
             VALUES ($1, $1::text || '@zone.test', 'test-only')",
        )
        .bind(user)
        .execute(&self.pool)
        .await
        .expect("a user");
        self.users.lock().expect("the user list").push(user);
        user
    }

    pub async fn organization_member(&self, role: OrgRole) -> Uuid {
        let user = self.user().await;
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) \
             VALUES ($1, $2, $3)",
        )
        .bind(self.organization)
        .bind(user)
        .bind(role.as_str())
        .execute(&self.pool)
        .await
        .expect("an organization membership");
        user
    }

    pub async fn workspace_member(&self, user: Uuid, role: WorkspaceRole) {
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

    /// An organization member seated in the workspace as a viewer and moved
    /// onto a fresh workspace role holding `permissions` and the base.
    pub async fn workspace_holder(&self, permissions: &[Permission]) -> Uuid {
        let user = self.organization_member(OrgRole::Member).await;
        self.workspace_member(user, WorkspaceRole::Viewer).await;
        let role = self.custom_role(Scope::Workspace, permissions).await;
        self.hold_workspace_role(user, role).await;
        user
    }

    /// A custom role of this tenant's organization holding `permissions` and
    /// the base of `scope`.
    pub async fn custom_role(&self, scope: Scope, permissions: &[Permission]) -> Uuid {
        let creator = self.user().await;
        custom_roles::testing::role(&self.pool, self.organization, creator, scope, permissions)
            .await
    }

    pub async fn hold_organization_role(&self, user: Uuid, role: Uuid) {
        custom_roles::testing::assign_organization(&self.pool, self.organization, user, role).await;
    }

    pub async fn hold_workspace_role(&self, user: Uuid, role: Uuid) {
        custom_roles::testing::assign_workspace(&self.pool, self.workspace, user, role).await;
    }

    /// Removes the organization, which takes its workspace, memberships and
    /// roles with it, and then every user this tenant made. A tenant whose
    /// members hold another tenant's role is cleaned up before that tenant.
    pub async fn cleanup(self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .expect("the organization is removed");
        let users = self.users.into_inner().expect("the user list");
        sqlx::query("DELETE FROM users WHERE id = ANY($1)")
            .bind(&users)
            .execute(&self.pool)
            .await
            .expect("the users are removed");
    }
}
