mod common;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::{
    auth::{AuthUser, jwt::Claims},
    db::{sources, tasks},
    routes::tasks::{CreateTaskRequest, create},
};

#[tokio::test]
async fn creation_preserves_source_and_rejects_other_workspaces() {
    let pool = common::create_test_pool().await;
    let (_, workspace_id, user_id) = common::setup_test_data(&pool).await;
    let (_, other_workspace_id, _) = common::setup_test_data(&pool).await;
    let source = sources::create_source(
        &pool,
        workspace_id,
        "Repository",
        "github",
        json!({}),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let state = common::create_test_state(common::test_config(), pool.clone());
    for (workspace, source_id, expected) in [
        (workspace_id, Some(source.id), StatusCode::CREATED),
        (workspace_id, None, StatusCode::CREATED),
        (other_workspace_id, Some(source.id), StatusCode::BAD_REQUEST),
        (workspace_id, Some(Uuid::new_v4()), StatusCode::BAD_REQUEST),
    ] {
        let request: CreateTaskRequest = serde_json::from_value(json!({
            "title": "Task creation regression", "description": "Selected code source", "is_agentic": true, "source_id": source_id
        })).unwrap();
        let auth = AuthUser(Claims {
            sub: user_id.to_string(),
            email: "test@example.com".into(),
            roles: vec![],
            permissions: vec![],
            exp: 0,
            iat: 0,
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        });
        let response = create(State(state.clone()), auth, Path(workspace), Json(request))
            .await
            .into_response();
        assert_eq!(response.status(), expected);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        if expected == StatusCode::CREATED {
            assert_eq!(body["task"]["source_id"], json!(source_id));
            assert_eq!(body["task"]["model_name"], Value::Null);
            assert_eq!(body["task"]["source_ids"], json!([]));
            let id = Uuid::parse_str(body["task"]["id"].as_str().unwrap()).unwrap();
            let persisted = tasks::get_task(&pool, id).await.unwrap().unwrap();
            assert_eq!(persisted.source_id, source_id);
        }
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE workspace_id = $1 OR workspace_id = $2",
    )
    .bind(workspace_id)
    .bind(other_workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2, "rejected source IDs must not create tasks");
}
