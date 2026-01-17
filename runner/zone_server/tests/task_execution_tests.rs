//! Task execution integration tests

mod common;

use uuid::Uuid;
use zone_server::db::tasks;

/// Test helper to create a test project and workspace
/// Returns (workspace_id, project_id)
async fn create_test_project(pool: &sqlx::PgPool) -> (Uuid, Uuid) {
    // Create workspace and related data
    let (_org_id, workspace_id, _user_id) = common::setup_test_data(pool).await;

    let project_id = Uuid::new_v4();
    let _: Uuid = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO projects (id, name, description, workspace_id) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(project_id)
    .bind("Test Project")
    .bind("A test project")
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .expect("Failed to create test project");

    (workspace_id, project_id)
}

#[tokio::test]
async fn test_create_task_run() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create a task
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "This is a test task",
        Some("Should complete successfully"),
        Some(1),
        true,
    )
    .await
    .expect("Failed to create task");

    // Create a task run
    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    assert_eq!(run.task_id, task.id);
    assert_eq!(run.status, "running");
    assert!(run.started_at.is_some());
    assert!(run.completed_at.is_none());
}

#[tokio::test]
async fn test_update_task_run_progress() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Update progress
    let updated = tasks::update_task_run_progress(&pool, run.id, Some("thinking"), Some(25))
        .await
        .expect("Failed to update progress");

    assert!(updated.is_some());
    let updated = updated.unwrap();
    assert_eq!(updated.current_phase, Some("thinking".to_string()));
    assert_eq!(updated.progress_percent, Some(25));
}

#[tokio::test]
async fn test_complete_task_run_success() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Complete successfully
    let completed = tasks::complete_task_run(
        &pool,
        run.id,
        "completed",
        None,
        Some(serde_json::json!({
            "iterations": 5,
            "tokens_used": 1000,
        })),
    )
    .await
    .expect("Failed to complete run");

    assert!(completed.is_some());
    let completed = completed.unwrap();
    assert_eq!(completed.status, "completed");
    assert!(completed.completed_at.is_some());
    assert_eq!(completed.progress_percent, Some(100));
    assert!(completed.error_message.is_none());
    assert!(completed.artifacts.is_some());
}

#[tokio::test]
async fn test_complete_task_run_failure() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Complete with failure
    let completed = tasks::complete_task_run(
        &pool,
        run.id,
        "failed",
        Some("Agent error: max iterations exceeded"),
        None,
    )
    .await
    .expect("Failed to complete run");

    assert!(completed.is_some());
    let completed = completed.unwrap();
    assert_eq!(completed.status, "failed");
    assert!(completed.completed_at.is_some());
    assert_eq!(
        completed.error_message,
        Some("Agent error: max iterations exceeded".to_string())
    );
}

#[tokio::test]
async fn test_add_task_run_log() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Add log entry
    let log = tasks::add_task_run_log(
        &pool,
        run.id,
        "thinking",
        "agent",
        "info",
        "Entering thinking phase",
        Some(serde_json::json!({"iteration": 1})),
    )
    .await
    .expect("Failed to add log");

    assert_eq!(log.task_run_id, run.id);
    assert_eq!(log.phase, "thinking");
    assert_eq!(log.agent_type, "agent");
    assert_eq!(log.log_level, "info");
    assert_eq!(log.message, "Entering thinking phase");
    assert!(log.metadata.is_some());
}

#[tokio::test]
async fn test_get_task_run_logs() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Add multiple log entries
    for i in 0..5 {
        tasks::add_task_run_log(
            &pool,
            run.id,
            "thinking",
            "agent",
            "info",
            &format!("Log entry {}", i),
            None,
        )
        .await
        .expect("Failed to add log");
    }

    // Get logs
    let logs = tasks::get_task_run_logs(&pool, run.id)
        .await
        .expect("Failed to get logs");

    assert_eq!(logs.len(), 5);
    // Logs should be ordered by created_at ASC
    for (i, log) in logs.iter().enumerate() {
        assert_eq!(log.message, format!("Log entry {}", i));
    }
}

