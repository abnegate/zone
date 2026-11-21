//! The custom roles routes: who may read and change an organization's roles, how the license gates
//! changes, how a role is validated, when widening a role people hold is refused, and what every
//! change audits.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::audit::{actions, resources};
use zone_server::db::custom_roles::{self, Assignment, Creation, Definition};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use crate::common::TestResponse;
use crate::common::policy::Fixture;
use crate::common::sso::{Account, grace, lapsed};

const PREFIX: &str = "custom_roles";
const LICENSE_REQUIRED: &str = "license_required";
const UNVERIFIABLE: &str = "not a license key";
const NAME_INVALID: &str = "name_invalid";
const NAME_RESERVED: &str = "name_reserved";
const NAME_TAKEN: &str = "role_name_taken";
const IN_USE: &str = "role_in_use";
const UNKNOWN: &str = "permission_unknown";
const NOT_COMPOSABLE: &str = "permission_not_composable";
const OUT_OF_SCOPE: &str = "permission_out_of_scope";
const BODY_INVALID: &str = "body_invalid";
const SCOPE_INVALID: &str = "scope_invalid";
const ORGANIZATION_MISSING: &str = "Organization not found";
const ROLE_MISSING: &str = "Role not found";

fn roles_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/roles")
}

fn role_path(organization: Uuid, role: impl std::fmt::Display) -> String {
    format!("/api/organizations/{organization}/roles/{role}")
}

async fn licensed() -> Fixture {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license().await;
    fixture
}

async fn list(fixture: &Fixture, token: &str) -> TestResponse {
    fixture
        .sso
        .client
        .get_auth(&roles_path(fixture.organization), token)
        .await
}

async fn create(fixture: &Fixture, token: &str, body: &Value) -> TestResponse {
    fixture
        .sso
        .client
        .post_json_auth(&roles_path(fixture.organization), body, token)
        .await
}

async fn update(fixture: &Fixture, token: &str, role: Uuid, body: &Value) -> TestResponse {
    fixture
        .sso
        .client
        .patch_json_auth(&role_path(fixture.organization, role), body, token)
        .await
}

async fn delete(fixture: &Fixture, token: &str, role: Uuid) -> TestResponse {
    fixture
        .sso
        .client
        .delete_auth(&role_path(fixture.organization, role), token)
        .await
}

/// Creates a role as the owner and returns it, failing the test unless it was created.
async fn created(fixture: &Fixture, body: Value) -> Value {
    let response = create(fixture, &fixture.owner.token, &body).await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["role"].clone()
}

fn id(role: &Value) -> Uuid {
    role["id"].as_str().unwrap().parse().unwrap()
}

fn workspace_role(name: &str, permissions: &[&str]) -> Value {
    json!({ "name": name, "scope": "workspace", "permissions": permissions })
}

fn organization_role(name: &str, permissions: &[&str]) -> Value {
    json!({ "name": name, "scope": "organization", "permissions": permissions })
}

/// Stores a role without going through the routes, as one made while the license was active.
async fn stored(fixture: &Fixture, name: &str, scope: Scope, permissions: &[Permission]) -> Uuid {
    let names: Vec<String> = permissions
        .iter()
        .map(|permission| permission.name().to_owned())
        .collect();
    let grant = Grant::compose(scope, &names).unwrap();
    let definition = Definition {
        name,
        description: None,
        scope,
        grant: &grant,
    };
    match custom_roles::create(
        fixture.pool(),
        fixture.organization,
        &definition,
        fixture.owner.id,
    )
    .await
    .unwrap()
    {
        Creation::Created(role) => role.id,
        Creation::NameTaken => panic!("{name} is already taken"),
    }
}

async fn seat_with(fixture: &Fixture, workspace: Uuid, account: &Account, role: Uuid) {
    let change = workspace_members::change_role(
        fixture.pool(),
        workspace,
        account.id,
        Assignment::Custom(role),
        fixture.owner.id,
    )
    .await
    .unwrap();
    assert!(
        matches!(change, workspace_members::RoleChange::Applied(_)),
        "{change:?}"
    );
}

