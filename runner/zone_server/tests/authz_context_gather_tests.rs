//! `POST /api/context/gather` and `POST /api/workspaces/{id}/sources/{id}/reindex`
//! both set a source's indexing running. Reindex asks for a workspace writer;
//! gather asked only whether the caller was a member at all, so the read-only
//! role could start the same work.

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

async fn seat(client: &TestClient, token: &str, workspace: &str, user: &str, role: &str) {
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
async fn a_workspace_viewer_cannot_start_a_context_gathering() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let viewer = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let ws = workspace(&client, &owner.token, &org).await;
    seat(&client, &owner.token, &ws, &viewer.user, "viewer").await;

    let refused_the_sibling = client
        .post_json_auth(
            &format!(
                "/api/workspaces/{ws}/sources/{}/reindex",
                uuid::Uuid::new_v4()
            ),
            &json!({}),
            &viewer.token,
        )
        .await;
    assert_eq!(
        refused_the_sibling.status,
        StatusCode::FORBIDDEN,
        "reindex admitted the read-only role: {}",
        refused_the_sibling.text()
    );

    let gathered = client
        .post_json_auth(
            "/api/context/gather",
            &json!({ "workspace_id": ws, "source_ids": [] }),
            &viewer.token,
        )
        .await;
    assert_eq!(
        gathered.status,
        StatusCode::FORBIDDEN,
        "the read-only role started the work reindex refuses it: {}",
        gathered.text()
    );
}

#[tokio::test]
async fn a_workspace_member_can_still_start_a_context_gathering() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let hand = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let ws = workspace(&client, &owner.token, &org).await;
    seat(&client, &owner.token, &ws, &hand.user, "member").await;

    let gathered = client
        .post_json_auth(
            "/api/context/gather",
            &json!({ "workspace_id": ws, "source_ids": [] }),
            &hand.token,
        )
        .await;
    assert!(
        gathered.status.is_success(),
        "a workspace member was refused a gathering: {} {}",
        gathered.status,
        gathered.text()
    );
}

#[tokio::test]
async fn a_workspace_viewer_can_still_search_the_context() {
    let client = TestClient::with_db().await;
    let owner = account(&client).await;
    let viewer = account(&client).await;
    let org = organization(&client, &owner.token).await;
    let ws = workspace(&client, &owner.token, &org).await;
    seat(&client, &owner.token, &ws, &viewer.user, "viewer").await;

    let searched = client
        .get_auth(
            &format!("/api/context/search?q=anything&workspace_id={ws}"),
            &viewer.token,
        )
        .await;
    // The embedding backend is not up in this environment, so the search
    // itself cannot complete -- but a 503 is raised past the guard, which is
    // what this pins: the read-only role is not turned away.
    assert_ne!(
        searched.status,
        StatusCode::FORBIDDEN,
        "the read-only role lost the read it is for: {}",
        searched.text()
    );
}
