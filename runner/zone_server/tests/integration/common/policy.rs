//! A licensed server with one organization and one workspace, both owned by the server admin, and
//! helpers for the agent policy routes. Wraps [`sso::Fixture`] for the license helpers; every
//! fixture has a database of its own, because the license is server-wide.

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::db::{organizations, policies, workspaces};
use zone_server::state::AppState;

use super::TestResponse;
use super::sso::{self, Account, Audit};

const POLICY_PATH: &str = "agent-policy";

pub fn organization_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/{POLICY_PATH}")
}

pub fn workspace_path(organization: Uuid, workspace: Uuid) -> String {
    format!("/api/organizations/{organization}/workspaces/{workspace}/{POLICY_PATH}")
}

pub struct Fixture {
    pub sso: sso::Fixture,
    /// The server admin, who owns the organization and the workspace.
    pub owner: Account,
    pub organization: Uuid,
    pub workspace: Uuid,
}

impl Fixture {
    pub async fn new(prefix: &'static str) -> Self {
        let sso = sso::Fixture::new(prefix).await;
        let owner = sso.register().await;
        let organization = create_organization(sso.pool(), owner.id).await;
        let workspace = create_workspace(sso.pool(), organization, owner.id).await;
        Self {
            sso,
            owner,
            organization,
            workspace,
        }
    }

    pub fn pool(&self) -> &PgPool {
        self.sso.pool()
    }

    pub fn state(&self) -> &AppState {
        self.sso.state()
    }

    pub async fn license(&self) {
        self.sso.license(&self.owner).await;
    }

    /// Installs a key through the license route, such as one in its grace period.
    pub async fn install(&self, key: &str) {
        self.sso.install(&self.owner, key).await;
    }

    /// Stores a key the license route would refuse, such as a lapsed one.
    pub async fn store_license(&self, key: &str) {
        self.sso.store_license(key).await;
    }

    pub async fn stranger(&self) -> Account {
        self.sso.register().await
    }

    /// Registers someone with `role` in the organization and, when given, `workspace` in the
    /// fixture's workspace.
    pub async fn member(&self, role: OrgRole, workspace: Option<WorkspaceRole>) -> Account {
        let account = self.sso.register().await;
        organization_members::add_member(self.pool(), self.organization, account.id, role, None)
            .await
            .unwrap();
        if let Some(workspace) = workspace {
            self.seat(self.workspace, &account, workspace).await;
        }
        account
    }

    pub async fn seat(&self, workspace: Uuid, account: &Account, role: WorkspaceRole) {
        workspace_members::add_member(self.pool(), workspace, account.id, role, None)
            .await
            .unwrap();
    }

    pub async fn another_workspace(&self) -> Uuid {
        create_workspace(self.pool(), self.organization, self.owner.id).await
    }

    pub async fn another_organization(&self, owner: &Account) -> (Uuid, Uuid) {
        let organization = create_organization(self.pool(), owner.id).await;
        let workspace = create_workspace(self.pool(), organization, owner.id).await;
        (organization, workspace)
    }

    pub async fn organization_policy(&self, token: &str) -> TestResponse {
        self.sso
            .client
            .get_auth(&organization_path(self.organization), token)
            .await
    }

    pub async fn update_organization(&self, token: &str, body: &Value) -> TestResponse {
        self.sso
            .client
            .patch_json_auth(&organization_path(self.organization), body, token)
            .await
    }

    pub async fn workspace_policy(&self, token: &str) -> TestResponse {
        self.workspace_policy_of(self.organization, self.workspace, token)
            .await
    }

    pub async fn update_workspace(&self, token: &str, body: &Value) -> TestResponse {
        self.update_workspace_of(self.organization, self.workspace, token, body)
            .await
    }

    pub async fn workspace_policy_of(
        &self,
        organization: Uuid,
        workspace: Uuid,
        token: &str,
    ) -> TestResponse {
        self.sso
            .client
            .get_auth(&workspace_path(organization, workspace), token)
            .await
    }

    pub async fn update_workspace_of(
        &self,
        organization: Uuid,
        workspace: Uuid,
        token: &str,
        body: &Value,
    ) -> TestResponse {
        self.sso
            .client
            .patch_json_auth(&workspace_path(organization, workspace), body, token)
            .await
    }

    /// Stores `rules` as they are, without validating them: the workspace's layer when
    /// `workspace` is given, otherwise the organization's.
    pub async fn write_rules(&self, organization: Uuid, workspace: Option<Uuid>, rules: Value) {
        let mut transaction = self.pool().begin().await.unwrap();
        match workspace {
            Some(workspace) => {
                policies::save_workspace(&mut transaction, workspace, &rules, self.owner.id)
                    .await
                    .unwrap();
            }
            None => {
                policies::save_organization(&mut transaction, organization, &rules, self.owner.id)
                    .await
                    .unwrap();
            }
        }
        transaction.commit().await.unwrap();
    }

    pub async fn write_organization(&self, rules: Value) {
        self.write_rules(self.organization, None, rules).await;
    }

    pub async fn write_workspace(&self, rules: Value) {
        self.write_rules(self.organization, Some(self.workspace), rules)
            .await;
    }

    pub async fn audits(&self, action: &str) -> Vec<Audit> {
        self.sso.audits(action).await
    }

    pub async fn cleanup(self) {
        self.sso.cleanup().await;
    }
}

async fn create_organization(pool: &PgPool, owner: Uuid) -> Uuid {
    let slug = format!("policy-{}", Uuid::new_v4());
    let organization = organizations::create_organization(pool, "Policy", &slug, None)
        .await
        .unwrap()
        .id;
    organization_members::add_member(pool, organization, owner, OrgRole::Owner, None)
        .await
        .unwrap();
    organization
}

async fn create_workspace(pool: &PgPool, organization: Uuid, owner: Uuid) -> Uuid {
    let slug = format!("policy-{}", Uuid::new_v4());
    let workspace = workspaces::create_workspace(pool, organization, "Policy", &slug, None)
        .await
        .unwrap()
        .id;
    workspace_members::add_member(pool, workspace, owner, WorkspaceRole::Owner, None)
        .await
        .unwrap();
    workspace
}
