//! A signed-up account can reach the console's tenant settings pages.
//!
//! The console has always gated its workspace and organization settings pages
//! on `workspaces:update` and `organizations:update`. Migration 001 seeded a
//! permission row for every other resource the console names and none for these
//! two, so no role could hold either and both pages answered "Access Denied" to
//! every user, including the owner of the organization. Migration 022 seeds them.
//!
//! Which tenant a caller may act on is decided per request from
//! `organization_members` and `workspace_members`; these grants only say that a
//! role administers tenants at all, which is why the refusals asserted below
//! still hold for a foreign tenant.

use crate::common::{TestClient, test_email, test_password};
use axum::http::StatusCode;
use serde_json::json;
use std::collections::BTreeSet;
use zone_server::db::users;

const PULL_REQUEST_AND_RELEASE_PERMISSIONS: [&str; 4] = [
    "pull_requests:read",
    "pull_requests:update",
    "releases:read",
    "releases:create",
];

const READ_PERMISSIONS: [&str; 2] = ["pull_requests:read", "releases:read"];

async fn register(client: &TestClient) -> serde_json::Value {
    register_as(client, &test_email()).await
}

async fn register_as(client: &TestClient, email: &str) -> serde_json::Value {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": email,
                "password": test_password(),
                "display_name": "Tenant Owner"
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()
}

fn permissions_of(body: &serde_json::Value) -> Vec<String> {
    body["permissions"]
        .as_array()
        .expect("the auth response returns the permission list the console gates on")
        .iter()
        .map(|permission| permission.as_str().unwrap_or_default().to_string())
        .collect()
}

async fn user_in_role(pool: &sqlx::PgPool, role: &str) -> uuid::Uuid {
    let user = users::create_user(pool, &test_email(), "password-hash", Some(role), false)
        .await
        .expect("a user is created");
    users::assign_user_role(pool, user.id, role)
        .await
        .expect("the role is assigned");
    user.id
}

async fn permissions_held(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> BTreeSet<String> {
    users::get_user_with_permissions(pool, user_id)
        .await
        .expect("permissions are read")
        .expect("the user exists")
        .permissions
        .into_iter()
        .collect()
}

#[tokio::test]
async fn a_new_account_may_administer_its_own_tenants() {
    let client = TestClient::with_db().await;

    let granted = permissions_of(&register(&client).await);

    for required in [
        "organizations:read",
        "organizations:create",
        "organizations:update",
        "workspaces:read",
        "workspaces:create",
        "workspaces:update",
    ] {
        assert!(
            granted.iter().any(|permission| permission == required),
            "{required} is missing, so the console refuses the settings page: {granted:?}"
        );
    }
}

#[tokio::test]
async fn deleting_a_tenant_is_not_granted_to_a_standard_account() {
    let client = TestClient::with_db().await;

    let granted = permissions_of(&register(&client).await);

    for withheld in ["organizations:delete", "workspaces:delete"] {
        assert!(
            !granted.iter().any(|permission| permission == withheld),
            "{withheld} was granted to a standard account: {granted:?}"
        );
    }
}

#[tokio::test]
async fn a_new_account_may_read_merge_and_release() {
    let client = TestClient::with_db().await;

    let granted = permissions_of(&register(&client).await);

    for required in PULL_REQUEST_AND_RELEASE_PERMISSIONS {
        assert!(
            granted.iter().any(|permission| permission == required),
            "{required} is missing, so the console hides the pull request and release pages: {granted:?}"
        );
    }
}

#[tokio::test]
async fn a_standard_role_holds_every_pull_request_and_release_permission() {
    let pool = crate::common::create_test_pool().await;
    let user_id = user_in_role(&pool, "user").await;

    let held = permissions_held(&pool, user_id).await;

    for required in PULL_REQUEST_AND_RELEASE_PERMISSIONS {
        assert!(
            held.contains(required),
            "{required} is missing from the user role: {held:?}"
        );
    }
}

#[tokio::test]
async fn a_viewer_may_only_read_pull_requests_and_releases() {
    let pool = crate::common::create_test_pool().await;
    let user_id = user_in_role(&pool, "viewer").await;

    let held = permissions_held(&pool, user_id).await;
    let granted: BTreeSet<&str> = PULL_REQUEST_AND_RELEASE_PERMISSIONS
        .into_iter()
        .filter(|permission| held.contains(*permission))
        .collect();

    assert_eq!(
        granted,
        BTreeSet::from(READ_PERMISSIONS),
        "a viewer reads pull requests and releases but never merges or releases"
    );
}

#[tokio::test]
async fn an_instance_admin_signs_in_holding_every_pull_request_and_release_permission() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let registered = register_as(&client, &email).await;
    let user_id = uuid::Uuid::parse_str(
        registered["user"]["id"]
            .as_str()
            .expect("registration names the user"),
    )
    .expect("the user id is a uuid");
    sqlx::query("UPDATE users SET is_admin = true WHERE id = $1")
        .bind(user_id)
        .execute(client.state().db())
        .await
        .expect("the account is made an instance admin");

    let response = client
        .post_json(
            "/api/auth/login",
            &json!({ "email": email, "password": test_password() }),
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();

    assert_eq!(body["user"]["is_admin"], json!(true));
    let granted = permissions_of(&body);
    for required in PULL_REQUEST_AND_RELEASE_PERMISSIONS {
        assert!(
            granted.iter().any(|permission| permission == required),
            "{required} is missing for an instance admin, who bypasses role grants: {granted:?}"
        );
    }
}

#[tokio::test]
async fn the_instance_admin_permission_list_names_every_seeded_permission() {
    let pool = crate::common::create_test_pool().await;
    let administrator = users::create_user(
        &pool,
        &test_email(),
        "password-hash",
        Some("Instance admin"),
        true,
    )
    .await
    .expect("an instance admin is created");

    let listed = permissions_held(&pool, administrator.id).await;
    let seeded: BTreeSet<String> = sqlx::query_scalar::<_, String>("SELECT name FROM permissions")
        .fetch_all(&pool)
        .await
        .expect("the seeded permissions are read")
        .into_iter()
        .collect();

    assert_eq!(
        listed, seeded,
        "an instance admin bypasses role grants, so the hard-coded list in db/users.rs must name every seeded permission"
    );
}
