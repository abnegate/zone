//! A new chat can opt out of public web for its lifetime.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::{TestClient, test_email, test_password};

async fn register(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    response.json_value()["access_token"]
        .as_str()
        .expect("registration must return an access token")
        .to_string()
}

fn slug() -> String {
    format!("chat-offline-{}", Uuid::new_v4())
}

async fn workspace(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Chat Offline Org", "slug": slug() }),
            token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization create must return a wrapped `organization` object")
        .to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Chat Offline Workspace", "slug": slug() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace create must return a wrapped `workspace` object")
        .to_string()
}

async fn create_chat(client: &TestClient, token: &str, body: Value) -> (StatusCode, Value) {
    let response = client.post_json_auth("/api/chats", &body, token).await;
    (response.status, response.json_value())
}

#[tokio::test]
async fn create_binds_offline_and_list_returns_it() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Offline chat",
            "model_name": "gpt-4",
            "offline": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let chat = &body["chat"];
    assert_eq!(chat["offline"], true);

    let listed = client
        .get_auth(&format!("/api/chats?workspace_id={workspace_id}"), &token)
        .await;
    listed.assert_status(StatusCode::OK);
    let listed_body = listed.json_value();
    let chats = listed_body["chats"].as_array().expect("chats list");
    let found = chats
        .iter()
        .find(|item| item["id"] == chat["id"])
        .expect("created chat is listed");
    assert_eq!(found["offline"], true);
}

#[tokio::test]
async fn create_without_offline_stays_online() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Live chat",
            "model_name": "gpt-4",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["chat"]["offline"], false);
}
