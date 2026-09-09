mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::json;
use tokio::time::timeout;
use uuid::Uuid;
use zone_server::db::actions::{self, StartTask};
use zone_server::db::tasks;
use zone_server::services::checkout::Repository;

use common::{
    TestClient, create_test_pool, create_test_router, create_test_state, test_config, test_email,
    test_password,
};

struct Tenant {
    user: Uuid,
    token: String,
    workspace: Uuid,
}

async fn tenant(client: &TestClient) -> Tenant {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    let user = Uuid::parse_str(body["user"]["id"].as_str().expect("user id is returned"))
        .expect("user id is a UUID");
    let token = body["access_token"]
        .as_str()
        .expect("access token is returned")
        .to_owned();

    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Task authorization", "slug": Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    organization.assert_status(StatusCode::CREATED);
    let organization = organization.json_value()["organization"]["id"]
        .as_str()
        .expect("organization id is returned")
        .to_owned();

    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Task authorization", "slug": Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    workspace.assert_status(StatusCode::CREATED);
    let workspace = Uuid::parse_str(
        workspace.json_value()["workspace"]["id"]
            .as_str()
            .expect("workspace id is returned"),
    )
    .expect("workspace id is a UUID");

    Tenant {
        user,
        token,
        workspace,
    }
}

async fn project(client: &TestClient, owner: &Tenant) -> Uuid {
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({
                "workspace_id": owner.workspace,
                "name": format!("Project {}", Uuid::new_v4()),
            }),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    Uuid::parse_str(
        response.json_value()["project"]["id"]
            .as_str()
            .expect("project id is returned"),
    )
    .expect("project id is a UUID")
}

async fn task(client: &TestClient, owner: &Tenant, project: Uuid) -> Uuid {
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/tasks", owner.workspace),
            &json!({
                "project_ids": [project],
                "title": "Protected task",
                "description": "Private work",
            }),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    Uuid::parse_str(
        response.json_value()["task"]["id"]
            .as_str()
            .expect("task id is returned"),
    )
    .expect("task id is a UUID")
}

fn assert_project_rejected(response: &common::TestResponse) {
    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json_value(),
        json!({ "error": "Project is not available in this workspace" })
    );
}

fn assert_task_hidden(response: &common::TestResponse) {
    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(response.json_value(), json!({ "error": "Task not found" }));
}

#[tokio::test]
async fn task_creation_requires_projects_from_its_workspace() {
    let client = TestClient::with_db().await;
    let owner = tenant(&client).await;
    let foreign = tenant(&client).await;
    let own_project = project(&client, &owner).await;
    let foreign_project = project(&client, &foreign).await;
    let nonexistent_project = Uuid::new_v4();

    task(&client, &owner, own_project).await;

    let rejected = client
        .post_json_auth(
            &format!("/api/workspaces/{}/tasks", owner.workspace),
            &json!({
                "project_ids": [foreign_project],
                "title": "Foreign project",
                "description": "Must not be created",
            }),
            &owner.token,
        )
        .await;
    assert_project_rejected(&rejected);

    let missing = client
        .post_json_auth(
            &format!("/api/workspaces/{}/tasks", owner.workspace),
            &json!({
                "project_ids": [nonexistent_project],
                "title": "Missing project",
                "description": "Must not be created",
            }),
            &owner.token,
        )
        .await;
    assert_project_rejected(&missing);
}

#[tokio::test]
async fn task_update_requires_projects_from_its_workspace() {
    let client = TestClient::with_db().await;
    let owner = tenant(&client).await;
    let foreign = tenant(&client).await;
    let own_project = project(&client, &owner).await;
    let foreign_project = project(&client, &foreign).await;

    let task = task(&client, &owner, own_project).await;
    let rejected = client
        .put_json_auth(
            &format!("/api/tasks/{task}"),
            &json!({ "title": "Must roll back", "project_ids": [foreign_project] }),
            &owner.token,
        )
        .await;
    assert_project_rejected(&rejected);

    let unchanged = client
        .get_auth(&format!("/api/tasks/{task}"), &owner.token)
        .await;
    unchanged.assert_status(StatusCode::OK);
    assert_eq!(unchanged.json_value()["task"]["title"], "Protected task");
    assert_eq!(
        unchanged.json_value()["task"]["project_ids"],
        json!([own_project])
    );

    let updated = client
        .put_json_auth(
            &format!("/api/tasks/{task}"),
            &json!({ "title": "Authorized update", "project_ids": [own_project] }),
            &owner.token,
        )
        .await;
    updated.assert_status(StatusCode::OK);
    assert_eq!(updated.json_value()["task"]["title"], "Authorized update");
    assert_eq!(
        updated.json_value()["task"]["project_ids"],
        json!([own_project])
    );
}