/// Registers an organization member who holds the workspace role `role` in `workspace`.
async fn holder(fixture: &Fixture, workspace: Uuid, role: Uuid) -> Account {
    let account = fixture.member(OrgRole::Member, None).await;
    fixture
        .seat(workspace, &account, WorkspaceRole::Viewer)
        .await;
    seat_with(fixture, workspace, &account, role).await;
    account
}

async fn give_organization_role(fixture: &Fixture, account: &Account, role: Uuid) {
    let change = organization_members::change_role(
        fixture.pool(),
        fixture.organization,
        account.id,
        Assignment::Custom(role),
        fixture.owner.id,
    )
    .await
    .unwrap();
    assert!(
        matches!(change, organization_members::RoleChange::Applied(_)),
        "{change:?}"
    );
}

async fn give_built_in(fixture: &Fixture, workspace: Uuid, account: &Account) {
    let change = workspace_members::change_role(
        fixture.pool(),
        workspace,
        account.id,
        Assignment::BuiltIn(WorkspaceRole::Member),
        fixture.owner.id,
    )
    .await
    .unwrap();
    assert!(
        matches!(change, workspace_members::RoleChange::Applied(_)),
        "{change:?}"
    );
}

async fn listed(fixture: &Fixture, role: Uuid) -> Value {
    let response = list(fixture, &fixture.owner.token).await;
    response.assert_status(StatusCode::OK);
    response.json_value()["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|listed| id(listed) == role)
        .cloned()
        .unwrap_or(Value::Null)
}

fn refused(response: &TestResponse, status: StatusCode, code: &str) {
    response.assert_status(status);
    assert_eq!(
        response.json_value()["code"],
        code,
        "unexpected refusal: {}",
        response.text()
    );
}

fn uncoded(response: &TestResponse, status: StatusCode) {
    response.assert_status(status);
    assert_eq!(
        response.json_value().get("code"),
        None,
        "an access refusal carries no role code: {}",
        response.text()
    );
}

fn names(permissions: &[Permission]) -> Value {
    let mut names: Vec<&str> = permissions
        .iter()
        .map(|permission| permission.name())
        .collect();
    names.sort_unstable();
    json!(names)
}

/// Every write the routes take, against `role`.
async fn writes(fixture: &Fixture, token: &str, role: Uuid) -> [TestResponse; 3] {
    [
        create(
            fixture,
            token,
            &workspace_role("Fresh", &["workspace:write"]),
        )
        .await,
        update(fixture, token, role, &json!({ "name": "Renamed" })).await,
        delete(fixture, token, role).await,
    ]
}

#[tokio::test]
async fn the_listing_carries_the_catalogue_and_the_license() {
    let fixture = licensed().await;

    let response = list(&fixture, &fixture.owner.token).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["entitled"], true);
    assert_eq!(body["editable"], true);
    assert_eq!(body["edition"], "enterprise");
    assert_eq!(body["status"], "active");
    assert_eq!(body["roles"], json!([]));
    let expected: Vec<Value> = Permission::ALL
        .into_iter()
        .filter(|permission| permission.scope() != Scope::Server)
        .map(|permission| {
            json!({
                "name": permission.name(),
                "scope": permission.scope().name(),
                "composable": permission.composable(),
            })
        })
        .collect();
    assert_eq!(body["catalogue"], json!(expected));
    fixture.cleanup().await;
}

