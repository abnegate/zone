//! A throwaway organization with one workspace, for tests that judge callers
//! against their memberships.

use std::sync::Mutex;

use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{Grant, Permission, Scope};
use crate::db::custom_roles::{self, Creation, Definition};
use crate::db::organization_members::OrgRole;
use crate::db::workspace_members::WorkspaceRole;

const HOLD_ORGANIZATION_ROLE: &str = "UPDATE organization_members SET custom_role_id = $3 \
     WHERE organization_id = $1 AND user_id = $2";
const HOLD_WORKSPACE_ROLE: &str = "UPDATE workspace_members SET custom_role_id = $3 \
     WHERE workspace_id = $1 AND user_id = $2";
const PIN_WORKSPACE_ROLE: &str = "UPDATE workspace_members SET role = $3, custom_role_id = $4 \
     WHERE workspace_id = $1 AND user_id = $2";

/// Moves the workspace member `user` onto a fresh custom role of
/// `organization` holding `permissions` and the base, pinned to viewer as
/// every custom holder is. For fixtures that seat their own members.
pub(crate) async fn hold(
    pool: &PgPool,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
    permissions: &[Permission],
) {
    let names: Vec<String> = permissions
        .iter()
        .map(|permission| permission.name().to_owned())
        .collect();
    let grant = Grant::compose(Scope::Workspace, &names).expect("a composable grant");
    let name = Uuid::new_v4().to_string();
    let definition = Definition {
        name: &name,
        description: None,
        scope: Scope::Workspace,
        grant: &grant,
    };
    let Creation::Created(role) = custom_roles::create(pool, organization, &definition, user)
        .await
        .expect("a custom role")
    else {
        panic!("a fresh name is never taken");
    };
    sqlx::query(PIN_WORKSPACE_ROLE)
        .bind(workspace)
        .bind(user)
        .bind(WorkspaceRole::Viewer.as_str())
        .bind(role.id)
        .execute(pool)
        .await
        .expect("a workspace custom role");
}

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
        let names: Vec<String> = permissions
            .iter()
            .map(|permission| permission.name().to_owned())
            .collect();
        let grant = Grant::compose(scope, &names).expect("a composable grant");
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

    pub async fn hold_organization_role(&self, user: Uuid, role: Uuid) {
        sqlx::query(HOLD_ORGANIZATION_ROLE)
            .bind(self.organization)
            .bind(user)
            .bind(role)
            .execute(&self.pool)
            .await
            .expect("an organization custom role");
    }

    pub async fn hold_workspace_role(&self, user: Uuid, role: Uuid) {
        sqlx::query(HOLD_WORKSPACE_ROLE)
            .bind(self.workspace)
            .bind(user)
            .bind(role)
            .execute(&self.pool)
            .await
            .expect("a workspace custom role");
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
