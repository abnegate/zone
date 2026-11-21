//! Custom roles held against the rules they replace and the routes they guard.
//!
//! - `oracle`: every built-in caller, target and granted role through
//!   `change_role` and `remove_guarded`, against the rules before permissions
//!   existed, re-encoded here.
//! - `assignments`: the outcomes only custom roles reach.
//! - `locking`: an assignment racing an edit of the role it assigns.
//! - `routes`: every organization and workspace route, held by a custom role
//!   with exactly its permission and by one without it.
//! - `members`: adding, ranking, escalating and licensing through the member
//!   routes.

mod assignments;
mod locking;
mod members;
mod oracle;
mod routes;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::{self, Assignment, Creation, Definition};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use crate::common::TestResponse;
use crate::common::policy::Fixture;
use crate::common::sso::Account;

const PREFIX: &str = "custom_role_enforcement";

async fn fixture() -> Fixture {
    Fixture::new(PREFIX).await
}

async fn licensed() -> Fixture {
    let fixture = fixture().await;
    fixture.license().await;
    fixture
}

fn names(permissions: &[Permission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| permission.name().to_string())
        .collect()
}

fn composed(scope: Scope, permissions: &[Permission]) -> Grant {
    Grant::compose(scope, &names(permissions)).expect("the grant composes")
}

/// Every permission a custom role of `scope` may hold.
fn composable(scope: Scope) -> Vec<Permission> {
    Permission::ALL
        .into_iter()
        .filter(|permission| permission.scope() == scope && permission.composable())
        .collect()
}

async fn user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("unused")
        .fetch_one(pool)
        .await
        .expect("a user")
}

async fn organization(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
        .bind("Enforcement")
        .bind(Uuid::new_v4().to_string())
        .fetch_one(pool)
        .await
        .expect("an organization")
}

async fn workspace(pool: &PgPool, organization: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind("Enforcement")
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .expect("a workspace")
}

async fn seat_organization(pool: &PgPool, organization: Uuid, member: Uuid, role: OrgRole) {
    organization_members::add_member(pool, organization, member, role, None)
        .await
        .expect("an organization member is seated");
}

async fn seat_workspace(pool: &PgPool, workspace: Uuid, member: Uuid, role: WorkspaceRole) {
    workspace_members::add_member(pool, workspace, member, role, None)
        .await
        .expect("a workspace member is seated");
}

/// A custom role of `scope` in `organization` holding `permissions` and its base.
async fn role(
    pool: &PgPool,
    organization: Uuid,
    creator: Uuid,
    scope: Scope,
    permissions: &[Permission],
) -> Uuid {
    let grant = composed(scope, permissions);
    let name = format!("Role {}", Uuid::new_v4());
    let definition = Definition {
        name: &name,
        description: None,
        scope,
        grant: &grant,
    };
    match custom_roles::create(pool, organization, &definition, creator)
        .await
        .expect("the role is created")
    {
        Creation::Created(role) => role.id,
        Creation::NameTaken => panic!("a fresh name is never taken"),
    }
}

/// Gives an organization member a custom role, as an owner would.
async fn assign_organization(pool: &PgPool, organization: Uuid, member: Uuid, role: Uuid) {
    let outcome = organization_members::change_role(
        pool,
        organization,
        member,
        Assignment::Custom(role),
        &Grant::organization(OrgRole::Owner),
    )
    .await
    .expect("the assignment is answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::Applied(_)),
        "an owner assigns a custom role to a member: {outcome:?}"
    );
}

/// Gives a workspace member a custom role, as an owner would.
async fn assign_workspace(pool: &PgPool, workspace: Uuid, member: Uuid, role: Uuid) {
    let outcome = workspace_members::change_role(
        pool,
        workspace,
        member,
        Assignment::Custom(role),
        &Grant::workspace(WorkspaceRole::Owner),
    )
    .await
    .expect("the assignment is answered");
    assert!(
        matches!(outcome, workspace_members::RoleChange::Applied(_)),
        "an owner assigns a custom role to a workspace member: {outcome:?}"
    );
}

/// Someone holding a custom organization role with exactly `permissions` in
/// the fixture's organization.
async fn organization_holder(fixture: &Fixture, permissions: &[Permission]) -> Account {
    let role = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
        Scope::Organization,
        permissions,
    )
    .await;
    holding_organization(fixture, role).await
}

/// Someone holding the custom organization role `role` in the fixture's organization.
async fn holding_organization(fixture: &Fixture, role: Uuid) -> Account {
    let account = fixture.member(OrgRole::Member, None).await;
    assign_organization(fixture.pool(), fixture.organization, account.id, role).await;
    account
}

/// Someone holding a custom workspace role with exactly `permissions` in the
/// fixture's workspace, and the built-in member role in its organization.
async fn workspace_holder(fixture: &Fixture, permissions: &[Permission]) -> Account {
    let role = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
        Scope::Workspace,
        permissions,
    )
    .await;
    holding_workspace(fixture, role).await
}

/// Someone holding the custom workspace role `role` in the fixture's workspace.
async fn holding_workspace(fixture: &Fixture, role: Uuid) -> Account {
    let account = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Viewer))
        .await;
    assign_workspace(fixture.pool(), fixture.workspace, account.id, role).await;
    account
}

/// A request a test sends as someone.
#[derive(Debug, Clone)]
struct Call {
    method: Method,
    path: String,
    body: Option<Value>,
}

impl Call {
    fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            body: None,
        }
    }

    fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    fn post(path: impl Into<String>, body: Value) -> Self {
        Self::new(Method::POST, path).with(body)
    }

    fn put(path: impl Into<String>, body: Value) -> Self {
        Self::new(Method::PUT, path).with(body)
    }

    fn patch(path: impl Into<String>, body: Value) -> Self {
        Self::new(Method::PATCH, path).with(body)
    }

    fn delete(path: impl Into<String>) -> Self {
        Self::new(Method::DELETE, path)
    }

    fn with(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    async fn send(&self, fixture: &Fixture, token: &str) -> TestResponse {
        let builder = Request::builder()
            .method(self.method.clone())
            .uri(&self.path)
            .header("Authorization", format!("Bearer {token}"));
        let request = match &self.body {
            Some(body) => builder
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        }
        .expect("the request builds");
        fixture.sso.client.send_request(request).await
    }
}

fn describe(call: &Call, response: &TestResponse) -> String {
    format!(
        "{} {} answered {}: {}",
        call.method,
        call.path,
        response.status,
        response.text()
    )
}

fn code(response: &TestResponse) -> Value {
    serde_json::from_slice::<Value>(response.bytes())
        .map(|body| body["code"].clone())
        .unwrap_or(Value::Null)
}

fn refused_with(response: &TestResponse, expected: &str) -> bool {
    response.status == StatusCode::FORBIDDEN && code(response) == expected
}
