//! The console's contract with the organization endpoints, pinned from the
//! live pass of 2026-09-20: an invitation names its workspace and inviter, a
//! removed member can be invited again, members are listed by email, every
//! organization carries the caller's role, organization AI settings are read
//! by administrators only, changes are audited, and every organization has a
//! subscription to show.

mod common;

use axum::http::StatusCode;
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{TestClient, test_email, test_password};
use zone_server::db::organization_members::{self, OrgRole};

struct Account {
    token: String,
    user: Uuid,
    email: String,
}

async fn register(client: &TestClient) -> Account {
    let email = test_email();
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": email, "password": test_password(), "display_name": "Console" }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Account {
        token: body["access_token"].as_str().unwrap().to_string(),
        user: Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap(),
        email,
    }
}

async fn organization(client: &TestClient, owner: &Account) -> Uuid {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Contract Org", "slug": format!("contract-{}", Uuid::new_v4()) }),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    Uuid::parse_str(
        response.json_value()["organization"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

async fn workspace(client: &TestClient, owner: &Account, org: Uuid) -> (Uuid, String) {
    let name = format!("Workspace {}", Uuid::new_v4());
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{org}/workspaces"),
            &json!({ "name": name, "slug": format!("ws-{}", Uuid::new_v4()) }),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let id = Uuid::parse_str(response.json_value()["workspace"]["id"].as_str().unwrap()).unwrap();
    (id, name)
}

async fn seat(client: &TestClient, owner: &Account, org: Uuid, member: &Account, role: &str) {
    client
        .post_json_auth(
            &format!("/api/organizations/{org}/members"),
            &json!({ "user_id": member.user, "role": role }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

async fn invite(client: &TestClient, owner: &Account, org: Uuid, body: Value) -> Value {
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{org}/invitations"),
            &body,
            &owner.token,
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the invitation was refused: {}",
        response.text()
    );
    response.json_value()
}

#[tokio::test]
async fn an_invitation_names_its_workspace_and_its_inviter_in_console_time() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let org = organization(&client, &owner).await;
    let (ws, ws_name) = workspace(&client, &owner, org).await;

    let invitation = invite(
        &client,
        &owner,
        org,
        json!({ "email": test_email(), "org_role": "member", "workspace_id": ws }),
    )
    .await;
    let token = invitation["token"].as_str().unwrap();

    let response = client.get(&format!("/api/invitations/{token}")).await;
    response.assert_status(StatusCode::OK);
    let details = response.json_value();
    assert_eq!(details["workspace_name"], json!(ws_name), "{details}");
    assert_eq!(details["invited_by_email"], json!(owner.email), "{details}");
    let expires_at = details["expires_at"].as_str().unwrap();
    assert!(
        expires_at.ends_with('Z'),
        "expires_at must be a Z-suffixed instant for the console: {expires_at}"
    );
}

#[tokio::test]
async fn a_removed_member_can_be_invited_again() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let invitee = register(&client).await;
    let org = organization(&client, &owner).await;

    let first = invite(
        &client,
        &owner,
        org,
        json!({ "email": invitee.email, "org_role": "member" }),
    )
    .await;
    client
        .post_json_auth(
            &format!(
                "/api/invitations/{}/accept",
                first["token"].as_str().unwrap()
            ),
            &json!({}),
            &invitee.token,
        )
        .await
        .assert_status(StatusCode::OK);
    client
        .delete_auth(
            &format!("/api/organizations/{org}/members/{}", invitee.user),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);

    invite(
        &client,
        &owner,
        org,
        json!({ "email": invitee.email, "org_role": "member" }),
    )
    .await;
}

#[tokio::test]
async fn members_are_listed_with_their_emails_and_names() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let org = organization(&client, &owner).await;

    let response = client
        .get_auth(&format!("/api/organizations/{org}/members"), &owner.token)
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let members = body["members"].as_array().unwrap();
    assert_eq!(members.len(), 1, "{body}");
    assert_eq!(members[0]["email"], json!(owner.email), "{body}");
    assert_eq!(members[0]["display_name"], json!("Console"), "{body}");
}

#[tokio::test]
async fn every_organization_carries_the_callers_role() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let hand = register(&client).await;
    let org = organization(&client, &owner).await;
    seat(&client, &owner, org, &hand, "member").await;

    let listed = client.get_auth("/api/organizations", &hand.token).await;
    listed.assert_status(StatusCode::OK);
    let body = listed.json_value();
    let seat = body["organizations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|organization| organization["id"] == json!(org.to_string()))
        .unwrap_or_else(|| panic!("the member's organization is missing: {body}"));
    assert_eq!(seat["role"], json!("member"), "{body}");

    let single = client
        .get_auth(&format!("/api/organizations/{org}"), &owner.token)
        .await;
    single.assert_status(StatusCode::OK);
    assert_eq!(single.json_value()["organization"]["role"], json!("owner"));
}

#[tokio::test]
async fn a_plain_member_cannot_read_the_organizations_ai_settings() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let hand = register(&client).await;
    let org = organization(&client, &owner).await;
    seat(&client, &owner, org, &hand, "member").await;

    let refused = client
        .get_auth(
            &format!("/api/organizations/{org}/settings/ai"),
            &hand.token,
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a member read the organization's AI settings: {}",
        refused.text()
    );
    client
        .get_auth(
            &format!("/api/organizations/{org}/settings/ai"),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn a_role_change_is_written_to_the_audit_log() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let hand = register(&client).await;
    let org = organization(&client, &owner).await;
    seat(&client, &owner, org, &hand, "member").await;

    client
        .patch_json_auth(
            &format!("/api/organizations/{org}/members/{}", hand.user),
            &json!({ "role": "admin" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let today = Utc::now().format("%Y-%m-%d").to_string();
    let response = client
        .get_auth(
            &format!("/api/organizations/{org}/audit-logs?start_date={today}&end_date={today}"),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let logs = body["logs"].as_array().unwrap();
    let change = logs
        .iter()
        .find(|log| log["action"] == json!("member.role_changed"))
        .unwrap_or_else(|| panic!("the role change was not audited: {body}"));
    assert_eq!(change["actor_email"], json!(owner.email), "{change}");
    assert_eq!(change["resource_type"], json!("member"), "{change}");
    assert_eq!(
        change["resource_id"],
        json!(hand.user.to_string()),
        "{change}"
    );
    assert_eq!(change["new_values"]["role"], json!("admin"), "{change}");
    assert!(
        logs.iter()
            .any(|log| log["action"] == json!("member.added")),
        "seating the member was not audited: {body}"
    );
}

#[tokio::test]
async fn every_organization_has_a_subscription_to_show() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let org = organization(&client, &owner).await;

    let response = client
        .get_auth(
            &format!("/api/organizations/{org}/subscription"),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["subscription"]["plan_name"], json!("Free"), "{body}");
    assert_eq!(body["subscription"]["status"], json!("active"), "{body}");

    let legacy: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind("Legacy Org")
            .bind(format!("legacy-{}", Uuid::new_v4()))
            .fetch_one(client.state().db())
            .await
            .unwrap();
    organization_members::add_member(
        client.state().db(),
        legacy,
        owner.user,
        OrgRole::Owner,
        None,
    )
    .await
    .unwrap();

    let response = client
        .get_auth(
            &format!("/api/organizations/{legacy}/subscription"),
            &owner.token,
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::OK,
        "an organization without a subscription row shows nothing: {}",
        response.text()
    );
    assert_eq!(
        response.json_value()["subscription"]["plan_name"],
        json!("Free")
    );

    let usage = client
        .get_auth(&format!("/api/organizations/{legacy}/usage"), &owner.token)
        .await;
    usage.assert_status(StatusCode::OK);
    let usage = usage.json_value();
    assert_eq!(usage["usage"]["members"], json!(1), "{usage}");
    assert_eq!(usage["usage"]["workspaces"], json!(0), "{usage}");
    assert_eq!(usage["usage"]["chat_messages"], json!(0), "{usage}");

    let limits = client
        .get_auth(&format!("/api/organizations/{legacy}/limits"), &owner.token)
        .await;
    limits.assert_status(StatusCode::OK);
    assert_eq!(limits.json_value()["max_members"], json!(3));
}
