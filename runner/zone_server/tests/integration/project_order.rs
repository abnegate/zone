//! A workspace keeps its projects in an order its members drag them into, and
//! lists each with when a member last worked in it.

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
    format!("project-order-{}", Uuid::new_v4())
}

async fn workspace(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Project Order Org", "slug": slug() }),
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
            &json!({ "name": "Project Order Workspace", "slug": slug() }),
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

async fn listed(client: &TestClient, token: &str, workspace_id: &str) -> Vec<Value> {
    let response = client
        .get_auth(&format!("/api/projects?workspace_id={workspace_id}"), token)
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()["projects"]
        .as_array()
        .expect("the list must carry a `projects` array")
        .clone()
}

fn names(projects: &[Value]) -> Vec<&str> {
    projects
        .iter()
        .map(|project| project["name"].as_str().expect("a project name"))
        .collect()
}

async fn reorder(
    client: &TestClient,
    token: &str,
    workspace_id: &str,
    order: &[&str],
) -> StatusCode {
    client
        .put_json_auth(
            &format!("/api/workspaces/{workspace_id}/projects/order"),
            &json!({ "project_ids": order }),
            token,
        )
        .await
        .status
}

#[tokio::test]
async fn a_dragged_order_is_listed_with_new_projects_first() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let alpha = project(&client, &token, &workspace_id, "Alpha").await;
    let beta = project(&client, &token, &workspace_id, "Beta").await;
    let gamma = project(&client, &token, &workspace_id, "Gamma").await;
    assert_eq!(
        names(&listed(&client, &token, &workspace_id).await),
        ["Gamma", "Beta", "Alpha"]
    );

    let status = reorder(&client, &token, &workspace_id, &[&alpha, &gamma, &beta]).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let projects = listed(&client, &token, &workspace_id).await;
    assert_eq!(names(&projects), ["Alpha", "Gamma", "Beta"]);
    let positions: Vec<i64> = projects
        .iter()
        .map(|project| project["position"].as_i64().expect("a position"))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "positions must rise down the list: {positions:?}"
    );
    project(&client, &token, &workspace_id, "Delta").await;
    assert_eq!(
        names(&listed(&client, &token, &workspace_id).await),
        ["Delta", "Alpha", "Gamma", "Beta"]
    );
}

#[tokio::test]
async fn an_order_naming_a_project_twice_is_refused() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let alpha = project(&client, &token, &workspace_id, "Alpha").await;
    project(&client, &token, &workspace_id, "Beta").await;

    let status = reorder(&client, &token, &workspace_id, &[&alpha, &alpha]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        names(&listed(&client, &token, &workspace_id).await),
        ["Beta", "Alpha"]
    );
}

#[tokio::test]
async fn an_order_cannot_move_another_workspaces_projects() {
    let client = TestClient::with_db().await;
    let owner = register(&client).await;
    let outsider = register(&client).await;
    let workspace_id = workspace(&client, &owner).await;
    let other_workspace = workspace(&client, &outsider).await;
    let alpha = project(&client, &owner, &workspace_id, "Alpha").await;
    let beta = project(&client, &owner, &workspace_id, "Beta").await;
    let foreign = project(&client, &outsider, &other_workspace, "Foreign").await;

    assert_eq!(
        reorder(&client, &outsider, &workspace_id, &[&alpha, &beta]).await,
        StatusCode::NOT_FOUND,
        "a stranger to the workspace is told it does not exist"
    );
    assert_eq!(
        reorder(&client, &outsider, &other_workspace, &[&foreign, &alpha]).await,
        StatusCode::BAD_REQUEST,
        "a project of another workspace cannot be pulled into this one's order"
    );
    assert_eq!(
        names(&listed(&client, &owner, &workspace_id).await),
        ["Beta", "Alpha"]
    );
}

#[tokio::test]
async fn a_project_lists_when_a_member_last_typed_in_it() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace_id = workspace(&client, &token).await;
    let project_id = project(&client, &token, &workspace_id, "Worked").await;
    let projects = listed(&client, &token, &workspace_id).await;
    assert!(projects[0]["last_used_at"].is_null());

    let chat: Uuid = sqlx::query_scalar(
        "INSERT INTO chats (workspace_id, title, model_name, project_id) \
         VALUES ($1::uuid, 'Work', 'test', $2::uuid) RETURNING id",
    )
    .bind(&workspace_id)
    .bind(&project_id)
    .fetch_one(client.state().db())
    .await
    .expect("a chat in the project");
    for (metadata, at) in [
        (json!({}), "2026-11-01 09:00:00"),
        (json!({"source": "reminder"}), "2026-11-05 09:00:00"),
    ] {
        sqlx::query(
            "INSERT INTO messages (chat_id, role, content, metadata, created_at) \
             VALUES ($1, 'user', 'Hello', $2, $3::timestamp)",
        )
        .bind(chat)
        .bind(metadata)
        .bind(at)
        .execute(client.state().db())
        .await
        .expect("a message");
    }

    let projects = listed(&client, &token, &workspace_id).await;
    assert_eq!(projects[0]["last_used_at"], "2026-11-01T09:00:00+00:00");
}
