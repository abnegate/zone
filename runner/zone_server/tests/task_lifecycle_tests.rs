mod common;

use zone_server::db::tasks;

#[tokio::test]
async fn terminal_run_cannot_be_resurrected() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Lifecycle",
        "Regression",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    tasks::complete_task_run(&pool, run.id, "failed", Some("orphaned"), None)
        .await
        .unwrap();
    assert!(
        tasks::complete_task_run(&pool, run.id, "completed", None, None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tasks::update_task_run_progress(&pool, run.id, Some("acting"), Some(50))
            .await
            .unwrap()
            .is_none()
    );
    let task = tasks::get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(task.status, "blocked");
}

#[tokio::test]
async fn concurrent_run_creation_admits_only_one() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Concurrent",
        "Regression",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let (first, second) = tokio::join!(
        tasks::create_task_run(&pool, task.id),
        tasks::create_task_run(&pool, task.id)
    );
    assert_ne!(first.is_ok(), second.is_ok());
}

#[tokio::test]
async fn foreign_actor_cannot_read_task() {
    use axum::{
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
    };
    use zone_server::auth::{AuthUser, jwt::Claims};
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let (_, _, foreign) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Private",
        "Secret",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let state = common::create_test_state(common::test_config(), pool);
    let auth = AuthUser(Claims {
        sub: foreign.to_string(),
        email: "foreign@example.com".into(),
        roles: vec![],
        permissions: vec![],
        exp: 0,
        iat: 0,
        jti: uuid::Uuid::new_v4().to_string(),
        is_admin: false,
    });
    let response = zone_server::routes::tasks::get(State(state), auth, Path(task.id))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sweep_preserves_fresh_leases_and_newer_runs() {
    let pool = common::create_test_pool().await;
    let (_, workspace, actor) = common::setup_test_data(&pool).await;
    sqlx::query(
        "INSERT INTO workspace_members(workspace_id, user_id, role) VALUES ($1, $2, 'member')",
    )
    .bind(workspace)
    .bind(actor)
    .execute(&pool)
    .await
    .unwrap();
    let task = tasks::create_task_as(
        &pool,
        workspace,
        &[],
        "Sweep",
        "Regression",
        None,
        None,
        true,
        None,
        Some(actor),
    )
    .await
    .unwrap();
    assert_eq!(task.created_by, Some(actor));
    let run = tasks::create_task_run_as(&pool, task.id, Some(actor))
        .await
        .unwrap();
    assert_eq!(run.triggered_by, Some(actor));
    let owner = uuid::Uuid::new_v4();
    assert!(tasks::claim_task_run(&pool, run.id, owner).await.unwrap());
    assert!(
        !tasks::claim_task_run(&pool, run.id, uuid::Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(
        !tasks::heartbeat_task_run(&pool, run.id, uuid::Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(
        tasks::heartbeat_task_run(&pool, run.id, owner)
            .await
            .unwrap()
    );
    tasks::sweep_task_runs(&pool).await.unwrap();
    assert_eq!(
        tasks::get_task_run(&pool, run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    sqlx::query("UPDATE task_runs SET heartbeat_at = NOW() - INTERVAL '61 seconds' WHERE id = $1")
        .bind(run.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !tasks::heartbeat_task_run(&pool, run.id, owner)
            .await
            .unwrap()
    );
    tasks::sweep_task_runs(&pool).await.unwrap();
    let failed = tasks::get_task_run(&pool, run.id).await.unwrap().unwrap();
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.error_message.as_deref(), Some("orphaned"));
    let next = tasks::create_task_run(&pool, task.id).await.unwrap();
    assert!(tasks::start_task_run(&pool, next.id).await.unwrap());
    assert!(
        tasks::complete_task_run(&pool, run.id, "completed", None, None)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        tasks::get_task(&pool, task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "in_progress"
    );
    tasks::complete_task_run(&pool, next.id, "completed", None, None)
        .await
        .unwrap();
    assert_eq!(
        tasks::get_task(&pool, task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "review"
    );
}

#[tokio::test]
async fn task_routes_reject_foreign_and_readonly_mutations() {
    use axum::{
        Json,
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
    };
    use zone_server::auth::{AuthUser, jwt::Claims};
    use zone_server::routes::tasks as routes;
    let pool = common::create_test_pool().await;
    let (_, workspace, actor) = common::setup_test_data(&pool).await;
    sqlx::query(
        "INSERT INTO workspace_members(workspace_id, user_id, role) VALUES ($1, $2, 'member')",
    )
    .bind(workspace)
    .bind(actor)
    .execute(&pool)
    .await
    .unwrap();
    let (_, _, foreign) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Private",
        "Secret",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    let state = common::create_test_state(common::test_config(), pool.clone());
    let auth = |id: uuid::Uuid| {
        AuthUser(Claims {
            sub: id.to_string(),
            email: "test@example.com".into(),
            roles: vec![],
            permissions: vec![],
            exp: 0,
            iat: 0,
            jti: uuid::Uuid::new_v4().to_string(),
            is_admin: false,
        })
    };
    for id in [foreign, actor] {
        if id == actor {
            sqlx::query("UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2").bind(workspace).bind(actor).execute(&pool).await.unwrap();
        }
        let response = routes::create_run(State(state.clone()), auth(id), Path(task.id))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = routes::queue(State(state.clone()), auth(id), Path(task.id))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = routes::delete(State(state.clone()), auth(id), Path(task.id))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let request =
            serde_json::from_value(serde_json::json!({"description":"malicious"})).unwrap();
        let response = routes::update(State(state.clone()), auth(id), Path(task.id), Json(request))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let request = serde_json::from_value(
            serde_json::json!({"title":"Malicious","description":"Injected"}),
        )
        .unwrap();
        let response = routes::create(
            State(state.clone()),
            auth(id),
            Path(workspace),
            Json(request),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for response in [
        routes::get_run(State(state.clone()), auth(foreign), Path(run.id))
            .await
            .into_response(),
        routes::get_run_logs(State(state.clone()), auth(foreign), Path(run.id))
            .await
            .into_response(),
        routes::list_runs(State(state.clone()), auth(foreign), Path(task.id))
            .await
            .into_response(),
    ] {
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = routes::get(State(state.clone()), auth(actor), Path(task.id))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(actor)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        routes::get(State(state), auth(actor), Path(task.id))
            .await
            .into_response()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        tasks::get_task(&pool, task.id)
            .await
            .unwrap()
            .unwrap()
            .description,
        "Secret"
    );
}

#[tokio::test]
async fn stale_owner_cannot_write_progress_logs_or_terminal_state() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Ownership",
        "Regression",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    let first = uuid::Uuid::new_v4();
    let second = uuid::Uuid::new_v4();
    assert!(tasks::claim_task_run(&pool, run.id, first).await.unwrap());
    sqlx::query("UPDATE task_runs SET owner=$2 WHERE id=$1")
        .bind(run.id)
        .bind(second)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        tasks::update_owned_task_run_progress(&pool, run.id, Some(first), Some("acting"), Some(10))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tasks::update_task_run_progress(&pool, run.id, Some("acting"), Some(10))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !tasks::add_owned_task_run_log(
            &pool,
            run.id,
            Some(first),
            "acting",
            "agent",
            "info",
            "stale",
            None
        )
        .await
        .unwrap()
    );
    assert!(
        tasks::complete_owned_task_run(&pool, run.id, Some(first), "completed", None, None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tasks::complete_task_run(&pool, run.id, "completed", None, None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tasks::get_task_run_logs(&pool, run.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        tasks::get_task_run(&pool, run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    let state = common::create_test_state(common::test_config(), pool.clone());
    let tools =
        zone_server::agent::ChatTools::for_task(&state, std::env::temp_dir(), workspace, None)
            .await
            .with_task_lease(pool.clone(), run.id, first);
    let denied = tools
        .execute(
            "run_command",
            r#"{"command":"echo","args":["must not execute"]}"#,
        )
        .await;
    assert!(!denied.success);
    assert_eq!(
        denied.error.as_deref(),
        Some("Task execution lost its lease")
    );
    assert!(
        tasks::complete_owned_task_run(&pool, run.id, Some(second), "completed", None, None)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn active_run_owns_task_status_until_completion() {
    let pool = common::create_test_pool().await;
    let (_, workspace, _) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Active",
        "Regression",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    let run = tasks::create_task_run(&pool, task.id).await.unwrap();
    assert!(
        tasks::update_task(
            &pool,
            task.id,
            None,
            None,
            None,
            Some("complete"),
            None,
            None
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(tasks::queue_task(&pool, task.id).await.unwrap().is_none());
    assert!(
        tasks::update_task(
            &pool,
            task.id,
            Some("Renamed"),
            None,
            None,
            None,
            None,
            None
        )
        .await
        .unwrap()
        .is_some()
    );
    assert_eq!(
        tasks::get_task(&pool, task.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "queued"
    );
    tasks::complete_task_run(&pool, run.id, "completed", None, None)
        .await
        .unwrap();
    assert!(
        tasks::update_task(
            &pool,
            task.id,
            None,
            None,
            None,
            Some("complete"),
            None,
            None
        )
        .await
        .unwrap()
        .is_some()
    );
}
