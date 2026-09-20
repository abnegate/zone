//! Attaching sources to a chat: the attachment round-trips, stays inside the
//! chat's workspace, and is what a retrieval for that chat is confined to.
mod common;

use axum::http::StatusCode;
use common::{TestClient, seed_chat};
use serde_json::json;
use uuid::Uuid;
use zone_server::agent::tools::{ChatTools, WorkspaceScope};
use zone_server::db::{chat_attached_sources, knowledge};

async fn create_source(client: &TestClient, token: &str, workspace: &str, name: &str) -> Uuid {
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/sources"),
            &json!({
                "name": format!("{name} {}", Uuid::new_v4()),
                "source_type": "filesystem",
                "config": { "base_path": "/tmp/attached" }
            }),
            token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    Uuid::parse_str(response.json_value()["source"]["id"].as_str().unwrap()).unwrap()
}

fn attached_ids(body: &serde_json::Value) -> Vec<Uuid> {
    body["sources"]
        .as_array()
        .expect("sources is a list")
        .iter()
        .map(|source| Uuid::parse_str(source["id"].as_str().unwrap()).unwrap())
        .collect()
}

#[tokio::test]
async fn attached_sources_round_trip_and_replace_the_previous_set() {
    let client = TestClient::with_db().await;
    let (token, chat, workspace) = seed_chat(&client, "llama3.1").await;
    let repository = create_source(&client, &token, &workspace, "Repository").await;
    let notes = create_source(&client, &token, &workspace, "Notes").await;

    let empty = client
        .get_auth(&format!("/api/chats/{chat}/sources"), &token)
        .await;
    empty.assert_status(StatusCode::OK);
    assert!(attached_ids(&empty.json_value()).is_empty());

    let attached = client
        .put_json_auth(
            &format!("/api/chats/{chat}/sources"),
            &json!({ "source_ids": [repository, notes] }),
            &token,
        )
        .await;
    attached.assert_status(StatusCode::OK);
    let body = attached.json_value();
    let mut ids = attached_ids(&body);
    ids.sort();
    let mut expected = vec![repository, notes];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(body["sources"][0]["source_type"], "filesystem");
    assert!(body["sources"][0]["name"].as_str().is_some());
    assert!(body["sources"][0]["attached_at"].as_str().is_some());

    let listed = client
        .get_auth(&format!("/api/chats/{chat}/sources"), &token)
        .await;
    let mut listed_ids = attached_ids(&listed.json_value());
    listed_ids.sort();
    assert_eq!(listed_ids, expected, "the attachment survives a reload");

    let narrowed = client
        .put_json_auth(
            &format!("/api/chats/{chat}/sources"),
            &json!({ "source_ids": [notes] }),
            &token,
        )
        .await;
    narrowed.assert_status(StatusCode::OK);
    assert_eq!(attached_ids(&narrowed.json_value()), vec![notes]);

    let pool = client.state().db();
    assert_eq!(
        chat_attached_sources::scope(pool, Some(Uuid::parse_str(&chat).unwrap())).await,
        Some(vec![notes]),
        "a retrieval for this chat is confined to what is attached"
    );

    let cleared = client
        .put_json_auth(
            &format!("/api/chats/{chat}/sources"),
            &json!({ "source_ids": [] }),
            &token,
        )
        .await;
    cleared.assert_status(StatusCode::OK);
    assert!(attached_ids(&cleared.json_value()).is_empty());
    assert_eq!(
        chat_attached_sources::scope(pool, Some(Uuid::parse_str(&chat).unwrap())).await,
        None,
        "nothing attached searches the whole workspace"
    );
}

#[tokio::test]
async fn a_source_from_another_workspace_is_refused_and_nothing_is_attached() {
    let client = TestClient::with_db().await;
    let (token, chat, workspace) = seed_chat(&client, "llama3.1").await;
    let (other_token, _other_chat, other_workspace) = seed_chat(&client, "llama3.1").await;
    let ours = create_source(&client, &token, &workspace, "Ours").await;
    let theirs = create_source(&client, &other_token, &other_workspace, "Theirs").await;

    let refused = client
        .put_json_auth(
            &format!("/api/chats/{chat}/sources"),
            &json!({ "source_ids": [ours, theirs] }),
            &token,
        )
        .await;
    refused.assert_status(StatusCode::BAD_REQUEST);
    let error = refused.json_value()["error"].as_str().unwrap().to_string();
    assert!(error.contains(&theirs.to_string()), "{error}");
    assert!(!error.contains(&ours.to_string()), "{error}");

    let listed = client
        .get_auth(&format!("/api/chats/{chat}/sources"), &token)
        .await;
    assert!(
        attached_ids(&listed.json_value()).is_empty(),
        "a refused write attaches nothing"
    );
}

#[tokio::test]
async fn a_stranger_can_neither_read_nor_change_the_attachment() {
    let client = TestClient::with_db().await;
    let (token, chat, workspace) = seed_chat(&client, "llama3.1").await;
    let (stranger, _, _) = seed_chat(&client, "llama3.1").await;
    let source = create_source(&client, &token, &workspace, "Ours").await;

    let read = client
        .get_auth(&format!("/api/chats/{chat}/sources"), &stranger)
        .await;
    assert_ne!(read.status, StatusCode::OK);

    let write = client
        .put_json_auth(
            &format!("/api/chats/{chat}/sources"),
            &json!({ "source_ids": [source] }),
            &stranger,
        )
        .await;
    assert_ne!(write.status, StatusCode::OK);

    let listed = client
        .get_auth(&format!("/api/chats/{chat}/sources"), &token)
        .await;
    assert!(attached_ids(&listed.json_value()).is_empty());
}

/// The chat's own search tool honours the attachment: a knowledge entry the
/// whole workspace finds is out of reach once the chat is pinned to a source,
/// and the tool says why rather than answering an empty list.
#[tokio::test]
async fn an_attachment_confines_what_the_chat_tool_searches() {
    let client = TestClient::with_db().await;
    let (token, chat, workspace) = seed_chat(&client, "llama3.1").await;
    let pool = client.state().db().clone();
    let workspace_id = Uuid::parse_str(&workspace).unwrap();
    let chat_id = Uuid::parse_str(&chat).unwrap();
    let user_id: Uuid =
        sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id = $1 LIMIT 1")
            .bind(workspace_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    knowledge::create_knowledge(
        &pool,
        workspace_id,
        "Harbour bridge facts",
        "The harbour bridge opened in 1932 and carries eight lanes.",
        None,
        &[],
        12,
        user_id,
    )
    .await
    .unwrap();

    let tools = ChatTools::build(WorkspaceScope {
        state: client.state().clone(),
        workspace_id,
        chat_id: Some(chat_id),
        user_id,
    })
    .await;
    let arguments = json!({"query": "harbour bridge"}).to_string();

    let open = tools.execute("search_knowledge", &arguments).await;
    let found = open.output.clone().unwrap_or_default();
    assert!(
        found.contains("1932"),
        "nothing attached searches the whole workspace: {open:?}"
    );

    let repository = create_source(&client, &token, &workspace, "Repository").await;
    client
        .put_json_auth(
            &format!("/api/chats/{chat}/sources"),
            &json!({ "source_ids": [repository] }),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let confined = tools.execute("search_knowledge", &arguments).await;
    let answer = confined.output.clone().unwrap_or_default();
    assert!(
        !answer.contains("1932"),
        "an attached source confines the search: {confined:?}"
    );
    assert!(
        answer.contains("1 source(s) attached to this chat"),
        "{answer}"
    );
}
