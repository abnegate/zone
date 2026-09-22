//! The chat routes carry `agent_sandboxed` — whether the agent keeps its own
//! file and shell tools — as an axis of its own, independent of auto-approval.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::{TestClient, test_email, test_password};

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
    format!("sandbox-{}", uuid::Uuid::new_v4())
}

async fn workspace(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Sandbox Org", "slug": slug() }),
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
            &json!({ "name": "Sandbox Workspace", "slug": slug() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace create must return a wrapped `workspace` object")
        .to_string()
}

async fn create_chat(client: &TestClient, token: &str, workspace_id: &str, body: Value) -> Value {
    let mut request = json!({
        "workspace_id": workspace_id,
        "title": "Sandbox Chat",
        "model_name": "gpt-4",
    });
    let map = request.as_object_mut().expect("request is an object");
    for (key, value) in body.as_object().expect("overrides are an object") {
        map.insert(key.clone(), value.clone());
    }
    let response = client.post_json_auth("/api/chats", &request, token).await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["chat"].clone()
}

async fn update_chat(client: &TestClient, token: &str, chat_id: &str, body: Value) -> Value {
    let response = client
        .put_json_auth(&format!("/api/chats/{chat_id}"), &body, token)
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()["chat"].clone()
}

async fn fetch_chat(client: &TestClient, token: &str, chat_id: &str) -> Value {
    let response = client
        .get_auth(&format!("/api/chats/{chat_id}"), token)
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()["chat"].clone()
}

#[tokio::test]
async fn chat_creation_defaults_to_sandboxed() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let chat = create_chat(&client, &token, &workspace_id, json!({})).await;

    assert_eq!(
        chat["agent_sandboxed"], true,
        "a chat created without an opinion must confine the agent to zone's tools"
    );
}

#[tokio::test]
async fn chat_creation_honours_an_unsandboxed_agent() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;

    let chat = create_chat(
        &client,
        &token,
        &workspace_id,
        json!({ "agent_enabled": true, "agent_sandboxed": false }),
    )
    .await;
    assert_eq!(chat["agent_sandboxed"], false);

    let stored = fetch_chat(&client, &token, chat["id"].as_str().unwrap()).await;
    assert_eq!(
        stored["agent_sandboxed"], false,
        "the choice made at creation must survive a reload"
    );
}

#[tokio::test]
async fn update_round_trips_agent_sandboxed() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let chat = create_chat(&client, &token, &workspace_id, json!({})).await;
    let chat_id = chat["id"].as_str().unwrap().to_string();

    let updated = update_chat(
        &client,
        &token,
        &chat_id,
        json!({ "agent_sandboxed": false }),
    )
    .await;
    assert_eq!(
        updated["agent_sandboxed"], false,
        "the update response must report the value it was asked to store"
    );
    assert_eq!(
        fetch_chat(&client, &token, &chat_id).await["agent_sandboxed"],
        false,
        "leaving the sandbox must be persisted, not just echoed"
    );

    let restored = update_chat(
        &client,
        &token,
        &chat_id,
        json!({ "agent_sandboxed": true }),
    )
    .await;
    assert_eq!(restored["agent_sandboxed"], true);
    assert_eq!(
        fetch_chat(&client, &token, &chat_id).await["agent_sandboxed"],
        true
    );
}

#[tokio::test]
async fn an_update_that_omits_the_sandbox_leaves_it_alone() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let chat = create_chat(
        &client,
        &token,
        &workspace_id,
        json!({ "agent_sandboxed": false }),
    )
    .await;
    let chat_id = chat["id"].as_str().unwrap().to_string();

    let updated = update_chat(&client, &token, &chat_id, json!({ "title": "Renamed" })).await;

    assert_eq!(updated["title"], "Renamed");
    assert_eq!(
        updated["agent_sandboxed"], false,
        "an update that says nothing about the sandbox must not re-sandbox the chat"
    );
}

#[tokio::test]
async fn the_sandbox_and_auto_approval_move_independently() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let chat = create_chat(
        &client,
        &token,
        &workspace_id,
        json!({ "agent_enabled": true }),
    )
    .await;
    let chat_id = chat["id"].as_str().unwrap().to_string();
    assert_eq!(chat["agent_sandboxed"], true);
    assert_eq!(chat["auto_approve"], false);

    let approving = update_chat(&client, &token, &chat_id, json!({ "auto_approve": true })).await;
    assert_eq!(
        approving["agent_sandboxed"], true,
        "auto-approving zone's own tools must not hand the agent its own"
    );

    let unsandboxed = update_chat(
        &client,
        &token,
        &chat_id,
        json!({ "agent_sandboxed": false }),
    )
    .await;
    assert_eq!(
        unsandboxed["auto_approve"], true,
        "leaving the sandbox must not disturb the approval policy for zone's tools"
    );
    assert_eq!(unsandboxed["agent_sandboxed"], false);
}
