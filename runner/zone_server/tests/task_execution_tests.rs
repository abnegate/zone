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
        None,
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
        None,
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
        None,
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
        None,
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
        None,
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
        None,
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
        None,
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
        None,
    )
    .await
    .expect("Failed to create task");

    // Create multiple runs
    for _ in 0..3 {
        let run = tasks::create_task_run(&pool, task.id)
            .await
            .expect("Failed to create task run");
        tasks::complete_task_run(&pool, run.id, "completed", None, None)
            .await
            .unwrap();
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
        None,
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
        None,
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

/// A run parked on `ask_user`, its worker still holding the lease.
struct Parked {
    pool: sqlx::PgPool,
    client: common::TestClient,
    token: String,
    organization: Uuid,
    user: Uuid,
    run: Uuid,
    provider: wiremock::MockServer,
    worker: tokio::task::JoinHandle<()>,
}

impl Parked {
    async fn status(&self) -> String {
        sqlx::query_scalar("SELECT status FROM task_runs WHERE id = $1")
            .bind(self.run)
            .fetch_one(&self.pool)
            .await
            .expect("the run is still readable")
    }

    async fn settles_on(&self, status: &str) {
        for _ in 0..400 {
            if self.status().await == status {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!(
            "the run never reached {status}; it is {}",
            self.status().await
        );
    }

    /// Every completion body the provider was sent, in order.
    async fn rounds(&self) -> Vec<serde_json::Value> {
        self.provider
            .received_requests()
            .await
            .expect("the provider recorded its requests")
            .into_iter()
            .filter(|request| request.url.path() == "/chat/completions")
            .map(|request| serde_json::from_slice(&request.body).expect("a JSON completion body"))
            .collect()
    }

    async fn answer(&self, body: serde_json::Value) -> common::TestResponse {
        self.client
            .post_json_auth(
                &format!("/api/tasks/runs/{}/answers", self.run),
                &body,
                &self.token,
            )
            .await
    }

    async fn finish(self) {
        self.worker.abort();
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(self.user)
            .execute(&self.pool)
            .await
            .unwrap();
    }
}

/// What the asking turn streams before it stops to ask, and what the turn the
/// answer buys streams after it.
const BEFORE_ASKING: &str = "Checking the ledger first.";
const AFTER_ANSWERING: &str = "Proceeding as answered.";

/// Run a task whose first completion asks `questions`, and stop once it parks.
async fn park(questions: serde_json::Value) -> Parked {
    use chrono::{Duration, Utc};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};
    use zone_server::auth::jwt::create_session_access_token;
    use zone_server::db::{sessions, workspace_members};

    let pool = common::create_test_pool().await;
    let (organization, workspace, user) = common::setup_test_data(&pool).await;
    workspace_members::add_member(
        &pool,
        workspace,
        user,
        workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .unwrap();
    let task = tasks::create_task_as(
        &pool,
        workspace,
        &[],
        "Asks before acting",
        "Decide the scope first",
        None,
        None,
        true,
        None,
        Some(user),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET model_name = 'gpt-4' WHERE id = $1")
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
    let run = tasks::create_task_run_as(&pool, task.id, Some(user))
        .await
        .unwrap();

    let provider = MockServer::start().await;
    let rounds = Arc::new(AtomicUsize::new(0));
    let asked = questions.to_string();
    Mock::given(method("POST")).and(path("/chat/completions")).respond_with(move |_: &Request| {
        let deltas = if rounds.fetch_add(1, Ordering::SeqCst) == 0 {
            vec![
                serde_json::json!({"content": BEFORE_ASKING}),
                serde_json::json!({"tool_calls":[{"index":0,"id":"ask-call","type":"function","function":{"name":"ask_user","arguments":asked}}]}),
            ]
        } else {
            vec![serde_json::json!({"content": AFTER_ANSWERING})]
        };
        let mut body = String::new();
        for delta in deltas {
            let chunk = serde_json::json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            body.push_str(&format!("data: {chunk}\n\n"));
        }
        let end = serde_json::json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
        body.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
        ResponseTemplate::new(200).insert_header("Content-Type", "text/event-stream").set_body_string(body)
    }).mount(&provider).await;

    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    let state = common::create_test_state(config.clone(), pool.clone());
    let client = common::TestClient::new(common::create_test_router(state.clone()));

    let session = sessions::create_session(
        &pool,
        user,
        &format!("refresh-{}", Uuid::new_v4()),
        None,
        None,
        None,
        (Utc::now() + Duration::hours(1)).naive_utc(),
    )
    .await
    .unwrap();
    let token = create_session_access_token(
        user,
        "answers@example.com",
        vec![],
        vec![],
        false,
        session.id,
        &config.jwt_secret,
        Duration::minutes(5),
    )
    .unwrap();

    let run_id = run.id;
    let task_id = task.id;
    let worker = tokio::spawn(async move {
        zone_server::workers::task::execute_task_run(&state, run_id, task_id).await;
    });

    let parked = Parked {
        pool,
        client,
        token,
        organization,
        user,
        run: run_id,
        provider,
        worker,
    };
    parked.settles_on("waiting").await;
    parked
}

fn optional() -> serde_json::Value {
    serde_json::json!({"questions":[{
        "header": "Scope",
        "question": "How far back should the fix reach?",
        "options": [
            {"label":"Backfill","description":"Repair every existing row"},
            {"label":"Forward only","description":"Leave history alone"}
        ]
    }]})
}

#[tokio::test]
async fn an_optional_question_parks_the_run_until_a_member_answers_it() {
    let parked = park(optional()).await;

    let read = parked
        .client
        .get_auth(&format!("/api/tasks/runs/{}", parked.run), &parked.token)
        .await;
    read.assert_status(axum::http::StatusCode::OK);
    let body = read.json_value();
    assert_eq!(body["run"]["status"], "waiting");
    let pending = &body["run"]["pending_question"];
    assert_eq!(pending["tool_call_id"], "ask-call");
    assert_eq!(pending["questions"][0]["header"], "Scope");
    assert_eq!(pending["questions"][0]["required"], false);
    assert_eq!(
        pending["questions"][0]["choices"][2]["label"], "Other",
        "the card the console renders carries the free-text option the server appended"
    );

    parked
        .answer(serde_json::json!({"answers":[{"header":"Scope","labels":["Forward only"]}]}))
        .await
        .assert_status(axum::http::StatusCode::ACCEPTED);

    parked.settles_on("completed").await;
    let resumed = sqlx::query_scalar::<_, Option<serde_json::Value>>(
        "SELECT pending_question FROM task_runs WHERE id = $1",
    )
    .bind(parked.run)
    .fetch_one(&parked.pool)
    .await
    .unwrap();
    assert_eq!(resumed, None, "a finished run is no longer asking anything");

    let rounds = parked.rounds().await;
    assert_eq!(rounds.len(), 2, "the answer bought exactly one more turn");
    let messages = rounds[1]["messages"].as_array().unwrap();
    let last = messages.last().unwrap();
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"], "Scope: Forward only");

    parked.finish().await;
}

/// The artifacts describe the run, and a run that asked something ran more
/// than the turn that answered. Reporting only the last turn hides the work
/// every earlier one did and throws away what it said before it stopped.
#[tokio::test]
async fn a_parked_turn_still_counts_towards_the_run_it_belongs_to() {
    let parked = park(optional()).await;

    parked
        .answer(serde_json::json!({"answers":[{"header":"Scope","labels":["Backfill"]}]}))
        .await
        .assert_status(axum::http::StatusCode::ACCEPTED);
    parked.settles_on("completed").await;

    let artifacts: serde_json::Value =
        sqlx::query_scalar("SELECT artifacts FROM task_runs WHERE id = $1")
            .bind(parked.run)
            .fetch_one(&parked.pool)
            .await
            .unwrap();
    let summary = artifacts["summary"].as_str().expect("a recorded summary");
    assert!(
        summary.contains(BEFORE_ASKING),
        "the asking turn's prose never reached the run: {summary}"
    );
    assert!(
        summary.contains(AFTER_ANSWERING),
        "the answering turn's prose never reached the run: {summary}"
    );
    assert_eq!(
        artifacts["tool_calls"], 1,
        "the question the run asked is a tool call it made"
    );

    parked.finish().await;
}

#[tokio::test]
async fn an_answer_is_refused_unless_it_fits_the_question_that_was_asked() {
    let parked = park(optional()).await;

    let unknown = parked
        .answer(serde_json::json!({"answers":[{"header":"Branch","labels":["main"]}]}))
        .await;
    unknown.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert!(
        unknown.text().contains("Branch"),
        "the rejection names the header nothing asked: {}",
        unknown.text()
    );
    assert_eq!(parked.status().await, "waiting");

    let blank = parked
        .answer(
            serde_json::json!({"answers":[{"header":"Scope","labels":["Other"],"other":"   "}]}),
        )
        .await;
    blank.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(
        parked.status().await,
        "waiting",
        "a refused answer leaves the run exactly where it was"
    );

    let both = parked
        .answer(serde_json::json!({"answers":[{"header":"Scope","labels":["Backfill","Forward only"]}]}))
        .await;
    both.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(parked.status().await, "waiting");

    // The answer becomes an entry compaction can never shed, so a paste that
    // fits under the body limit would still sit in the window for the rest of
    // the run and push every later turn into a capacity failure.
    let paste = "x".repeat(zone_server::agent::question::MAX_FREE_TEXT + 1);
    let oversized = parked
        .answer(
            serde_json::json!({"answers":[{"header":"Scope","labels":["Other"],"other":paste}]}),
        )
        .await;
    oversized.assert_status(axum::http::StatusCode::BAD_REQUEST);
    assert!(
        oversized.text().contains("Scope"),
        "the rejection names the question it came back on: {}",
        oversized.text()
    );
    assert_eq!(parked.status().await, "waiting");

    for empty in [
        serde_json::json!({"answers":[]}),
        serde_json::json!({"answers":[{"header":"Scope","labels":[]}]}),
    ] {
        let nothing = parked.answer(empty).await;
        nothing.assert_status(axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(
            parked.status().await,
            "waiting",
            "declining is what letting the window elapse means, not an empty resume"
        );
    }

    parked
        .answer(serde_json::json!({"answers":[{"header":"Scope","labels":["Other"],"other":"Only the last quarter"}]}))
        .await
        .assert_status(axum::http::StatusCode::ACCEPTED);
    parked.settles_on("completed").await;

    let rounds = parked.rounds().await;
    let messages = rounds[1]["messages"].as_array().unwrap();
    assert_eq!(
        messages.last().unwrap()["content"],
        "Scope: Other: Only the last quarter"
    );

    let late = parked
        .answer(serde_json::json!({"answers":[{"header":"Scope","labels":["Backfill"]}]}))
        .await;
    assert!(
        late.status == axum::http::StatusCode::CONFLICT
            || late.status == axum::http::StatusCode::NOT_FOUND,
        "a run that already moved on has nothing to answer: {}",
        late.status
    );

    parked.finish().await;
}

#[tokio::test]
async fn a_parked_run_keeps_its_lease_and_its_admission_slot() {
    let parked = park(optional()).await;

    let owner: Uuid = sqlx::query_scalar("SELECT owner FROM task_runs WHERE id = $1")
        .bind(parked.run)
        .fetch_one(&parked.pool)
        .await
        .unwrap();
    assert!(
        tasks::heartbeat_task_run(&parked.pool, parked.run, owner)
            .await
            .unwrap(),
        "a waiting run still refreshes the lease its worker holds"
    );
    tasks::sweep_task_runs(&parked.pool).await.unwrap();
    assert_eq!(
        parked.status().await,
        "waiting",
        "the sweeper has no orphan to reap while the worker keeps heartbeating"
    );

    parked
        .answer(serde_json::json!({"answers":[{"header":"Scope","labels":["Backfill"]}]}))
        .await
        .assert_status(axum::http::StatusCode::ACCEPTED);
    parked.settles_on("completed").await;
    parked.finish().await;
}

fn required() -> serde_json::Value {
    serde_json::json!({"questions":[{
        "header": "Scope",
        "question": "How far back should the fix reach?",
        "required": true,
        "options": [
            {"label":"Backfill","description":"Repair every existing row"},
            {"label":"Forward only","description":"Leave history alone"}
        ]
    }]})
}

/// A required question never proceeds on a default, so the only thing that ends
/// an unanswered one is `TASK_TIMEOUT`, an hour later.
///
/// The hour is not waited out here. What the run must survive to reach it is
/// the parked state itself, and what the timeout must then be able to do is end
/// a `waiting` row terminally: the classification that decides there is no
/// retry is asserted beside `Fault::timeout` in the worker's own tests.
#[tokio::test]
async fn a_required_question_waits_and_a_timeout_can_still_end_the_parked_run() {
    let parked = park(required()).await;

    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT pending_question FROM task_runs WHERE id = $1")
            .bind(parked.run)
            .fetch_one(&parked.pool)
            .await
            .unwrap();
    assert_eq!(stored["questions"][0]["required"], true);

    let owner: Uuid = sqlx::query_scalar("SELECT owner FROM task_runs WHERE id = $1")
        .bind(parked.run)
        .fetch_one(&parked.pool)
        .await
        .unwrap();
    let ended = tasks::complete_owned_task_run(
        &parked.pool,
        parked.run,
        Some(owner),
        "failed",
        Some("Task execution timed out after 3600 seconds"),
        Some(serde_json::json!({"attempts": 1, "classification": "terminal"})),
    )
    .await
    .expect("a parked run is still the worker's to end");
    assert!(
        ended.is_some(),
        "a timeout must be able to end a run that is waiting, not only one that is running"
    );
    let ended = ended.unwrap();
    assert_eq!(ended.status, "failed");
    assert_eq!(
        ended.error_message.as_deref(),
        Some("Task execution timed out after 3600 seconds")
    );
    assert_eq!(
        ended.pending_question, None,
        "a run nobody is waiting on any more must stop offering an answerable card"
    );

    parked.finish().await;
}
