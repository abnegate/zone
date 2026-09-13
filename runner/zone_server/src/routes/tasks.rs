//! Task endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::question::{self, Answer, Question};
use crate::auth::AuthUser;
use crate::db::{task_access, tasks, workspace_members};
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

/// Task response
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    task: TaskData,
}

/// Task data
#[derive(Debug, Serialize)]
pub struct TaskData {
    id: Uuid,
    workspace_id: Uuid,
    project_ids: Vec<Uuid>,
    title: String,
    description: String,
    acceptance_criteria: Option<String>,
    status: String,
    priority: Option<i32>,
    is_agentic: bool,
    model_name: Option<String>,
    dependencies: serde_json::Value,
    github_repo_url: Option<String>,
    source_id: Option<Uuid>,
    source_ids: Vec<Uuid>,
    worker_id: Option<String>,
    queued_at: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    pr_url: Option<String>,
    branch_name: Option<String>,
    pr_status: Option<String>,
    pr_created_at: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

/// Tasks list response
#[derive(Debug, Serialize)]
pub struct TasksListResponse {
    tasks: Vec<TaskData>,
}

impl From<tasks::TaskRow> for TaskData {
    fn from(row: tasks::TaskRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            project_ids: row.project_ids,
            title: row.title,
            description: row.description,
            acceptance_criteria: row.acceptance_criteria,
            status: row.status,
            priority: row.priority,
            is_agentic: row.is_agentic,
            model_name: row.model_name,
            dependencies: row.dependencies.unwrap_or_else(|| serde_json::json!([])),
            github_repo_url: row.github_repo_url,
            source_id: row.source_id,
            source_ids: row.source_ids.unwrap_or_default(),
            worker_id: row.worker_id,
            queued_at: row
                .queued_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            started_at: row
                .started_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            completed_at: row
                .completed_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            pr_url: row.pr_url,
            branch_name: row.branch_name,
            pr_status: row.pr_status,
            pr_created_at: row
                .pr_created_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl From<tasks::TaskRow> for TaskResponse {
    fn from(row: tasks::TaskRow) -> Self {
        Self {
            task: TaskData::from(row),
        }
    }
}

/// Task run response
#[derive(Debug, Serialize)]
pub struct TaskRunResponse {
    run: TaskRunData,
}

/// Task run data
#[derive(Debug, Serialize)]
pub struct TaskRunData {
    id: Uuid,
    task_id: Uuid,
    status: String,
    current_phase: Option<String>,
    progress_percent: Option<i32>,
    /// When the run began and when it finished, in RFC 3339, as `TaskData` in
    /// this file sends the same two columns.
    ///
    /// The console orders a task's earlier runs by `started_at`, so a run that
    /// travelled without one sorted as though it had never started.
    started_at: Option<String>,
    completed_at: Option<String>,
    error_message: Option<String>,
    /// The question envelope a `waiting` run is parked on, or nothing.
    ///
    /// Without it the console can see that a run stopped and not what it
    /// stopped to ask, which is the only thing anyone can act on.
    pending_question: Option<serde_json::Value>,
    /// What a `waiting` run is waiting for, and until when, or nothing.
    ///
    /// The sibling of `pending_question`: a run parks on one or the other, and
    /// which field arrived is how a reader tells a question nobody has answered
    /// from a wait nobody can. The column keeps `pending_wait` beside its
    /// sibling; the wire says what the run is doing. Absent rather than null,
    /// so a reader that has the key knows there is a wait to describe.
    #[serde(skip_serializing_if = "Option::is_none")]
    waiting_on: Option<serde_json::Value>,
}

/// Task runs list response
#[derive(Debug, Serialize)]
pub struct TaskRunsListResponse {
    runs: Vec<TaskRunData>,
}

impl From<tasks::TaskRunRow> for TaskRunData {
    fn from(row: tasks::TaskRunRow) -> Self {
        Self {
            id: row.id,
            task_id: row.task_id,
            status: row.status,
            current_phase: row.current_phase,
            progress_percent: row.progress_percent,
            started_at: row
                .started_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            completed_at: row
                .completed_at
                .map(|timestamp| timestamp.and_utc().to_rfc3339()),
            error_message: row.error_message,
            pending_question: row.pending_question,
            waiting_on: row.pending_wait,
        }
    }
}

impl From<tasks::TaskRunRow> for TaskRunResponse {
    fn from(row: tasks::TaskRunRow) -> Self {
        Self {
            run: TaskRunData::from(row),
        }
    }
}

/// Task run log response
#[derive(Debug, Serialize)]
pub struct TaskRunLogResponse {
    log: TaskRunLogData,
}

/// Task run log data
#[derive(Debug, Serialize)]
pub struct TaskRunLogData {
    id: Uuid,
    phase: String,
    agent_type: String,
    log_level: String,
    message: String,
    metadata: Option<serde_json::Value>,
    created_at: String,
}

/// Task run logs list response
#[derive(Debug, Serialize)]
pub struct TaskRunLogsListResponse {
    logs: Vec<TaskRunLogData>,
}

impl From<tasks::TaskRunLogRow> for TaskRunLogData {
    fn from(row: tasks::TaskRunLogRow) -> Self {
        Self {
            id: row.id,
            phase: row.phase,
            agent_type: row.agent_type,
            log_level: row.log_level,
            message: row.message,
            metadata: row.metadata,
            created_at: row
                .created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

impl From<tasks::TaskRunLogRow> for TaskRunLogResponse {
    fn from(row: tasks::TaskRunLogRow) -> Self {
        Self {
            log: TaskRunLogData::from(row),
        }
    }
}

/// Query parameters for listing tasks
#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    project_id: Option<Uuid>,
    status: Option<String>,
}

/// Create task request
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    #[serde(default)]
    project_ids: Vec<Uuid>,
    title: String,
    description: String,
    acceptance_criteria: Option<String>,
    priority: Option<i32>,
    is_agentic: Option<bool>,
    source_id: Option<Uuid>,
}

/// Update task request
#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    title: Option<String>,
    description: Option<String>,
    acceptance_criteria: Option<String>,
    status: Option<String>,
    priority: Option<i32>,
    project_ids: Option<Vec<Uuid>>,
}

fn denied(status: StatusCode, message: &str) -> Box<Response> {
    Box::new((status, Json(ErrorResponse::new(message))).into_response())
}

fn database_error(error: impl std::fmt::Display) -> Box<Response> {
    tracing::error!("Database error: {error}");
    denied(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
}

fn user_id(auth: &AuthUser) -> Result<Uuid, Box<Response>> {
    Uuid::parse_str(&auth.0.sub)
        .map_err(|_| denied(StatusCode::UNAUTHORIZED, "Invalid user ID in token"))
}

fn mutation_error(error: tasks::MutationError) -> Box<Response> {
    match error {
        tasks::MutationError::Project => denied(
            StatusCode::BAD_REQUEST,
            "Project is not available in this workspace",
        ),
        tasks::MutationError::Source => denied(
            StatusCode::BAD_REQUEST,
            "Source is not available in this workspace",
        ),
        tasks::MutationError::ActiveRun => denied(
            StatusCode::CONFLICT,
            "Task has an active run or is no longer available",
        ),
        tasks::MutationError::Database(error) => database_error(error),
    }
}

/// Every handler in this module addresses a task, run, or workspace by an id
/// taken straight from the request, and the queries behind them are keyed on
/// that id alone. Without this the id is the only credential: any account can
/// read, rewrite, delete, and start agent runs in any other tenant's
/// workspace.
///
/// A caller who is not a member is told the resource does not exist, so the
/// endpoints cannot be used to enumerate ids across tenants.
async fn authorize_workspace(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: Uuid,
    missing: &str,
) -> Result<(), Box<Response>> {
    let user_id = user_id(auth)?;
    let permitted = workspace_members::is_member(state.db(), user_id, workspace_id)
        .await
        .map_err(database_error)?;

    if permitted {
        return Ok(());
    }

    Err(denied(StatusCode::NOT_FOUND, missing))
}

async fn authorize_task(
    state: &AppState,
    auth: &AuthUser,
    task_id: Uuid,
) -> Result<(), Box<Response>> {
    let task = tasks::get_task(state.db(), task_id)
        .await
        .map_err(database_error)?
        .ok_or_else(|| denied(StatusCode::NOT_FOUND, "Task not found"))?;

    authorize_workspace(state, auth, task.workspace_id, "Task not found").await
}

/// Authorize a run and read it in the same transaction.
///
/// `task_access::read` holds a shared lock on the caller's membership while it
/// reads the run and its logs, so a revocation committing mid-request cannot be
/// overtaken by the disclosure. Checking first and reading afterwards would
/// leave exactly that window open.
async fn authorize_run(
    state: &AppState,
    auth: &AuthUser,
    run_id: Uuid,
) -> Result<task_access::Snapshot, Box<Response>> {
    let actor = user_id(auth)?;
    task_access::read(state.db(), run_id, actor)
        .await
        .map_err(database_error)?
        .ok_or_else(|| denied(StatusCode::NOT_FOUND, "Task run not found"))
}

/// GET /api/workspaces/:workspace_id/tasks
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListTasksQuery>,
) -> impl IntoResponse {
    if let Err(response) =
        authorize_workspace(&state, &auth, workspace_id, "Workspace not found").await
    {
        return *response;
    }
    match tasks::list_tasks(
        state.db(),
        workspace_id,
        query.project_id,
        query.status.as_deref(),
    )
    .await
    {
        Ok(items) => Json(TasksListResponse {
            tasks: items.into_iter().map(TaskData::from).collect(),
        })
        .into_response(),
        Err(error) => *database_error(error),
    }
}

/// POST /api/workspaces/:workspace_id/tasks
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let user_id = match user_id(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match tasks::create_task_authorized(
        state.db(),
        user_id,
        tasks::Create {
            workspace_id,
            project_ids: &request.project_ids,
            title: &request.title,
            description: &request.description,
            acceptance_criteria: request.acceptance_criteria.as_deref(),
            priority: request.priority,
            is_agentic: request.is_agentic.unwrap_or(false),
            source_id: request.source_id,
            created_by: Some(user_id),
        },
    )
    .await
    {
        Ok(tasks::Mutation::Applied(task)) => {
            (StatusCode::CREATED, Json(TaskResponse::from(task))).into_response()
        }
        Ok(tasks::Mutation::NotFound) => *denied(
            StatusCode::FORBIDDEN,
            "You do not have write access to this workspace",
        ),
        Err(error) => *mutation_error(error),
    }
}

/// GET /api/tasks/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(response) = authorize_task(&state, &auth, id).await {
        return *response;
    }
    match tasks::get_task(state.db(), id).await {
        Ok(Some(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Task not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// PUT /api/tasks/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTaskRequest>,
) -> impl IntoResponse {
    let user_id = match user_id(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match tasks::update_task_authorized(
        state.db(),
        user_id,
        tasks::Patch {
            id,
            title: req.title.as_deref(),
            description: req.description.as_deref(),
            acceptance_criteria: req.acceptance_criteria.as_deref(),
            status: req.status.as_deref(),
            priority: req.priority,
            project_ids: req.project_ids.as_deref(),
        },
    )
    .await
    {
        Ok(tasks::Mutation::Applied(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(tasks::Mutation::NotFound) => *denied(StatusCode::NOT_FOUND, "Task not found"),
        Err(error) => *mutation_error(error),
    }
}

/// DELETE /api/tasks/:id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match user_id(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match tasks::delete_task_authorized(state.db(), user_id, id).await {
        Ok(tasks::Mutation::Applied(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(tasks::Mutation::NotFound) => *denied(StatusCode::NOT_FOUND, "Task not found"),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/tasks/:id/queue
pub async fn queue(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match user_id(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match tasks::queue_task_authorized(state.db(), user_id, id).await {
        Ok(tasks::Mutation::Applied(task)) => Json(TaskResponse::from(task)).into_response(),
        Ok(tasks::Mutation::NotFound) => *denied(StatusCode::NOT_FOUND, "Task not found"),
        Err(error) => *mutation_error(error),
    }
}

/// GET /api/tasks/:id/runs
pub async fn list_runs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(response) = authorize_task(&state, &auth, id).await {
        return *response;
    }
    match tasks::list_task_runs(state.db(), id).await {
        Ok(runs) => Json(TaskRunsListResponse {
            runs: runs.into_iter().map(TaskRunData::from).collect(),
        })
        .into_response(),
        Err(error) => *database_error(error),
    }
}

/// POST /api/tasks/:id/runs
pub async fn create_run(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match user_id(&auth) {
        Ok(user_id) => user_id,
        Err(response) => return *response,
    };
    match tasks::create_task_run_authorized(state.db(), user_id, id).await {
        Ok(tasks::Mutation::Applied(tasks::RunMutation::Created(run))) => {
            let run_id = run.id;
            let task_id = id;
            let state_clone = state.clone();

            // Spawn background task execution
            tokio::spawn(async move {
                crate::workers::task::execute_task_run(&state_clone, run_id, task_id).await;
            });

            (StatusCode::CREATED, Json(TaskRunResponse::from(run))).into_response()
        }
        Ok(tasks::Mutation::Applied(tasks::RunMutation::Active(run))) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse::new(format!(
                "Task already has an active run (id: {}, status: {})",
                run.id, run.status
            ))),
        )
            .into_response(),
        Ok(tasks::Mutation::NotFound) => *denied(StatusCode::NOT_FOUND, "Task not found"),
        Err(e) => {
            if e.as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                return (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new("Task already has an active run")),
                )
                    .into_response();
            }

            *database_error(e)
        }
    }
}

/// GET /api/tasks/runs/:run_id
pub async fn get_run(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    match authorize_run(&state, &auth, run_id).await {
        Ok(snapshot) => Json(TaskRunResponse::from(snapshot.run)).into_response(),
        Err(response) => response.into_response(),
    }
}

/// What a member sends back for a run parked on a question.
#[derive(Debug, Deserialize)]
pub struct AnswersRequest {
    answers: Vec<Answer>,
}

/// What the questions were, read back off the parked run itself.
///
/// The submission is checked against this rather than against anything the
/// caller sent, so a stale card cannot answer a question the run is not asking.
#[derive(Debug, Deserialize)]
struct Pending {
    questions: Vec<Question>,
}

/// Confirmation that the answer reached the run that asked.
#[derive(Debug, Serialize)]
pub struct AnswersResponse {
    run_id: Uuid,
    answered: usize,
}

const WAITING: &str = "waiting";
const NOT_WAITING: &str = "Task run is not waiting on a question";
const NOT_FOUND: &str = "Task run not found";
const NOTHING_CHOSEN: &str = "Choose an option for at least one question";

/// POST /api/tasks/runs/:run_id/answers
///
/// The waiter registry is process-local, so this resolves a question only on
/// the instance running the parked worker; anywhere else the run reads as
/// waiting and the answer finds nothing to deliver to. The same single-instance
/// assumption the run socket already documents holds here.
pub async fn answer_run(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(run_id): Path<Uuid>,
    Json(body): Json<AnswersRequest>,
) -> impl IntoResponse {
    let actor = match user_id(&auth) {
        Ok(actor) => actor,
        Err(response) => return *response,
    };
    let run = match task_access::write(state.db(), run_id, actor).await {
        Ok(Some(run)) => run,
        Ok(None) => return *denied(StatusCode::NOT_FOUND, NOT_FOUND),
        Err(error) => return *database_error(error),
    };
    if run.status != WAITING {
        return *denied(StatusCode::CONFLICT, NOT_WAITING);
    }
    let Some(pending) = run.pending_question else {
        return *denied(StatusCode::CONFLICT, NOT_WAITING);
    };
    let questions = match serde_json::from_value::<Pending>(pending) {
        Ok(pending) => pending.questions,
        Err(error) => return *database_error(error),
    };
    // An empty submission validates but renders to nothing, and resuming a run
    // with a blank user message tells the model less than never answering does.
    // Declining is what letting the window elapse already means.
    match question::render(&questions, &body.answers) {
        Err(rejection) => return *denied(StatusCode::BAD_REQUEST, &rejection),
        Ok(rendered) if rendered.trim().is_empty() => {
            return *denied(StatusCode::BAD_REQUEST, NOTHING_CHOSEN);
        }
        Ok(_) => {}
    }
    let answered = body.answers.len();
    if !question::answer(run_id, body.answers) {
        return *denied(StatusCode::NOT_FOUND, NOT_FOUND);
    }
    (
        StatusCode::ACCEPTED,
        Json(AnswersResponse { run_id, answered }),
    )
        .into_response()
}

/// GET /api/tasks/runs/:run_id/logs
pub async fn get_run_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    match authorize_run(&state, &auth, run_id).await {
        Ok(snapshot) => Json(TaskRunLogsListResponse {
            logs: snapshot
                .logs
                .into_iter()
                .map(TaskRunLogData::from)
                .collect(),
        })
        .into_response(),
        Err(response) => response.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use serde_json::Value;

    fn row(populated: bool) -> tasks::TaskRow {
        let timestamp = NaiveDateTime::parse_from_str("2026-09-04 12:05:28", "%Y-%m-%d %H:%M:%S")
            .expect("valid timestamp");
        tasks::TaskRow {
            id: Uuid::from_u128(1),
            workspace_id: Uuid::from_u128(2),
            created_by: None,
            project_ids: if populated {
                vec![Uuid::from_u128(3)]
            } else {
                vec![]
            },
            title: "Test task".into(),
            description: "Test description".into(),
            acceptance_criteria: populated.then(|| "Checks pass".into()),
            status: "created".into(),
            priority: populated.then_some(5),
            model_name: populated.then(|| "test-model".into()),
            dependencies: populated.then(|| serde_json::json!([Uuid::from_u128(4)])),
            is_agentic: true,
            github_repo_url: populated.then(|| "https://github.com/abnegate/zone".into()),
            source_id: populated.then_some(Uuid::from_u128(5)),
            source_ids: populated.then(|| vec![Uuid::from_u128(5)]),
            worker_id: populated.then(|| "worker-1".into()),
            queued_at: populated.then_some(timestamp),
            started_at: populated.then_some(timestamp),
            completed_at: populated.then_some(timestamp),
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
            pr_url: populated.then(|| "https://github.com/abnegate/zone/pull/1".into()),
            branch_name: populated.then(|| "fix/task".into()),
            pr_status: populated.then(|| "open".into()),
            pr_created_at: populated.then_some(timestamp),
        }
    }

    #[test]
    fn task_response_preserves_nullable_fields() {
        let expected: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/task-created.json")).unwrap();
        assert_eq!(
            serde_json::to_value(TaskResponse::from(row(false))).unwrap(),
            expected
        );
    }

    #[test]
    fn task_response_preserves_populated_fields() {
        let expected: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/task-populated.json")).unwrap();
        assert_eq!(
            serde_json::to_value(TaskResponse::from(row(true))).unwrap(),
            expected
        );
        let list = TasksListResponse {
            tasks: vec![TaskData::from(row(true))],
        };
        assert_eq!(
            serde_json::to_value(list).unwrap()["tasks"][0],
            expected["task"]
        );
    }

    fn run(pending: Option<serde_json::Value>) -> tasks::TaskRunRow {
        tasks::TaskRunRow {
            id: Uuid::from_u128(6),
            task_id: Uuid::from_u128(1),
            triggered_by: None,
            status: if pending.is_some() {
                "waiting".into()
            } else {
                "running".into()
            },
            current_phase: Some("acting".into()),
            progress_percent: Some(40),
            started_at: None,
            completed_at: None,
            error_message: None,
            artifacts: None,
            pending_question: pending,
            pending_wait: None,
        }
    }

    /// The same run, parked on a wait instead of a question.
    fn parked_on_wait(waiting: serde_json::Value) -> tasks::TaskRunRow {
        tasks::TaskRunRow {
            status: "waiting".into(),
            pending_wait: Some(waiting),
            ..run(None)
        }
    }

    #[test]
    fn a_waiting_run_discloses_the_question_it_parked_on() {
        let asked = serde_json::json!({
            "tool_call_id": "ask-call",
            "questions": [{"header": "Scope", "question": "How far back?"}],
        });
        let body = serde_json::to_value(TaskRunResponse::from(run(Some(asked.clone())))).unwrap();
        assert_eq!(body["run"]["status"], "waiting");
        assert_eq!(
            body["run"]["pending_question"], asked,
            "a console that cannot read the question cannot answer it"
        );
        assert_eq!(
            serde_json::to_value(TaskRunResponse::from(run(None))).unwrap()["run"]["pending_question"],
            Value::Null
        );
    }

    /// A wait park reaches the console or it does not exist: the column went in
    /// with the park and nothing carried it to the wire, so a console rendering
    /// `waiting` had a badge and no subject. The absence on a plain row is half
    /// the contract -- the key is what says there is a wait at all, which is
    /// how a reader tells this park from the question beside it.
    #[test]
    fn a_run_parked_on_a_wait_discloses_what_it_waits_for() {
        let waiting = serde_json::json!({
            "kind": "job",
            "id": "job_9f3c1a7b2e04",
            "deadline": "2026-09-13T09:41:00Z",
        });
        let body =
            serde_json::to_value(TaskRunResponse::from(parked_on_wait(waiting.clone()))).unwrap();

        assert_eq!(body["run"]["status"], "waiting");
        assert_eq!(
            body["run"]["waiting_on"], waiting,
            "a console that cannot read the wait cannot say what the run is doing"
        );
        assert_eq!(
            body["run"]["pending_question"],
            Value::Null,
            "a wait is not a question and answering it must stay refused"
        );

        let running = serde_json::to_value(TaskRunResponse::from(run(None))).unwrap();
        assert!(
            running["run"].get("waiting_on").is_none(),
            "a run waiting for nothing carries no key: {running}"
        );
    }

    /// The row has carried both columns since before this route existed and the
    /// run it sends carried neither, so the console ordering a task's earlier
    /// runs by `started_at` compared two blanks and kept whatever order the
    /// list arrived in. Both travel formatted as `TaskData` formats them.
    #[test]
    fn a_run_discloses_when_it_started_and_when_it_finished() {
        let started = NaiveDateTime::parse_from_str("2026-09-13 09:41:00", "%Y-%m-%d %H:%M:%S")
            .expect("valid timestamp");
        let completed = NaiveDateTime::parse_from_str("2026-09-13 09:44:30", "%Y-%m-%d %H:%M:%S")
            .expect("valid timestamp");
        let finished = tasks::TaskRunRow {
            status: "completed".into(),
            started_at: Some(started),
            completed_at: Some(completed),
            ..run(None)
        };

        let body = serde_json::to_value(TaskRunResponse::from(finished)).unwrap();
        assert_eq!(
            body["run"]["started_at"], "2026-09-13T09:41:00+00:00",
            "a console sorting runs by when they started needs the value, not the column"
        );
        assert_eq!(body["run"]["completed_at"], "2026-09-13T09:44:30+00:00");

        let running = serde_json::to_value(TaskRunResponse::from(run(None))).unwrap();
        assert_eq!(
            running["run"]["started_at"],
            Value::Null,
            "a run that has not started sends the key as null, as a task does"
        );
        assert_eq!(running["run"]["completed_at"], Value::Null);
    }
}
