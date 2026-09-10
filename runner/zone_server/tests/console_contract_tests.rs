//! What the console sends, against what the routes accept.
//!
//! The console's organization "Add Member" form posts `{ email, role }`, while
//! the route deserialises `{ user_id, role }`. Nothing in either suite compared
//! the two, so the form could only ever have failed. The invitation form next
//! to it posts `workspace_id` against a required `workspace_ids`, and failed
//! the same way.

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

async fn owner(client: &TestClient) -> (String, String) {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let token = response.json_value()["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": uuid::Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization is created")
        .to_string();
    (token, organization)
}

#[tokio::test]
async fn the_console_can_add_an_organization_member_by_email() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;

    let invitee = test_email();
    client
        .post_json(
            "/api/auth/register",
            &json!({ "email": invitee, "password": test_password() }),
        )
        .await
        .assert_status(StatusCode::CREATED);

    // Exactly the body `AddOrgMemberRequest` in the console produces.
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "email": invitee, "role": "member" }),
            &token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the console's Add Member body was refused: {}",
        response.text()
    );
}

/// The console's workspace form names its invitee by id, and that shape has to
/// keep working unchanged.
#[tokio::test]
async fn the_console_can_still_add_an_organization_member_by_id() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;
    let invitee = account(&client).await;

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": invitee.user, "role": "member" }),
            &token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the id-shaped body stopped working: {}",
        response.text()
    );
}

/// The email path must not become a second, unguarded way in. An admin cannot
/// seat an admin by id, so it cannot seat one by email either.
#[tokio::test]
async fn an_admin_cannot_seat_an_admin_by_email() {
    let client = TestClient::with_db().await;
    let (owner_token, organization) = owner(&client).await;
    let deputy = account(&client).await;
    let outsider = account(&client).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    for role in ["admin", "owner"] {
        let response = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/members"),
                &json!({ "email": outsider.email, "role": role }),
                &deputy.token,
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "an admin seated a fresh {role} by email: {}",
            response.text()
        );
    }

    let members = client
        .get_auth(
            &format!("/api/organizations/{organization}/members"),
            &owner_token,
        )
        .await;
    assert!(
        !members.text().contains(&outsider.user),
        "the outsider was seated anyway: {}",
        members.text()
    );
}

/// A non-owner is refused before the email is ever resolved, so the guarded
/// roles cannot be used to ask whether an account exists.
#[tokio::test]
async fn an_admin_is_refused_before_a_guarded_role_resolves_an_email() {
    let client = TestClient::with_db().await;
    let (owner_token, organization) = owner(&client).await;
    let deputy = account(&client).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let known = account(&client).await;
    let unknown = test_email();

    let for_known = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "email": known.email, "role": "admin" }),
            &deputy.token,
        )
        .await;
    let for_unknown = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "email": unknown, "role": "admin" }),
            &deputy.token,
        )
        .await;

    assert_eq!(
        (for_known.status, for_known.text()),
        (for_unknown.status, for_unknown.text()),
        "an admin can tell a registered email from an unregistered one"
    );
}

/// Adding by email must say nothing about the tenants the account already
/// belongs to: a stranger's colleague and a stranger with no organization at
/// all are answered identically.
#[tokio::test]
async fn adding_by_email_discloses_nothing_about_other_tenants() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;

    let unaffiliated = account(&client).await;
    let elsewhere = account(&client).await;
    let (stranger_token, stranger_org) = owner(&client).await;
    client
        .post_json_auth(
            &format!("/api/organizations/{stranger_org}/members"),
            &json!({ "user_id": elsewhere.user, "role": "member" }),
            &stranger_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let mut answers = Vec::new();
    for email in [&unaffiliated.email, &elsewhere.email] {
        let response = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/members"),
                &json!({ "email": email, "role": "member" }),
                &token,
            )
            .await;
        answers.push(response.status);
    }

    assert_eq!(
        answers[0], answers[1],
        "an account already seated in another organization answers differently"
    );
    assert_eq!(answers[0], StatusCode::CREATED);
}

