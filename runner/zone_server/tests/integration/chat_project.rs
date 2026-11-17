//! A chat can be bound to a workspace project, at creation or from its header
//! later. The list then carries that project so the console can group by it.

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
    format!("chat-project-{}", Uuid::new_v4())
}

async fn workspace(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Chat Project Org", "slug": slug() }),
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
            &json!({ "name": "Chat Project Workspace", "slug": slug() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace create must return a wrapped `workspace` object")
        .to_string()
}

async fn project(client: &TestClient, token: &str, workspace_id: &str, name: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({ "workspace_id": workspace_id, "name": name }),
            token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["project"]["id"]
        .as_str()
        .expect("project create must return a wrapped `project` object")
        .to_string()
}

async fn create_chat(client: &TestClient, token: &str, body: Value) -> (StatusCode, Value) {
    let response = client.post_json_auth("/api/chats", &body, token).await;
    (response.status, response.json_value())
}

async fn chat_in(client: &TestClient, token: &str, workspace_id: &str, body: Value) -> String {
    let mut request = json!({
        "workspace_id": workspace_id,
        "title": "Movable chat",
        "model_name": "gpt-4",
    });
    let map = request.as_object_mut().expect("request is an object");
    for (key, value) in body.as_object().expect("overrides are an object") {
        map.insert(key.clone(), value.clone());
    }
    let (status, body) = create_chat(client, token, request).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["chat"]["id"].as_str().expect("an id").to_string()
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
async fn create_binds_a_workspace_project_and_list_returns_it() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let project_id = project(&client, &token, &workspace_id, "Alpha").await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Bound chat",
            "model_name": "gpt-4",
            "project_id": project_id,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let chat = &body["chat"];
    assert_eq!(chat["project_id"], project_id);

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
    assert_eq!(found["project_id"], project_id);
}

#[tokio::test]
async fn create_rejects_a_project_from_another_workspace() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let other_workspace = workspace(&client, &token).await;
    let foreign = project(&client, &token, &other_workspace, "Foreign").await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Should fail",
            "model_name": "gpt-4",
            "project_id": foreign,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Project not found in this workspace"),
        "{body}"
    );
}

#[tokio::test]
async fn create_without_a_project_omits_project_id() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let (status, body) = create_chat(
        &client,
        &token,
        json!({
            "workspace_id": workspace_id,
            "title": "Loose chat",
            "model_name": "gpt-4",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body["chat"]["project_id"].is_null());
}

#[tokio::test]
async fn update_joins_moves_and_leaves_a_project() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let alpha = project(&client, &token, &workspace_id, "Alpha").await;
    let beta = project(&client, &token, &workspace_id, "Beta").await;
    let chat_id = chat_in(&client, &token, &workspace_id, json!({})).await;

    let (status, body) =
        update_chat(&client, &token, &chat_id, json!({ "project_id": alpha })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chat"]["project_id"], alpha);
    assert_eq!(
        fetch_chat(&client, &token, &chat_id).await["project_id"],
        alpha,
        "joining a project must survive a reload"
    );

    let (status, body) =
        update_chat(&client, &token, &chat_id, json!({ "project_id": beta })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chat"]["project_id"], beta, "{body}");

    let (status, body) =
        update_chat(&client, &token, &chat_id, json!({ "clear_project": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["chat"]["project_id"].is_null(),
        "clearing takes the chat out of its project: {body}"
    );
    assert!(
        fetch_chat(&client, &token, &chat_id).await["project_id"].is_null(),
        "leaving a project must survive a reload"
    );
}

#[tokio::test]
async fn update_without_a_project_field_leaves_the_project_alone() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let alpha = project(&client, &token, &workspace_id, "Alpha").await;
    let chat_id = chat_in(
        &client,
        &token,
        &workspace_id,
        json!({ "project_id": alpha }),
    )
    .await;

    let (status, body) =
        update_chat(&client, &token, &chat_id, json!({ "title": "Renamed" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["chat"]["project_id"], alpha,
        "an update that says nothing about the project must not unbind it"
    );
}

#[tokio::test]
async fn update_rejects_a_project_from_another_workspace() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let other_workspace = workspace(&client, &token).await;
    let foreign = project(&client, &token, &other_workspace, "Foreign").await;
    let chat_id = chat_in(&client, &token, &workspace_id, json!({})).await;

    let (status, body) =
        update_chat(&client, &token, &chat_id, json!({ "project_id": foreign })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Project not found in this workspace"),
        "{body}"
    );
    assert!(
        fetch_chat(&client, &token, &chat_id).await["project_id"].is_null(),
        "a refused move must leave the chat where it was"
    );
}

#[tokio::test]
async fn update_rejects_naming_a_project_and_clearing_it_at_once() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let alpha = project(&client, &token, &workspace_id, "Alpha").await;
    let chat_id = chat_in(&client, &token, &workspace_id, json!({})).await;

    let (status, body) = update_chat(
        &client,
        &token,
        &chat_id,
        json!({ "project_id": alpha, "clear_project": true }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        fetch_chat(&client, &token, &chat_id).await["project_id"].is_null(),
        "a contradictory request must change nothing"
    );
}