#[tokio::test]
async fn test_task_run_lifecycle() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Lifecycle Test Task",
        "Test full lifecycle",
        Some("Should track all phases"),
        Some(1),
        true,
    )
    .await
    .expect("Failed to create task");

    // Create run
    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    assert_eq!(run.status, "running");

    // Simulate thinking phase
    tasks::update_task_run_progress(&pool, run.id, Some("thinking"), Some(10))
        .await
        .expect("Failed to update progress");

    tasks::add_task_run_log(
        &pool,
        run.id,
        "thinking",
        "agent",
        "info",
        "Analyzing task requirements",
        None,
    )
    .await
    .expect("Failed to add log");

    // Simulate acting phase
    tasks::update_task_run_progress(&pool, run.id, Some("acting"), Some(50))
        .await
        .expect("Failed to update progress");

    tasks::add_task_run_log(
        &pool,
        run.id,
        "acting",
        "tool",
        "info",
        "Executing tool: read_file",
        Some(serde_json::json!({"tool": "read_file"})),
    )
    .await
    .expect("Failed to add log");

    // Simulate responding phase
    tasks::update_task_run_progress(&pool, run.id, Some("responding"), Some(90))
        .await
        .expect("Failed to update progress");

    // Complete
    let completed = tasks::complete_task_run(
        &pool,
        run.id,
        "completed",
        None,
        Some(serde_json::json!({
            "iterations": 3,
            "tokens_used": 500,
            "summary": "Task completed successfully"
        })),
    )
    .await
    .expect("Failed to complete run");

    assert!(completed.is_some());
    let completed = completed.unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.progress_percent, Some(100));

    // Get all logs
    let logs = tasks::get_task_run_logs(&pool, run.id)
        .await
        .expect("Failed to get logs");

    assert!(logs.len() >= 2);
}

#[tokio::test]
async fn test_list_task_runs() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    // Create multiple runs
    for _ in 0..3 {
        tasks::create_task_run(&pool, task.id)
            .await
            .expect("Failed to create task run");
    }

    // List runs
    let runs = tasks::list_task_runs(&pool, task.id)
        .await
        .expect("Failed to list runs");

    assert_eq!(runs.len(), 3);
    // All should be for the same task
    for run in &runs {
        assert_eq!(run.task_id, task.id);
    }
}

#[tokio::test]
async fn test_get_task_run() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Test Task",
        "Test description",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Get run by ID
    let fetched = tasks::get_task_run(&pool, run.id)
        .await
        .expect("Failed to get run");

    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.id, run.id);
    assert_eq!(fetched.task_id, task.id);
    assert_eq!(fetched.status, "running");
}

#[tokio::test]
async fn test_task_run_with_error() {
    let pool = common::create_test_pool().await;
    let (workspace_id, project_id) = create_test_project(&pool).await;

    // Create task and run
    let task = tasks::create_task(
        &pool,
        workspace_id,
        &[project_id],
        "Error Task",
        "This task will fail",
        None,
        None,
        true,
    )
    .await
    .expect("Failed to create task");

    let run = tasks::create_task_run(&pool, task.id)
        .await
        .expect("Failed to create task run");

    // Add error log
    tasks::add_task_run_log(
        &pool,
        run.id,
        "error",
        "agent",
        "error",
        "Tool execution failed: command not found",
        Some(serde_json::json!({
            "tool": "run_command",
            "error": "command not found"
        })),
    )
    .await
    .expect("Failed to add error log");

    // Complete with error
    let completed = tasks::complete_task_run(
        &pool,
        run.id,
        "failed",
        Some("Tool execution failed: command not found"),
        None,
    )
    .await
    .expect("Failed to complete run");

    assert!(completed.is_some());
    let completed = completed.unwrap();
    assert_eq!(completed.status, "failed");
    assert!(completed.error_message.is_some());
    assert!(
        completed
            .error_message
            .unwrap()
            .contains("command not found")
    );
}
