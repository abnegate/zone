//! A new chat can choose how large a local model's context window is.

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
    format!("chat-context-{}", Uuid::new_v4())
}

async fn workspace(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Chat Context Org", "slug": slug() }),
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
            &json!({ "name": "Chat Context Workspace", "slug": slug() }),
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
async fn create_binds_context_tokens_and_list_returns_it() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "32k chat",
            "model_name": "gpt-4",
            "context_tokens": 32768,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let chat = &body["chat"];
    assert_eq!(chat["context_tokens"], 32768);

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
    assert_eq!(found["context_tokens"], 32768);
}

#[tokio::test]
async fn create_without_context_tokens_omits_the_choice() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Default chat",
            "model_name": "gpt-4",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body["chat"]["context_tokens"].is_null());
}

#[tokio::test]
async fn create_rejects_a_non_positive_context_size() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Zero context",
            "model_name": "gpt-4",
            "context_tokens": 0,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Context size must be a positive token count");
}

#[tokio::test]
async fn update_changes_the_chosen_context_size() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Resizable",
            "model_name": "gpt-4",
            "context_tokens": 8192,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["chat"]["id"].as_str().expect("chat id");

    let updated = client
        .put_json_auth(
            &format!("/api/chats/{id}"),
            &json!({ "context_tokens": 16384 }),
            &token,
        )
        .await;
    updated.assert_status(StatusCode::OK);
    assert_eq!(updated.json_value()["chat"]["context_tokens"], 16384);
}
