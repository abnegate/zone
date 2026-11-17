//! A chat can opt out of public web, at creation or from its header later.

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

async fn update_chat(
    client: &TestClient,
    token: &str,
    chat_id: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = client
        .put_json_auth(&format!("/api/chats/{chat_id}"), &body, token)
        .await;
    (response.status, response.json_value())
}

async fn fetch_chat(client: &TestClient, token: &str, chat_id: &str) -> Value {
    let response = client
        .get_auth(&format!("/api/chats/{chat_id}"), token)
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()["chat"].clone()
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

#[tokio::test]
async fn update_takes_a_chat_offline_and_back_online() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Switchable chat",
            "model_name": "gpt-4",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let chat_id = body["chat"]["id"].as_str().expect("an id").to_string();

    let (status, body) = update_chat(&client, &token, &chat_id, json!({ "offline": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chat"]["offline"], true);
    assert_eq!(
        fetch_chat(&client, &token, &chat_id).await["offline"],
        true,
        "going offline must survive a reload"
    );

    let (status, body) = update_chat(&client, &token, &chat_id, json!({ "offline": false })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chat"]["offline"], false);
    assert_eq!(
        fetch_chat(&client, &token, &chat_id).await["offline"],
        false,
        "coming back online must survive a reload"
    );
}

#[tokio::test]
async fn update_without_offline_leaves_it_alone() {
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
    let chat_id = body["chat"]["id"].as_str().expect("an id").to_string();

    let (status, body) =
        update_chat(&client, &token, &chat_id, json!({ "title": "Renamed" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["chat"]["offline"], true,
        "an update that says nothing about offline must not bring the chat back online"
    );
}
