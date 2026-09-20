//! The reception sweep reads a task's pull request back from GitHub. What it
//! learns has to reach the task the console shows: a pull request GitHub says
//! merged reads "merged" on its task, not the "open" it was created with.
mod common;

use axum::{Json, Router, extract::Path, routing::get};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use zone_server::db::{projects, tasks};
use zone_server::state::AppState;
use zone_server::workers::pr::{ReceptionSyncResult, sync_reception};

const MERGED_AT: &str = "2026-09-20T04:13:23Z";

async fn pull_request(Path((_, _, number)): Path<(String, String, i64)>) -> Json<Value> {
    Json(json!({
        "number": number,
        "state": "closed",
        "created_at": "2026-09-20T03:40:00Z",
        "merged_at": MERGED_AT,
    }))
}

async fn nothing() -> Json<Value> {
    Json(json!([]))
}

/// A GitHub that answers one merged pull request with no reviews or comments.
async fn github_with_a_merged_pull_request() -> String {
    let router = Router::new()
        .route("/repos/{owner}/{repo}/pulls/{number}", get(pull_request))
        .route("/repos/{owner}/{repo}/pulls/{number}/reviews", get(nothing))
        .route(
            "/repos/{owner}/{repo}/pulls/{number}/comments",
            get(nothing),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{address}")
}

#[tokio::test]
async fn a_merged_pull_request_is_read_back_as_merged_on_its_task() {
    let pool = common::create_test_pool().await;
    let (organization, workspace, user) = common::setup_test_data(&pool).await;
    let project = projects::create_project(&pool, "Ships", None, Some(workspace))
        .await
        .unwrap();
    projects::link_github(
        &pool,
        project.id,
        "https://github.com/acme/project",
        Some("ghp_reception"),
    )
    .await
    .unwrap();
    let task = tasks::create_task(
        &pool,
        workspace,
        &[project.id],
        "Ships a change",
        "Opens a pull request",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    sqlx::query("UPDATE tasks SET pr_url = $2, pr_status = 'open' WHERE id = $1")
        .bind(task.id)
        .bind("http://127.0.0.1/acme/project/pull/7")
        .execute(&pool)
        .await
        .unwrap();

    let mut config = common::test_config();
    config.github_api_url = github_with_a_merged_pull_request().await;
    let state = AppState::new(config, pool.clone(), None);

    let outcome = sync_reception(&state, run.id, task.id).await;
    assert!(
        matches!(outcome, ReceptionSyncResult::Recorded(_)),
        "the reception was not recorded: {outcome:?}"
    );

    let read = tasks::get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(
        read.pr_status.as_deref(),
        Some("merged"),
        "the task still reads {:?} after GitHub said merged",
        read.pr_status
    );
    let artifacts: Value = sqlx::query_scalar("SELECT artifacts FROM task_runs WHERE id = $1")
        .bind(run.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(artifacts["pr"]["merged_at"], json!(MERGED_AT));
    assert_eq!(artifacts["pr"]["pr_state"], json!("closed"));

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
}