#[tokio::test]
async fn an_unregistered_email_is_pointed_at_the_invitation_route() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "email": test_email(), "role": "member" }),
            &token,
        )
        .await;

    response.assert_status(StatusCode::NOT_FOUND);
    assert!(
        response.text().to_lowercase().contains("invitation"),
        "the caller was not pointed at the invitation route: {}",
        response.text()
    );
}

#[tokio::test]
async fn a_member_must_be_named_exactly_once() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;
    let invitee = account(&client).await;

    for body in [
        json!({ "role": "member" }),
        json!({ "user_id": invitee.user, "email": invitee.email, "role": "member" }),
    ] {
        let response = client
            .post_json_auth(
                &format!("/api/organizations/{organization}/members"),
                &body,
                &token,
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "an ambiguous body was accepted: {body} -> {}",
            response.text()
        );
    }
}

struct Account {
    token: String,
    user: String,
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
        user: body["user"]["id"]
            .as_str()
            .expect("registration returns the user")
            .to_string(),
        email,
    }
}

/// `InvitationsSection` posts `{ email, org_role }`, and adds a singular
/// `workspace_id` and `workspace_role` only when a workspace is picked. The
/// route required a plural `workspace_ids` and a `workspace_role`, so every
/// invitation the console sent was rejected before a handler saw it.
#[tokio::test]
async fn the_console_can_create_an_invitation_without_naming_a_workspace() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({ "email": test_email(), "org_role": "member" }),
            &token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the console's invitation body was refused: {}",
        response.text()
    );
    assert_eq!(
        response.json_value()["workspace_ids"],
        json!([]),
        "an invitation with no workspace named picked one up"
    );
}

#[tokio::test]
async fn the_console_can_create_an_invitation_naming_one_workspace() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;
    let workspace = workspace(&client, &token, &organization).await;

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({
                "email": test_email(),
                "org_role": "member",
                "workspace_id": workspace,
                "workspace_role": "member",
            }),
            &token,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the console's single-workspace invitation body was refused: {}",
        response.text()
    );
    assert_eq!(
        response.json_value()["workspace_ids"],
        json!([workspace]),
        "the named workspace did not reach the invitation"
    );
}

/// The plural shape the API's own callers use has to keep working.
#[tokio::test]
async fn the_plural_invitation_shape_still_works() {
    let client = TestClient::with_db().await;
    let (token, organization) = owner(&client).await;
    let workspace = workspace(&client, &token, &organization).await;

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({
                "email": test_email(),
                "org_role": "member",
                "workspace_ids": [workspace],
                "workspace_role": "viewer",
            }),
            &token,
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the plural shape stopped working: {}",
        response.text()
    );
}

/// The singular form must not slip past the guards the plural form answers to.
#[tokio::test]
async fn a_singular_workspace_answers_to_the_same_guards() {
    let client = TestClient::with_db().await;
    let (owner_token, organization) = owner(&client).await;
    let deputy = account(&client).await;
    let (stranger_token, stranger_org) = owner(&client).await;
    let stranger_workspace = workspace(&client, &stranger_token, &stranger_org).await;
    let own_workspace = workspace(&client, &owner_token, &organization).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &owner_token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    let escalated = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({
                "email": test_email(),
                "org_role": "member",
                "workspace_id": own_workspace,
                "workspace_role": "admin",
            }),
            &deputy.token,
        )
        .await;
    assert_eq!(
        escalated.status,
        StatusCode::FORBIDDEN,
        "an admin invited a workspace admin through the singular form: {}",
        escalated.text()
    );

    let foreign = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({
                "email": test_email(),
                "org_role": "member",
                "workspace_id": stranger_workspace,
                "workspace_role": "member",
            }),
            &owner_token,
        )
        .await;
    assert_eq!(
        foreign.status,
        StatusCode::BAD_REQUEST,
        "the singular form named a workspace outside the organization: {}",
        foreign.text()
    );

    let both = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/invitations"),
            &json!({
                "email": test_email(),
                "org_role": "member",
                "workspace_id": own_workspace,
                "workspace_ids": [own_workspace],
                "workspace_role": "member",
            }),
            &owner_token,
        )
        .await;
    assert_eq!(
        both.status,
        StatusCode::BAD_REQUEST,
        "an ambiguous invitation body was accepted: {}",
        both.text()
    );
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
