//! `create_invitation` validated the roles it was asked to grant but never the
//! workspaces it was asked to grant them in. `revoke_invitation` in the same
//! file has always bound the invitation to the caller's organization -- "Invitation
//! does not belong to this organization" -- so the create route handed out
//! membership of workspaces the delete route would not even let it see.
//!
//! Written from the attacker's side: an organization the attacker owns outright
//! is used as a lever to seat an accomplice inside a stranger's workspace.

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

struct Account {
    token: String,
    email: String,
}

async fn account(client: &TestClient) -> Account {
    let email = test_email();
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": email, "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    Account {
        token: body["access_token"]
            .as_str()
            .expect("registration returns an access token")
            .to_string(),
        email,
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

#[tokio::test]
async fn an_invitation_cannot_seat_its_guest_in_another_tenants_workspace() {
    let client = TestClient::with_db().await;

    let victim = account(&client).await;
    let victim_org = organization(&client, &victim.token).await;
    let victim_workspace = workspace(&client, &victim.token, &victim_org).await;

    let attacker = account(&client).await;
    let attacker_org = organization(&client, &attacker.token).await;
    let accomplice = account(&client).await;

    let invited = client
        .post_json_auth(
            &format!("/api/organizations/{attacker_org}/invitations"),
            &json!({
                "email": accomplice.email,
                "workspace_ids": [victim_workspace],
                "org_role": "member",
                "workspace_role": "owner",
            }),
            &attacker.token,
        )
        .await;

    let token = invited.json_value()["token"].as_str().map(str::to_string);
    if let Some(token) = token {
        client
            .post_json_auth(
                &format!("/api/invitations/{token}/accept"),
                &json!({}),
                &accomplice.token,
            )
            .await;
    }

    let reached = client
        .get_auth(
            &format!("/api/workspaces/{victim_workspace}/members"),
            &accomplice.token,
        )
        .await;
    assert_eq!(
        reached.status,
        StatusCode::FORBIDDEN,
        "the accomplice reached inside another tenant's workspace: {}",
        reached.text()
    );

    let deleted = client
        .delete_auth(
            &format!("/api/workspaces/{victim_workspace}"),
            &accomplice.token,
        )
        .await;
    assert_eq!(
        deleted.status,
        StatusCode::FORBIDDEN,
        "the accomplice deleted another tenant's workspace: {}",
        deleted.text()
    );

    // Asserted last so a regression reports what the accomplice reached rather
    // than only that the invitation was accepted.
    assert_eq!(
        invited.status,
        StatusCode::BAD_REQUEST,
        "an invitation named a workspace outside the inviting organization: {}",
        invited.text()
    );
}

#[tokio::test]
async fn an_owner_can_still_invite_into_a_workspace_of_their_own_organization() {
    let client = TestClient::with_db().await;

    let owner = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let own_workspace = workspace(&client, &owner.token, &org).await;
    let guest = account(&client).await;

    let invited = client
        .post_json_auth(
            &format!("/api/organizations/{org}/invitations"),
            &json!({
                "email": guest.email,
                "workspace_ids": [own_workspace],
                "org_role": "member",
                "workspace_role": "member",
            }),
            &owner.token,
        )
        .await;
    assert_eq!(
        invited.status,
        StatusCode::CREATED,
        "an owner was refused an invitation into their own workspace: {}",
        invited.text()
    );

    let token = invited.json_value()["token"]
        .as_str()
        .expect("a fresh invitation carries its token")
        .to_string();
    client
        .post_json_auth(
            &format!("/api/invitations/{token}/accept"),
            &json!({}),
            &guest.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let reached = client
        .get_auth(
            &format!("/api/workspaces/{own_workspace}/members"),
            &guest.token,
        )
        .await;
    assert_eq!(
        reached.status,
        StatusCode::OK,
        "the invited guest cannot see the workspace they were invited into: {}",
        reached.text()
    );
}

#[tokio::test]
async fn an_invitation_with_no_workspaces_is_still_accepted() {
    let client = TestClient::with_db().await;

    let owner = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let guest = account(&client).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{org}/invitations"),
            &json!({
                "email": guest.email,
                "workspace_ids": [],
                "org_role": "member",
                "workspace_role": "member",
            }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}
