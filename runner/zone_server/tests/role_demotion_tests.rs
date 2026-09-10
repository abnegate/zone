//! `update_member_role` guarded only the role being *granted*, never the role
//! being taken away. `remove_member` had always checked both -- a non-owner
//! cannot remove an admin or owner, and the last owner cannot be removed at
//! all -- so the same tenant could be dismantled through the role route that
//! the delete route refused.
//!
//! Written from the attacker's side: a real admin, invited legitimately, uses
//! the role route to strip the owner that invited them.

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

struct Account {
    token: String,
    user: String,
}

async fn account(client: &TestClient) -> Account {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    Account {
        token: body["access_token"]
            .as_str()
            .expect("registration returns an access token")
            .to_string(),
        user: body["user"]["id"]
            .as_str()
            .expect("registration returns the user")
            .to_string(),
    }
}

async fn organization(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": uuid::Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization is created")
        .to_string()
}

async fn workspace(client: &TestClient, token: &str, organization: &str) -> String {
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Tenant workspace", "slug": uuid::Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace is created")
        .to_string()
}

async fn role_of(client: &TestClient, token: &str, organization: &str, user: &str) -> String {
    let response = client
        .get_auth(&format!("/api/organizations/{organization}/members"), token)
        .await;
    response.json_value()["members"]
        .as_array()
        .expect("members are listed")
        .iter()
        .find(|member| member["user_id"] == user)
        .unwrap_or_else(|| panic!("{user} is no longer a member"))["role"]
        .as_str()
        .expect("a member has a role")
        .to_string()
}

#[tokio::test]
async fn an_organization_admin_cannot_demote_the_owner_that_invited_them() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .patch_json_auth(
            &format!("/api/organizations/{organization}/members/{}", owner.user),
            &json!({ "role": "member" }),
            &deputy.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "an admin demoted the organization's owner: {}",
        response.text()
    );
    assert_eq!(
        role_of(&client, &owner.token, &organization, &owner.user).await,
        "owner",
        "the owner lost the organization to one of its admins"
    );
}

#[tokio::test]
async fn the_last_organization_owner_cannot_demote_themselves_into_a_locked_tenant() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    let response = client
        .patch_json_auth(
            &format!("/api/organizations/{organization}/members/{}", owner.user),
            &json!({ "role": "member" }),
            &owner.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "the only owner demoted themselves, and granting owner needs an owner: {}",
        response.text()
    );
    assert_eq!(
        role_of(&client, &owner.token, &organization, &owner.user).await,
        "owner",
        "the organization was left with no owner at all"
    );
}

#[tokio::test]
async fn a_workspace_admin_cannot_demote_the_workspace_owner() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization = organization(&client, &owner.token).await;
    let workspace = workspace(&client, &owner.token, &organization).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .patch_json_auth(
            &format!("/api/workspaces/{workspace}/members/{}", owner.user),
            &json!({ "role": "viewer" }),
            &deputy.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "a workspace admin demoted the workspace owner: {}",
        response.text()
    );
}

#[tokio::test]
async fn an_owner_still_promotes_and_demotes_the_admins_below_them() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    client
        .patch_json_auth(
            &format!("/api/organizations/{organization}/members/{}", deputy.user),
            &json!({ "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        role_of(&client, &owner.token, &organization, &deputy.user).await,
        "admin"
    );

    client
        .patch_json_auth(
            &format!("/api/organizations/{organization}/members/{}", deputy.user),
            &json!({ "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        role_of(&client, &owner.token, &organization, &deputy.user).await,
        "member"
    );
}

#[tokio::test]
async fn an_owner_can_step_down_once_another_owner_stands() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let successor = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": successor.user, "role": "owner" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    client
        .patch_json_auth(
            &format!("/api/organizations/{organization}/members/{}", owner.user),
            &json!({ "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(
        role_of(&client, &successor.token, &organization, &successor.user).await,
        "owner",
        "the successor should still hold the organization"
    );
    assert_eq!(
        role_of(&client, &successor.token, &organization, &owner.user).await,
        "member"
    );
}

#[tokio::test]
async fn a_workspace_owner_still_demotes_an_admin_while_another_admin_stands() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization = organization(&client, &owner.token).await;
    let workspace = workspace(&client, &owner.token, &organization).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    client
        .patch_json_auth(
            &format!("/api/workspaces/{workspace}/members/{}", deputy.user),
            &json!({ "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn an_organization_admin_cannot_seat_a_new_owner() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let mole = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": mole.user, "role": "owner" }),
            &deputy.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "an admin seated a brand new owner, which the role route refuses: {}",
        response.text()
    );
}

#[tokio::test]
async fn a_workspace_admin_can_be_removed_again_by_the_workspace_owner() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization = organization(&client, &owner.token).await;
    let workspace = workspace(&client, &owner.token, &organization).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .delete_auth(
            &format!("/api/workspaces/{workspace}/members/{}", deputy.user),
            &owner.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "the workspace's creator could not remove an admin they had added: {}",
        response.text()
    );
}

#[tokio::test]
async fn an_organization_admin_cannot_invite_a_new_owner() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({
                "email": test_email(),
                "workspace_ids": [],
                "org_role": "owner",
                "workspace_role": "member"
            }),
            &deputy.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "an admin invited an owner, and acceptance seats the invited role: {}",
        response.text()
    );
}

#[tokio::test]
async fn an_owner_still_invites_the_roles_they_are_entitled_to_grant() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    for (org_role, workspace_role) in [("owner", "owner"), ("admin", "admin"), ("member", "viewer")]
    {
        client
            .post_json_auth(
                &format!("/api/organizations/{organization}/invitations"),
                &json!({
                    "email": test_email(),
                    "workspace_ids": [],
                    "org_role": org_role,
                    "workspace_role": workspace_role
                }),
                &owner.token,
            )
            .await
            .assert_status(StatusCode::CREATED);
    }
}

#[tokio::test]
async fn an_organization_admin_cannot_seat_another_admin_they_could_never_unseat() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let recruit = account(&client).await;
    let organization = organization(&client, &owner.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": recruit.user, "role": "admin" }),
            &deputy.token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "an admin seated an admin that only an owner could remove: {}",
        response.text()
    );

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": recruit.user, "role": "member" }),
            &deputy.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}