#[tokio::test]
async fn community_reads_roles_and_cannot_change_them() {
    let fixture = Fixture::new(PREFIX).await;
    let role = stored(
        &fixture,
        "Writer",
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;

    let response = list(&fixture, &fixture.owner.token).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["entitled"], false);
    assert_eq!(body["editable"], false);
    assert_eq!(body["edition"], "community");
    assert_eq!(body["status"], "community");
    assert_eq!(body["roles"][0]["id"], role.to_string());

    for response in writes(&fixture, &fixture.owner.token, role).await {
        refused(&response, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
    }
    assert_eq!(listed(&fixture, role).await["name"], "Writer");
    for action in [
        actions::ROLE_CREATED,
        actions::ROLE_UPDATED,
        actions::ROLE_DELETED,
    ] {
        assert!(fixture.audits(action).await.is_empty(), "{action}");
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_license_in_its_grace_period_can_change_roles() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.install(&grace()).await;

    let body = list(&fixture, &fixture.owner.token).await.json_value();
    assert_eq!(body["status"], "grace");
    assert_eq!(body["edition"], "enterprise");
    assert_eq!(body["entitled"], true);
    assert_eq!(body["editable"], true);

    let role = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);
    update(
        &fixture,
        &fixture.owner.token,
        role,
        &json!({ "name": "Author" }),
    )
    .await
    .assert_status(StatusCode::OK);
    delete(&fixture, &fixture.owner.token, role)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_reads_roles_and_refuses_changes() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.store_license(&lapsed()).await;
    let role = stored(
        &fixture,
        "Writer",
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;

    let response = list(&fixture, &fixture.owner.token).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["status"], "lapsed");
    assert_eq!(body["edition"], "enterprise");
    assert_eq!(body["entitled"], false);
    assert_eq!(body["editable"], false);
    assert_eq!(body["roles"][0]["name"], "Writer");

    for response in writes(&fixture, &fixture.owner.token, role).await {
        refused(&response, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_key_that_fails_verification_refuses_changes() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.store_license(UNVERIFIABLE).await;
    let role = stored(&fixture, "Writer", Scope::Workspace, &[]).await;

    let body = list(&fixture, &fixture.owner.token).await.json_value();
    assert_eq!(body["entitled"], false);
    assert_eq!(body["editable"], false);
    for response in writes(&fixture, &fixture.owner.token, role).await {
        refused(&response, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_member_reads_roles_and_changes_none() {
    let fixture = licensed().await;
    let member = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Owner))
        .await;
    let role = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);

    let response = list(&fixture, &member.token).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["entitled"], true);
    assert_eq!(body["editable"], false);
    assert_eq!(body["roles"][0]["id"], role.to_string());

    for response in writes(&fixture, &member.token, role).await {
        uncoded(&response, StatusCode::FORBIDDEN);
        assert_eq!(
            response.json_value()["error"],
            Permission::RolesUpdate.refusal()
        );
    }
    assert_eq!(listed(&fixture, role).await["name"], "Writer");
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_creates_edits_and_deletes_roles() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;

    assert_eq!(
        list(&fixture, &admin.token).await.json_value()["editable"],
        true
    );
    let response = create(
        &fixture,
        &admin.token,
        &workspace_role("Writer", &["workspace:write"]),
    )
    .await;
    response.assert_status(StatusCode::CREATED);
    let role = id(&response.json_value()["role"]);
    update(&fixture, &admin.token, role, &json!({ "name": "Author" }))
        .await
        .assert_status(StatusCode::OK);
    delete(&fixture, &admin.token, role)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(listed(&fixture, role).await, Value::Null);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_organization_role_never_grants_roles_update() {
    let fixture = licensed().await;
    let role = id(&created(
        &fixture,
        organization_role("Recruiter", &["members:create", "invitations:create"]),
    )
    .await);
    let recruiter = fixture.member(OrgRole::Member, None).await;
    give_organization_role(&fixture, &recruiter, role).await;

    let body = list(&fixture, &recruiter.token).await.json_value();
    assert_eq!(body["editable"], false);
    for response in writes(&fixture, &recruiter.token, role).await {
        uncoded(&response, StatusCode::FORBIDDEN);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stranger_is_told_the_organization_does_not_exist() {
    let fixture = licensed().await;
    let stranger = fixture.stranger().await;
    let role = id(&created(&fixture, workspace_role("Writer", &[])).await);

    let mut responses = vec![list(&fixture, &stranger.token).await];
    responses.extend(writes(&fixture, &stranger.token, role).await);
    for response in responses {
        uncoded(&response, StatusCode::NOT_FOUND);
        assert_eq!(response.json_value()["error"], ORGANIZATION_MISSING);
    }

    let former = fixture.member(OrgRole::Admin, None).await;
    organization_members::remove_member(fixture.pool(), fixture.organization, former.id)
        .await
        .unwrap();
    uncoded(&list(&fixture, &former.token).await, StatusCode::NOT_FOUND);
    for response in writes(&fixture, &former.token, role).await {
        uncoded(&response, StatusCode::NOT_FOUND);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_role_of_another_organization_is_not_found() {
    let fixture = licensed().await;
    let (other, _) = fixture.another_organization(&fixture.owner).await;
    let response = fixture
        .sso
        .client
        .post_json_auth(
            &roles_path(other),
            &workspace_role("Elsewhere", &[]),
            &fixture.owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let elsewhere = id(&response.json_value()["role"]);

    for response in [
        update(
            &fixture,
            &fixture.owner.token,
            elsewhere,
            &json!({ "name": "Taken over" }),
        )
        .await,
        delete(&fixture, &fixture.owner.token, elsewhere).await,
        update(
            &fixture,
            &fixture.owner.token,
            Uuid::new_v4(),
            &json!({ "name": "Ghost" }),
        )
        .await,
        delete(&fixture, &fixture.owner.token, Uuid::new_v4()).await,
    ] {
        uncoded(&response, StatusCode::NOT_FOUND);
        assert_eq!(response.json_value()["error"], ROLE_MISSING);
    }
    for response in [
        fixture
            .sso
            .client
            .delete_auth(
                &role_path(fixture.organization, "not-a-role"),
                &fixture.owner.token,
            )
            .await,
        fixture
            .sso
            .client
            .patch_json_auth(
                &role_path(fixture.organization, "not-a-role"),
                &json!({ "name": "Ghost" }),
                &fixture.owner.token,
            )
            .await,
    ] {
        uncoded(&response, StatusCode::NOT_FOUND);
        assert_eq!(
            response.json_value()["error"],
            ROLE_MISSING,
            "a malformed role id in a real organization names no role"
        );
    }
    let response = fixture
        .sso
        .client
        .delete_auth(
            &format!("/api/organizations/not-an-organization/roles/{elsewhere}"),
            &fixture.owner.token,
        )
        .await;
    uncoded(&response, StatusCode::NOT_FOUND);
    assert_eq!(response.json_value()["error"], ORGANIZATION_MISSING);

    let survivor = fixture
        .sso
        .client
        .get_auth(&roles_path(other), &fixture.owner.token)
        .await
        .json_value();
    assert_eq!(survivor["roles"][0]["name"], "Elsewhere");
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_permission_is_checked_before_the_license() {
    let fixture = Fixture::new(PREFIX).await;
    let member = fixture.member(OrgRole::Member, None).await;
    let role = stored(&fixture, "Writer", Scope::Workspace, &[]).await;

    for response in writes(&fixture, &member.token, role).await {
        uncoded(&response, StatusCode::FORBIDDEN);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_license_is_checked_before_the_body_is_read() {
    let fixture = Fixture::new(PREFIX).await;
    let role = stored(&fixture, "Writer", Scope::Workspace, &[]).await;
    let token = &fixture.owner.token;

    for body in [
        json!({ "scope": "workspace" }),
        workspace_role("owner", &[]),
        workspace_role("Writer", &["nope"]),
    ] {
        refused(
            &create(&fixture, token, &body).await,
            StatusCode::FORBIDDEN,
            LICENSE_REQUIRED,
        );
    }
    for body in [json!({ "scope": "organization" }), json!({ "name": "" })] {
        refused(
            &update(&fixture, token, role, &body).await,
            StatusCode::FORBIDDEN,
            LICENSE_REQUIRED,
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_invalid_role_is_refused_with_its_code() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;

    let cases = [
        (workspace_role("", &[]), NAME_INVALID),
        (workspace_role("   ", &[]), NAME_INVALID),
        (workspace_role("owner", &[]), NAME_RESERVED),
        (workspace_role(" Admin ", &[]), NAME_RESERVED),
        (organization_role("MEMBER", &[]), NAME_RESERVED),
        (workspace_role("Viewer", &[]), NAME_RESERVED),
        (workspace_role("Writer", &["workspace:scribble"]), UNKNOWN),
        (
            workspace_role("Writer", &["workspace:delete"]),
            NOT_COMPOSABLE,
        ),
        (
            workspace_role("Writer", &["workspace_administrators:update"]),
            NOT_COMPOSABLE,
        ),
        (
            organization_role("Writer", &["roles:update"]),
            NOT_COMPOSABLE,
        ),
        (
            organization_role("Writer", &["administrators:update"]),
            NOT_COMPOSABLE,
        ),
        (
            organization_role("Writer", &["organization:delete"]),
            NOT_COMPOSABLE,
        ),
        (workspace_role("Writer", &["members:create"]), OUT_OF_SCOPE),
        (
            organization_role("Writer", &["workspace:write"]),
            OUT_OF_SCOPE,
        ),
        (
            organization_role("Writer", &["models:create"]),
            OUT_OF_SCOPE,
        ),
    ];
    for (body, code) in cases {
        let response = create(&fixture, token, &body).await;
        refused(&response, StatusCode::BAD_REQUEST, code);
        assert!(
            response.json_value()["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "{body}"
        );
    }

    for (body, code) in [
        (
            json!({ "scope": "workspace", "permissions": [] }),
            BODY_INVALID,
        ),
        (
            json!({ "name": "Writer", "scope": "workspace" }),
            BODY_INVALID,
        ),
        (
            json!({ "name": "Writer", "scope": "team", "permissions": [] }),
            BODY_INVALID,
        ),
        (
            json!({ "name": "Writer", "scope": "server", "permissions": [] }),
            SCOPE_INVALID,
        ),
        (
            json!({ "name": "Writer", "scope": "workspace", "permissions": [], "members": 3 }),
            BODY_INVALID,
        ),
    ] {
        refused(
            &create(&fixture, token, &body).await,
            StatusCode::BAD_REQUEST,
            code,
        );
    }

    let role = id(&created(&fixture, workspace_role("Writer", &[])).await);
    for (body, code) in [
        (json!({ "name": " " }), NAME_INVALID),
        (json!({ "name": "viewer" }), NAME_RESERVED),
        (json!({ "permissions": ["members:create"] }), OUT_OF_SCOPE),
        (
            json!({ "permissions": ["workspace:delete"] }),
            NOT_COMPOSABLE,
        ),
        (json!({ "permissions": ["nope"] }), UNKNOWN),
    ] {
        refused(
            &update(&fixture, token, role, &body).await,
            StatusCode::BAD_REQUEST,
            code,
        );
    }
    assert_eq!(listed(&fixture, role).await["name"], "Writer");
    assert_eq!(
        listed(&fixture, role).await["permissions"],
        names(&[Permission::WorkspaceRead])
    );
    assert!(fixture.audits(actions::ROLE_UPDATED).await.is_empty());
    assert_eq!(fixture.audits(actions::ROLE_CREATED).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_name_is_trimmed_and_the_base_permission_is_always_held() {
    let fixture = licensed().await;

    let workspace = created(
        &fixture,
        json!({
            "name": "  Writer  ",
            "description": "Writes in the workspace",
            "scope": "workspace",
            "permissions": ["workspace:write", "pull_requests:override"],
        }),
    )
    .await;
    assert_eq!(workspace["name"], "Writer");
    assert_eq!(workspace["description"], "Writes in the workspace");
    assert_eq!(workspace["scope"], "workspace");
    assert_eq!(
        workspace["organization_id"],
        fixture.organization.to_string()
    );
    assert_eq!(workspace["members"], 0);
    assert_eq!(
        workspace["permissions"],
        names(&[
            Permission::WorkspaceRead,
            Permission::WorkspaceWrite,
            Permission::PullRequestsOverride,
        ])
    );
    assert!(workspace["created_at"].is_string());
    assert!(workspace["updated_at"].is_string());

    let organization = created(
        &fixture,
        organization_role("Auditor", &["organization:read", "audit_logs:read"]),
    )
    .await;
    assert_eq!(organization["description"], Value::Null);
    assert_eq!(
        organization["permissions"],
        names(&[Permission::OrganizationRead, Permission::AuditLogsRead])
    );
    let bare = created(&fixture, organization_role("Bare", &[])).await;
    assert_eq!(bare["permissions"], names(&[Permission::OrganizationRead]));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_taken_name_is_refused_ignoring_case() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    created(&fixture, workspace_role("Reviewer", &[])).await;

    for name in ["Reviewer", "reviewer", " REVIEWER "] {
        refused(
            &create(&fixture, token, &organization_role(name, &[])).await,
            StatusCode::CONFLICT,
            NAME_TAKEN,
        );
    }
    let other = id(&created(&fixture, workspace_role("Author", &[])).await);
    refused(
        &update(&fixture, token, other, &json!({ "name": "rEVIEWER" })).await,
        StatusCode::CONFLICT,
        NAME_TAKEN,
    );
    assert_eq!(listed(&fixture, other).await["name"], "Author");

    update(&fixture, token, other, &json!({ "name": "author" }))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(listed(&fixture, other).await["name"], "author");
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sparse_edit_keeps_what_it_leaves_out_and_null_clears() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    let original = created(
        &fixture,
        json!({
            "name": "Writer",
            "description": "Writes",
            "scope": "workspace",
            "permissions": ["workspace:write"],
        }),
    )
    .await;
    let role = id(&original);

    let renamed = update(&fixture, token, role, &json!({ "name": "Author" })).await;
    renamed.assert_status(StatusCode::OK);
    let renamed = renamed.json_value()["role"].clone();
    assert_eq!(renamed["name"], "Author");
    assert_eq!(renamed["description"], "Writes");
    assert_eq!(renamed["permissions"], original["permissions"]);

    let cleared = update(&fixture, token, role, &json!({ "description": null })).await;
    cleared.assert_status(StatusCode::OK);
    let cleared = cleared.json_value()["role"].clone();
    assert_eq!(cleared["name"], "Author");
    assert_eq!(cleared["description"], Value::Null);
    assert_eq!(cleared["permissions"], original["permissions"]);

    let replaced = update(
        &fixture,
        token,
        role,
        &json!({ "permissions": ["workspace_ai_settings:update"] }),
    )
    .await;
    replaced.assert_status(StatusCode::OK);
    let replaced = replaced.json_value()["role"].clone();
    assert_eq!(
        replaced["permissions"],
        names(&[
            Permission::WorkspaceRead,
            Permission::WorkspaceAiSettingsUpdate
        ])
    );
    assert_eq!(replaced["name"], "Author");
    assert_eq!(replaced["description"], Value::Null);

    let untouched = update(&fixture, token, role, &json!({})).await;
    untouched.assert_status(StatusCode::OK);
    let untouched = untouched.json_value()["role"].clone();
    assert_eq!(untouched["name"], replaced["name"]);
    assert_eq!(untouched["description"], Value::Null);
    assert_eq!(untouched["permissions"], replaced["permissions"]);
    assert_eq!(fixture.audits(actions::ROLE_UPDATED).await.len(), 3);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_scope_cannot_be_changed() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    let role = id(&created(&fixture, workspace_role("Writer", &[])).await);

    for body in [
        json!({ "scope": "organization" }),
        json!({ "scope": "workspace" }),
        json!({ "name": "Author", "scope": "organization" }),
        json!({ "organization_id": Uuid::new_v4() }),
    ] {
        refused(
            &update(&fixture, token, role, &body).await,
            StatusCode::BAD_REQUEST,
            BODY_INVALID,
        );
    }
    let role = listed(&fixture, role).await;
    assert_eq!(role["scope"], "workspace");
    assert_eq!(role["name"], "Writer");
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_cannot_widen_a_workspace_role_people_hold() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;
    let role = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);
    holder(&fixture, fixture.workspace, role).await;

    for permissions in [
        json!(["workspace:write", "workspace_members:create"]),
        json!(["workspace_members:create"]),
    ] {
        let response = update(
            &fixture,
            &admin.token,
            role,
            &json!({ "permissions": permissions }),
        )
        .await;
        refused(&response, StatusCode::FORBIDDEN, custom_roles::EXCEEDS);
    }
    assert_eq!(
        listed(&fixture, role).await["permissions"],
        names(&[Permission::WorkspaceRead, Permission::WorkspaceWrite])
    );
    assert!(fixture.audits(actions::ROLE_UPDATED).await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_owner_widens_a_workspace_role_people_hold() {
    let fixture = licensed().await;
    let role = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);
    holder(&fixture, fixture.workspace, role).await;

    let response = update(
        &fixture,
        &fixture.owner.token,
        role,
        &json!({ "permissions": ["workspace:write", "workspace_members:create"] }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["role"]["permissions"],
        names(&[
            Permission::WorkspaceRead,
            Permission::WorkspaceWrite,
            Permission::WorkspaceMembersCreate,
        ])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_narrows_a_workspace_role_people_hold() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;
    let role = id(&created(
        &fixture,
        workspace_role("Writer", &["workspace:write", "project_syncs:update"]),
    )
    .await);
    holder(&fixture, fixture.workspace, role).await;

    let response = update(
        &fixture,
        &admin.token,
        role,
        &json!({ "permissions": ["workspace:write"] }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["role"]["permissions"],
        names(&[Permission::WorkspaceRead, Permission::WorkspaceWrite])
    );
    update(
        &fixture,
        &admin.token,
        role,
        &json!({ "name": "Author", "description": "Renamed while held" }),
    )
    .await
    .assert_status(StatusCode::OK);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_widens_a_workspace_role_nobody_holds() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;
    let role = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);
    let former = holder(&fixture, fixture.workspace, role).await;
    workspace_members::remove_member(fixture.pool(), fixture.workspace, former.id)
        .await
        .unwrap();
    assert_eq!(listed(&fixture, role).await["members"], 0);

    let response = update(
        &fixture,
        &admin.token,
        role,
        &json!({ "permissions": ["workspace:write", "workspace:update"] }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["role"]["permissions"],
        names(&[
            Permission::WorkspaceRead,
            Permission::WorkspaceWrite,
            Permission::WorkspaceUpdate,
        ])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_widens_an_organization_role_people_hold() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;
    let role = id(&created(
        &fixture,
        organization_role("Recruiter", &["members:create"]),
    )
    .await);
    let recruiter = fixture.member(OrgRole::Member, None).await;
    give_organization_role(&fixture, &recruiter, role).await;
    assert_eq!(listed(&fixture, role).await["members"], 1);

    let response = update(
        &fixture,
        &admin.token,
        role,
        &json!({ "permissions": ["members:create", "invitations:create"] }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value()["role"]["permissions"],
        names(&[
            Permission::OrganizationRead,
            Permission::MembersCreate,
            Permission::InvitationsCreate,
        ])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn members_counts_the_active_memberships_that_hold_the_role() {
    let fixture = licensed().await;
    let workspace = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);
    let organization = id(&created(&fixture, organization_role("Recruiter", &[])).await);
    let second = fixture.another_workspace().await;

    let first = holder(&fixture, fixture.workspace, workspace).await;
    fixture.seat(second, &first, WorkspaceRole::Viewer).await;
    seat_with(&fixture, second, &first, workspace).await;
    let other = holder(&fixture, second, workspace).await;
    give_organization_role(&fixture, &other, organization).await;

    assert_eq!(listed(&fixture, workspace).await["members"], 3);
    assert_eq!(listed(&fixture, organization).await["members"], 1);

    workspace_members::remove_member(fixture.pool(), second, first.id)
        .await
        .unwrap();
    assert_eq!(listed(&fixture, workspace).await["members"], 2);
    organization_members::remove_member(fixture.pool(), fixture.organization, other.id)
        .await
        .unwrap();
    assert_eq!(listed(&fixture, organization).await["members"], 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_role_in_use_is_kept_until_its_holders_are_given_another() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    let role = id(&created(&fixture, workspace_role("Writer", &["workspace:write"])).await);
    let second = fixture.another_workspace().await;
    let first = holder(&fixture, fixture.workspace, role).await;
    let other = holder(&fixture, second, role).await;

    let response = delete(&fixture, token, role).await;
    response.assert_status(StatusCode::CONFLICT);
    let body = response.json_value();
    assert_eq!(body["code"], IN_USE);
    assert_eq!(body["members"], 2);
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert_eq!(listed(&fixture, role).await["members"], 2);

    give_built_in(&fixture, fixture.workspace, &first).await;
    let response = delete(&fixture, token, role).await;
    response.assert_status(StatusCode::CONFLICT);
    assert_eq!(response.json_value()["members"], 1);

    give_built_in(&fixture, second, &other).await;
    delete(&fixture, token, role)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(listed(&fixture, role).await, Value::Null);
    assert_eq!(fixture.audits(actions::ROLE_DELETED).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_change_is_audited_with_the_old_and_new_values() {
    let fixture = licensed().await;
    let admin = fixture.member(OrgRole::Admin, None).await;
    let response = create(
        &fixture,
        &admin.token,
        &json!({
            "name": "Writer",
            "description": "Writes",
            "scope": "workspace",
            "permissions": ["workspace:write"],
        }),
    )
    .await;
    response.assert_status(StatusCode::CREATED);
    let role = id(&response.json_value()["role"]);
    let first = json!({
        "name": "Writer",
        "description": "Writes",
        "scope": "workspace",
        "permissions": names(&[Permission::WorkspaceRead, Permission::WorkspaceWrite]),
    });

    let creations = fixture.audits(actions::ROLE_CREATED).await;
    assert_eq!(creations.len(), 1);
    let creation = &creations[0];
    assert_eq!(creation.resource_type, resources::ROLE);
    assert_eq!(creation.resource_id, Some(role));
    assert_eq!(creation.organization_id, Some(fixture.organization));
    assert_eq!(creation.actor_id, Some(admin.id));
    assert_eq!(creation.old_values, None);
    assert_eq!(creation.new_values, Some(first.clone()));

    update(
        &fixture,
        &admin.token,
        role,
        &json!({ "name": "Author", "description": null, "permissions": [] }),
    )
    .await
    .assert_status(StatusCode::OK);
    let second = json!({
        "name": "Author",
        "description": null,
        "scope": "workspace",
        "permissions": names(&[Permission::WorkspaceRead]),
    });
    let updates = fixture.audits(actions::ROLE_UPDATED).await;
    assert_eq!(updates.len(), 1);
    let updated = &updates[0];
    assert_eq!(updated.resource_type, resources::ROLE);
    assert_eq!(updated.resource_id, Some(role));
    assert_eq!(updated.organization_id, Some(fixture.organization));
    assert_eq!(updated.actor_id, Some(admin.id));
    assert_eq!(updated.old_values, Some(first));
    assert_eq!(updated.new_values, Some(second.clone()));

    update(&fixture, &admin.token, role, &json!({ "name": "Author" }))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(fixture.audits(actions::ROLE_UPDATED).await.len(), 1);

    delete(&fixture, &admin.token, role)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let deletions = fixture.audits(actions::ROLE_DELETED).await;
    assert_eq!(deletions.len(), 1);
    let deletion = &deletions[0];
    assert_eq!(deletion.resource_type, resources::ROLE);
    assert_eq!(deletion.resource_id, Some(role));
    assert_eq!(deletion.organization_id, Some(fixture.organization));
    assert_eq!(deletion.actor_id, Some(admin.id));
    assert_eq!(deletion.old_values, Some(second));
    assert_eq!(deletion.new_values, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn refused_changes_are_not_audited() {
    let fixture = licensed().await;
    let token = &fixture.owner.token;
    let role = id(&created(&fixture, workspace_role("Writer", &[])).await);
    holder(&fixture, fixture.workspace, role).await;

    refused(
        &create(&fixture, token, &workspace_role("writer", &[])).await,
        StatusCode::CONFLICT,
        NAME_TAKEN,
    );
    refused(
        &delete(&fixture, token, role).await,
        StatusCode::CONFLICT,
        IN_USE,
    );
    assert_eq!(fixture.audits(actions::ROLE_CREATED).await.len(), 1);
    assert!(fixture.audits(actions::ROLE_DELETED).await.is_empty());
    fixture.cleanup().await;
}