#[tokio::test]
async fn foreign_and_nonexistent_task_mutations_have_the_same_contract() {
    let client = TestClient::with_db().await;
    let owner = tenant(&client).await;
    let stranger = tenant(&client).await;
    let task = task(&client, &owner, project(&client, &owner).await).await;
    let nonexistent = Uuid::new_v4();

    for target in [task, nonexistent] {
        assert_task_hidden(
            &client
                .put_json_auth(
                    &format!("/api/tasks/{target}"),
                    &json!({ "title": "Forbidden" }),
                    &stranger.token,
                )
                .await,
        );
        assert_task_hidden(
            &client
                .post_json_auth(
                    &format!("/api/tasks/{target}/queue"),
                    &json!({}),
                    &stranger.token,
                )
                .await,
        );
        assert_task_hidden(
            &client
                .post_json_auth(
                    &format!("/api/tasks/{target}/runs"),
                    &json!({}),
                    &stranger.token,
                )
                .await,
        );
        assert_task_hidden(
            &client
                .delete_auth(&format!("/api/tasks/{target}"), &stranger.token)
                .await,
        );
    }
}

#[tokio::test]
async fn committed_membership_revocation_wins_before_task_mutation() {
    let pool = create_test_pool().await;
    let state = create_test_state(test_config(), pool.clone());
    let setup = TestClient::new(create_test_router(state.clone()));
    let owner = tenant(&setup).await;
    let task = task(&setup, &owner, project(&setup, &owner).await).await;

    let mut revocation = pool.begin().await.expect("revocation transaction starts");
    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(owner.workspace)
    .bind(owner.user)
    .execute(&mut *revocation)
    .await
    .expect("membership is locked for revocation");

    let mutation_client = TestClient::new(create_test_router(state));
    let token = owner.token.clone();
    let mut mutation = tokio::spawn(async move {
        mutation_client
            .put_json_auth(
                &format!("/api/tasks/{task}"),
                &json!({ "title": "Must not race revocation" }),
                &token,
            )
            .await
    });

    assert!(
        timeout(Duration::from_millis(250), &mut mutation)
            .await
            .is_err(),
        "mutation must wait for the membership row's transaction order"
    );
    revocation
        .commit()
        .await
        .expect("membership revocation commits");

    let response = timeout(Duration::from_secs(5), mutation)
        .await
        .expect("mutation completes after revocation")
        .expect("mutation task does not panic");
    assert_task_hidden(&response);

    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .expect("task remains stored");
    assert_eq!(title, "Protected task");
}

#[tokio::test]
async fn agentic_task_creation_rejects_foreign_projects_atomically() {
    let pool = create_test_pool().await;
    let state = create_test_state(test_config(), pool.clone());
    let client = TestClient::new(create_test_router(state));
    let owner = tenant(&client).await;
    let foreign = tenant(&client).await;
    let foreign_project = project(&client, &foreign).await;

    let error = actions::start_task(
        &pool,
        owner.workspace,
        owner.user,
        StartTask {
            title: "Foreign agentic task".to_owned(),
            description: "Must not be created".to_owned(),
            acceptance_criteria: None,
            project_ids: vec![foreign_project],
            source_id: None,
            priority: None,
        },
    )
    .await
    .expect_err("foreign project must be rejected");
    assert!(matches!(
        error,
        sqlx::Error::Protocol(message)
            if message == "Project is not available in this workspace"
    ));

    let created: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE workspace_id = $1 AND title = $2")
            .bind(owner.workspace)
            .bind("Foreign agentic task")
            .fetch_one(&pool)
            .await
            .expect("task count is available");
    assert_eq!(created, 0);
}

#[tokio::test]
async fn pr_worker_rejects_legacy_foreign_project_associations() {
    let pool = create_test_pool().await;
    let state = create_test_state(test_config(), pool.clone());
    let client = TestClient::new(create_test_router(state.clone()));
    let owner = tenant(&client).await;
    let foreign = tenant(&client).await;
    let own_project = project(&client, &owner).await;
    let foreign_project = project(&client, &foreign).await;
    let task = task(&client, &owner, own_project).await;

    sqlx::query("DELETE FROM task_projects WHERE task_id = $1")
        .bind(task)
        .execute(&pool)
        .await
        .expect("owned association is removed");
    sqlx::query("INSERT INTO task_projects (task_id, project_id) VALUES ($1, $2)")
        .bind(task)
        .bind(foreign_project)
        .execute(&pool)
        .await
        .expect("legacy invalid association is simulated");

    // Publication resolves the repository before it can use any credential, and
    // that resolution is where a legacy cross-workspace association is caught.
    // Asserting there keeps the check under test without standing up a live run
    // for the execution lease that create_pr_for_task authorizes first.
    let row = tasks::get_task(&pool, task)
        .await
        .expect("task is readable")
        .expect("task exists");
    let error = match Repository::resolve(&pool, &row).await {
        Err(error) => error,
        Ok(_) => panic!("a foreign project must not resolve to a repository"),
    };
    assert_eq!(error, "Task project belongs to another workspace");
}
