//! `remove_member` refuses a workspace admin who tries to unseat another admin
//! or the owner, and refuses to remove the last admin at all. `delete_workspace`
//! unseats every member at once -- owner included -- and asked only for a
//! workspace admin. The organization sibling has always demanded an owner to
//! destroy a tenant.
//!
//! Written from the attacker's side: an admin the owner seated deletes the
//! workspace out from under them.

use axum::http::StatusCode;
use serde_json::json;

use crate::common::{TestClient, test_email, test_password};

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

async fn seat(
    client: &TestClient,
    token: &str,
    organization: &str,
    workspace: &str,
    user: &str,
    role: &str,
) {
    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": user, "role": "member" }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": user, "role": role }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn a_workspace_admin_cannot_delete_the_workspace_they_were_seated_in() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization_id = organization(&client, &owner.token).await;
    let workspace_id = workspace(&client, &owner.token, &organization_id).await;
    seat(
        &client,
        &owner.token,
        &organization_id,
        &workspace_id,
        &deputy.user,
        "admin",
    )
    .await;

    let refused = client
        .delete_auth(
            &format!("/api/workspaces/{workspace_id}/members/{}", owner.user),
            &deputy.token,
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the admin unseated the owner one member at a time: {}",
        refused.text()
    );

    let deleted = client
        .delete_auth(&format!("/api/workspaces/{workspace_id}"), &deputy.token)
        .await;
    assert_eq!(
        deleted.status,
        StatusCode::FORBIDDEN,
        "the admin destroyed the whole workspace instead: {}",
        deleted.text()
    );

    let survives = client
        .get_auth(&format!("/api/workspaces/{workspace_id}"), &owner.token)
        .await;
    assert_eq!(
        survives.status,
        StatusCode::OK,
        "the owner lost the workspace to one of its admins: {}",
        survives.text()
    );
}

#[tokio::test]
async fn a_workspace_owner_can_still_delete_their_own_workspace() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let organization_id = organization(&client, &owner.token).await;
    let workspace_id = workspace(&client, &owner.token, &organization_id).await;

    client
        .delete_auth(&format!("/api/workspaces/{workspace_id}"), &owner.token)
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_workspace_admin_can_still_rename_the_workspace() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let organization_id = organization(&client, &owner.token).await;
    let workspace_id = workspace(&client, &owner.token, &organization_id).await;
    seat(
        &client,
        &owner.token,
        &organization_id,
        &workspace_id,
        &deputy.user,
        "admin",
    )
    .await;

    let renamed = client
        .patch_json_auth(
            &format!("/api/workspaces/{workspace_id}"),
            &json!({ "name": "Renamed by the deputy" }),
            &deputy.token,
        )
        .await;
    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "an admin lost the administration the role is for: {}",
        renamed.text()
    );
}
