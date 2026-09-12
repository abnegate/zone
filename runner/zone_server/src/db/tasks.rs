//! Task database queries

use chrono::NaiveDateTime;
use sqlx::{PgConnection, PgPool};
use thiserror::Error;
use uuid::Uuid;

use crate::ws::task_run;

use super::{
    DbResult,
    workspace_members::{self, WorkspaceRole},
};

/// Result of a tenant-scoped mutation without exposing resource existence.
#[derive(Debug)]
pub enum Mutation<T> {
    Applied(T),
    NotFound,
}

/// Validation and database failures raised by task mutations.
#[derive(Debug, Error)]
pub enum MutationError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Project is not available in this workspace")]
    Project,
    #[error("Source is not available in this workspace")]
    Source,
    #[error("Task has an active run")]
    ActiveRun,
}

impl MutationError {
    fn into_database(self) -> sqlx::Error {
        match self {
            Self::Database(error) => error,
            error => sqlx::Error::Protocol(error.to_string()),
        }
    }
}

/// Result of creating a task run while serializing active-run checks.
#[derive(Debug)]
pub enum RunMutation {
    Created(TaskRunRow),
    Active(TaskRunRow),
}

/// Values required to create a task and its project associations atomically.
pub struct Create<'a> {
    pub workspace_id: Uuid,
    pub project_ids: &'a [Uuid],
    pub title: &'a str,
    pub description: &'a str,
    pub acceptance_criteria: Option<&'a str>,
    pub priority: Option<i32>,
    pub is_agentic: bool,
    pub source_id: Option<Uuid>,
    /// Recorded as `tasks.created_by`; a task without one gets no agent tools.
    pub created_by: Option<Uuid>,
}

/// Sparse values accepted by an atomic task update.
pub struct Patch<'a> {
    pub id: Uuid,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub acceptance_criteria: Option<&'a str>,
    pub status: Option<&'a str>,
    pub priority: Option<i32>,
    pub project_ids: Option<&'a [Uuid]>,
}

/// Task row from database
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub created_by: Option<Uuid>,
    pub project_ids: Vec<Uuid>, // Associated projects via task_projects join table
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Option<String>,
    pub status: String,
    pub priority: Option<i32>,
    pub model_name: Option<String>,
    pub dependencies: Option<serde_json::Value>,
    pub is_agentic: bool,
    pub github_repo_url: Option<String>,
    pub source_id: Option<Uuid>,
    pub source_ids: Option<Vec<Uuid>>,
    pub worker_id: Option<String>,
    pub queued_at: Option<NaiveDateTime>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    // PR-related fields
    pub pr_url: Option<String>,
    pub branch_name: Option<String>,
    pub pr_status: Option<String>,
    pub pr_created_at: Option<NaiveDateTime>,
}

/// The statuses a run holds while it still owns its lease and its task's
/// admission slot. A run parked on a question is idle, not finished: it keeps
/// heartbeating, keeps blocking a second admission, and stays sweepable.
const ACTIVE_RUN_STATUSES: &str = "('running','waiting')";

/// Why the sweeper failed a run, both in the row it writes and in the frame it
/// publishes to whoever was waiting on that run.
const ORPHANED: &str = "orphaned";

/// The one status a finished run reports as a success.
const COMPLETED: &str = "completed";

/// The status the sweeper writes onto a run whose lease went stale.
const FAILED: &str = "failed";

/// Identity carried by every side effect of a claimed task run.
#[derive(Debug, Clone, Copy)]
pub struct Execution {
    pub task: Uuid,
    pub run: Uuid,
    pub owner: Uuid,
    pub actor: Option<Uuid>,
}

impl Execution {
    /// Legacy tasks may execute without an actor, but can never publish.
    pub async fn authorized(&self, pool: &PgPool, publication: bool) -> DbResult<bool> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM tasks t JOIN task_runs r ON r.task_id=t.id WHERE t.id=$1 AND t.active_run_id=$2 AND r.id=$2 AND r.owner=$3 AND r.triggered_by IS NOT DISTINCT FROM $4 AND r.status IN {ACTIVE_RUN_STATUSES} AND r.heartbeat_at > NOW() - INTERVAL '60 seconds' AND (NOT $5 OR (t.created_by IS NOT NULL AND $4::uuid IS NOT NULL)) AND (($4::uuid IS NULL AND t.created_by IS NULL) OR EXISTS(SELECT 1 FROM workspace_members m WHERE m.workspace_id=t.workspace_id AND m.user_id=$4 AND m.is_active AND m.role IN ('member','admin','owner'))))")))
            .bind(self.task).bind(self.run).bind(self.owner).bind(self.actor).bind(publication).fetch_one(pool).await
    }
}

/// Task run row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskRunRow {
    pub id: Uuid,
    pub task_id: Uuid,
    pub triggered_by: Option<Uuid>,
    pub status: String,
    pub current_phase: Option<String>,
    pub progress_percent: Option<i32>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub artifacts: Option<serde_json::Value>,
    pub pending_question: Option<serde_json::Value>,
    pub pending_wait: Option<serde_json::Value>,
}

/// Task run log row
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskRunLogRow {
    pub id: Uuid,
    pub task_run_id: Uuid,
    pub phase: String,
    pub agent_type: String,
    pub log_level: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<NaiveDateTime>,
}

/// Helper macro to map a row to TaskRow (project_ids populated separately from join table)
macro_rules! map_task_row {
    ($r:expr) => {
        TaskRow {
            id: $r.id,
            workspace_id: $r.workspace_id,
            created_by: $r.created_by,
            project_ids: Vec::new(), // Populated separately from task_projects join table
            title: $r.title,
            description: $r.description,
            acceptance_criteria: $r.acceptance_criteria,
            status: $r.status,
            priority: $r.priority,
            model_name: $r.model_name,
            dependencies: $r.dependencies,
            is_agentic: $r.is_agentic,
            github_repo_url: $r.github_repo_url,
            source_id: $r.source_id,
            source_ids: $r.source_ids,
            worker_id: $r.worker_id,
            queued_at: $r.queued_at,
            started_at: $r.started_at,
            completed_at: $r.completed_at,
            created_at: $r.created_at,
            updated_at: $r.updated_at,
            pr_url: $r.pr_url,
            branch_name: $r.branch_name,
            pr_status: $r.pr_status,
            pr_created_at: $r.pr_created_at,
        }
    };
}

async fn lock_writer(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    Ok(
        workspace_members::lock_role(connection, workspace_id, user_id)
            .await?
            .is_some_and(|role| role >= WorkspaceRole::Member),
    )
}

async fn lock_task_writer(
    connection: &mut PgConnection,
    task_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<Uuid>> {
    let workspace_id: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM tasks WHERE id = $1 FOR UPDATE")
            .bind(task_id)
            .fetch_optional(&mut *connection)
            .await?;
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };

    Ok(lock_writer(connection, workspace_id, user_id)
        .await?
        .then_some(workspace_id))
}

async fn lock_projects(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    project_ids: &[Uuid],
) -> Result<(), MutationError> {
    if project_ids.is_empty() {
        return Ok(());
    }

    let mut expected = project_ids.to_vec();
    expected.sort_unstable();
    expected.dedup();

    let mut found: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM projects
        WHERE workspace_id = $1 AND id = ANY($2)
        ORDER BY id
        FOR SHARE
        "#,
    )
    .bind(workspace_id)
    .bind(&expected)
    .fetch_all(&mut *connection)
    .await?;
    found.sort_unstable();

    if found == expected {
        Ok(())
    } else {
        Err(MutationError::Project)
    }
}

async fn lock_source(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    source_id: Uuid,
) -> DbResult<bool> {
    let source: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM sources WHERE id = $1 AND workspace_id = $2 FOR SHARE")
            .bind(source_id)
            .bind(workspace_id)
            .fetch_optional(&mut *connection)
            .await?;

    Ok(source.is_some())
}

async fn get_task_project_ids_in(
    connection: &mut PgConnection,
    task_id: Uuid,
) -> DbResult<Vec<Uuid>> {
    let rows = sqlx::query!(
        r#"
        SELECT project_id FROM task_projects WHERE task_id = $1 ORDER BY project_id
        "#,
        task_id
    )
    .fetch_all(&mut *connection)
    .await?;

    Ok(rows.into_iter().map(|row| row.project_id).collect())
}

async fn add_task_projects_in(
    connection: &mut PgConnection,
    task_id: Uuid,
    project_ids: &[Uuid],
) -> DbResult<()> {
    for project_id in project_ids {
        sqlx::query!(
            r#"
            INSERT INTO task_projects (task_id, project_id)
            VALUES ($1, $2)
            ON CONFLICT (task_id, project_id) DO NOTHING
            "#,
            task_id,
            project_id
        )
        .execute(&mut *connection)
        .await?;
    }

    Ok(())
}

async fn set_task_projects_in(
    connection: &mut PgConnection,
    task_id: Uuid,
    project_ids: &[Uuid],
) -> DbResult<()> {
    sqlx::query!(r#"DELETE FROM task_projects WHERE task_id = $1"#, task_id)
        .execute(&mut *connection)
        .await?;
    add_task_projects_in(connection, task_id, project_ids).await
}

/// Get project IDs for a task from the join table
pub async fn get_task_project_ids(pool: &PgPool, task_id: Uuid) -> DbResult<Vec<Uuid>> {
    let rows = sqlx::query!(
        r#"
        SELECT project_id FROM task_projects WHERE task_id = $1 ORDER BY project_id
        "#,
        task_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.project_id).collect())
}

/// List tasks with optional filters (workspace_id is required)
pub async fn list_tasks(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    status: Option<&str>,
) -> DbResult<Vec<TaskRow>> {
    let mut tasks = match (project_id, status) {
        (Some(pid), Some(s)) => {
            // Filter by project via join table
            let rows = sqlx::query!(
                r#"
                SELECT DISTINCT t.id, t.title, t.description, t.acceptance_criteria, t.status, t.priority,
                       t.model_name, t.dependencies, t.is_agentic, t.github_repo_url, t.source_id, t.source_ids,
                       t.workspace_id, t.worker_id, t.queued_at, t.started_at, t.completed_at, t.created_at, t.updated_at,
                       t.pr_url, t.branch_name, t.pr_status, t.pr_created_at, t.created_by
                FROM tasks t
                INNER JOIN task_projects tp ON t.id = tp.task_id
                WHERE t.workspace_id = $1 AND tp.project_id = $2 AND t.status = $3
                ORDER BY t.created_at DESC
                "#,
                workspace_id,
                pid,
                s
            )
            .fetch_all(pool)
            .await?;
            rows.into_iter()
                .map(|r| map_task_row!(r))
                .collect::<Vec<_>>()
        }
        (Some(pid), None) => {
            // Filter by project via join table
            let rows = sqlx::query!(
                r#"
                SELECT DISTINCT t.id, t.title, t.description, t.acceptance_criteria, t.status, t.priority,
                       t.model_name, t.dependencies, t.is_agentic, t.github_repo_url, t.source_id, t.source_ids,
                       t.workspace_id, t.worker_id, t.queued_at, t.started_at, t.completed_at, t.created_at, t.updated_at,
                       t.pr_url, t.branch_name, t.pr_status, t.pr_created_at, t.created_by
                FROM tasks t
                INNER JOIN task_projects tp ON t.id = tp.task_id
                WHERE t.workspace_id = $1 AND tp.project_id = $2
                ORDER BY t.created_at DESC
                "#,
                workspace_id,
                pid
            )
            .fetch_all(pool)
            .await?;
            rows.into_iter()
                .map(|r| map_task_row!(r))
                .collect::<Vec<_>>()
        }
        (None, Some(s)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, title, description, acceptance_criteria, status, priority,
                       model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                       workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                       pr_url, branch_name, pr_status, pr_created_at, created_by
                FROM tasks
                WHERE workspace_id = $1 AND status = $2
                ORDER BY created_at DESC
                "#,
                workspace_id,
                s
            )
            .fetch_all(pool)
            .await?;
            rows.into_iter()
                .map(|r| map_task_row!(r))
                .collect::<Vec<_>>()
        }
        (None, None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, title, description, acceptance_criteria, status, priority,
                       model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                       workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                       pr_url, branch_name, pr_status, pr_created_at, created_by
                FROM tasks
                WHERE workspace_id = $1
                ORDER BY created_at DESC
                "#,
                workspace_id
            )
            .fetch_all(pool)
            .await?;
            rows.into_iter()
                .map(|r| map_task_row!(r))
                .collect::<Vec<_>>()
        }
    };

    // Populate project_ids for each task
    for task in &mut tasks {
        task.project_ids = get_task_project_ids(pool, task.id).await?;
    }

    Ok(tasks)
}

/// Get task by ID
pub async fn get_task(pool: &PgPool, id: Uuid) -> DbResult<Option<TaskRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, title, description, acceptance_criteria, status, priority,
               model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
               workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
               pr_url, branch_name, pr_status, pr_created_at, created_by
        FROM tasks
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => {
            let mut task = map_task_row!(r);
            task.project_ids = get_task_project_ids(pool, task.id).await?;
            Ok(Some(task))
        }
        None => Ok(None),
    }
}

/// Create a new task
pub async fn create_task(
    pool: &PgPool,
    workspace_id: Uuid,
    project_ids: &[Uuid],
    title: &str,
    description: &str,
    acceptance_criteria: Option<&str>,
    priority: Option<i32>,
    is_agentic: bool,
    source_id: Option<Uuid>,
) -> DbResult<TaskRow> {
    let input = Create {
        workspace_id,
        project_ids,
        title,
        description,
        acceptance_criteria,
        priority,
        is_agentic,
        source_id,
        created_by: None,
    };
    let mut transaction = pool.begin().await?;
    lock_projects(&mut transaction, input.workspace_id, input.project_ids)
        .await
        .map_err(MutationError::into_database)?;
    let task = insert_task(&mut transaction, &input).await?;
    transaction.commit().await?;
    Ok(task)
}

pub async fn create_task_authorized(
    pool: &PgPool,
    user_id: Uuid,
    input: Create<'_>,
) -> Result<Mutation<TaskRow>, MutationError> {
    let mut transaction = pool.begin().await?;
    if !lock_writer(&mut transaction, input.workspace_id, user_id).await? {
        return Ok(Mutation::NotFound);
    }
    if let Some(source_id) = input.source_id
        && !lock_source(&mut transaction, input.workspace_id, source_id).await?
    {
        return Err(MutationError::Source);
    }
    lock_projects(&mut transaction, input.workspace_id, input.project_ids).await?;
    let task = insert_task(&mut transaction, &input).await?;
    transaction.commit().await?;
    Ok(Mutation::Applied(task))
}

pub async fn start_task_authorized(
    pool: &PgPool,
    user_id: Uuid,
    input: Create<'_>,
) -> Result<Mutation<(TaskRow, TaskRunRow)>, MutationError> {
    let input = Create {
        is_agentic: true,
        ..input
    };
    let mut transaction = pool.begin().await?;
    if !lock_writer(&mut transaction, input.workspace_id, user_id).await? {
        return Ok(Mutation::NotFound);
    }
    if let Some(source_id) = input.source_id {
        let active: Option<Option<bool>> = sqlx::query_scalar(
            "SELECT is_active FROM sources WHERE id = $1 AND workspace_id = $2 FOR SHARE",
        )
        .bind(source_id)
        .bind(input.workspace_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if !matches!(active, Some(None) | Some(Some(true))) {
            return Err(MutationError::Source);
        }
    }
    lock_projects(&mut transaction, input.workspace_id, input.project_ids).await?;
    let task = insert_task(&mut transaction, &input).await?;
    let run = insert_task_run(&mut transaction, task.id, Some(user_id)).await?;
    transaction.commit().await?;
    Ok(Mutation::Applied((task, run)))
}

async fn insert_task(connection: &mut PgConnection, input: &Create<'_>) -> DbResult<TaskRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO tasks (workspace_id, title, description, acceptance_criteria, priority, is_agentic, source_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at, created_by
        "#,
        input.workspace_id,
        input.title,
        input.description,
        input.acceptance_criteria,
        input.priority,
        input.is_agentic,
        input.source_id,
        input.created_by
    )
    .fetch_one(&mut *connection)
    .await?;

    let mut task = map_task_row!(row);

    if !input.project_ids.is_empty() {
        add_task_projects_in(connection, task.id, input.project_ids).await?;
    }
    task.project_ids = get_task_project_ids_in(connection, task.id).await?;

    Ok(task)
}

/// Update a task
pub async fn update_task(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
    description: Option<&str>,
    acceptance_criteria: Option<&str>,
    status: Option<&str>,
    priority: Option<i32>,
    project_ids: Option<&[Uuid]>,
) -> DbResult<Option<TaskRow>> {
    let input = Patch {
        id,
        title,
        description,
        acceptance_criteria,
        status,
        priority,
        project_ids,
    };
    let mut transaction = pool.begin().await?;
    let workspace_id: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM tasks WHERE id = $1 FOR UPDATE")
            .bind(input.id)
            .fetch_optional(&mut *transaction)
            .await?;
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    if let Some(project_ids) = input.project_ids {
        lock_projects(&mut transaction, workspace_id, project_ids)
            .await
            .map_err(MutationError::into_database)?;
    }
    let task = update_task_in(&mut transaction, &input).await?;
    transaction.commit().await?;
    Ok(task)
}

pub async fn update_task_authorized(
    pool: &PgPool,
    user_id: Uuid,
    input: Patch<'_>,
) -> Result<Mutation<TaskRow>, MutationError> {
    let mut transaction = pool.begin().await?;
    let Some(workspace_id) = lock_task_writer(&mut transaction, input.id, user_id).await? else {
        return Ok(Mutation::NotFound);
    };
    if let Some(project_ids) = input.project_ids {
        lock_projects(&mut transaction, workspace_id, project_ids).await?;
    }
    let Some(task) = update_task_in(&mut transaction, &input).await? else {
        return Ok(Mutation::NotFound);
    };
    transaction.commit().await?;
    Ok(Mutation::Applied(task))
}

/// Whether the run lifecycle currently owns this task's status.
///
/// The worker advances `tasks.status` from the run, so a caller-supplied status
/// while a run is live would race those transitions. A missing task reads the
/// same way as one whose status is not the caller's to set.
async fn run_is_active(
    connection: &mut PgConnection,
    task_id: Uuid,
    changes_status: bool,
) -> DbResult<bool> {
    let active: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT active_run_id FROM tasks WHERE id = $1 FOR UPDATE")
            .bind(task_id)
            .fetch_optional(&mut *connection)
            .await?;
    Ok(active.is_none() || (changes_status && active.flatten().is_some()))
}

async fn update_task_in(
    connection: &mut PgConnection,
    input: &Patch<'_>,
) -> DbResult<Option<TaskRow>> {
    if run_is_active(connection, input.id, input.status.is_some()).await? {
        return Ok(None);
    }
    let row = sqlx::query!(
        r#"
        UPDATE tasks
        SET title = COALESCE($2, title),
            description = COALESCE($3, description),
            acceptance_criteria = COALESCE($4, acceptance_criteria),
            status = COALESCE($5, status),
            priority = COALESCE($6, priority),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at, created_by
        "#,
        input.id,
        input.title,
        input.description,
        input.acceptance_criteria,
        input.status,
        input.priority
    )
    .fetch_optional(&mut *connection)
    .await?;

    match row {
        Some(r) => {
            let mut task = map_task_row!(r);

            if let Some(project_ids) = input.project_ids {
                set_task_projects_in(connection, input.id, project_ids).await?;
                task.project_ids = get_task_project_ids_in(connection, task.id).await?;
            } else {
                task.project_ids = get_task_project_ids_in(connection, task.id).await?;
            }

            Ok(Some(task))
        }
        None => Ok(None),
    }
}

/// Delete a task
pub async fn delete_task(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    delete_task_as(pool, id, None).await
}

pub async fn delete_task_as(pool: &PgPool, id: Uuid, actor: Option<Uuid>) -> DbResult<bool> {
    let mut transaction = pool.begin().await?;
    if let Some(actor) = actor {
        authorize_task_in(&mut transaction, id, actor, true).await?;
    }
    let result = sqlx::query!("DELETE FROM tasks WHERE id = $1", id)
        .execute(&mut *transaction)
        .await?;

    transaction.commit().await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_task_authorized(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> DbResult<Mutation<()>> {
    let mut transaction = pool.begin().await?;
    if lock_task_writer(&mut transaction, id, user_id)
        .await?
        .is_none()
    {
        return Ok(Mutation::NotFound);
    }
    sqlx::query!("DELETE FROM tasks WHERE id = $1", id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Mutation::Applied(()))
}

/// Queue a task for execution
pub async fn queue_task(pool: &PgPool, id: Uuid) -> DbResult<Option<TaskRow>> {
    queue_task_as(pool, id, None).await
}

pub async fn queue_task_as(
    pool: &PgPool,
    id: Uuid,
    actor: Option<Uuid>,
) -> DbResult<Option<TaskRow>> {
    let mut transaction = pool.begin().await?;
    if let Some(actor) = actor {
        authorize_task_in(&mut transaction, id, actor, true).await?;
    }
    let active: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT active_run_id FROM tasks WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
    if active.is_none() || active.flatten().is_some() {
        return Ok(None);
    }
    let row = sqlx::query!(
        r#"
        UPDATE tasks
        SET status = 'queued',
            queued_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at, created_by
        "#,
        id
    )
    .fetch_optional(&mut *transaction)
    .await?;

    let result = match row {
        Some(r) => {
            let mut task = map_task_row!(r);
            task.project_ids =
                sqlx::query_scalar("SELECT project_id FROM task_projects WHERE task_id=$1")
                    .bind(task.id)
                    .fetch_all(&mut *transaction)
                    .await?;
            Ok(Some(task))
        }
        None => Ok(None),
    };
    transaction.commit().await?;
    result
}

/// Membership locks always precede task locks in user-triggered operations.
async fn authorize_task_in(
    connection: &mut PgConnection,
    task: Uuid,
    actor: Uuid,
    write: bool,
) -> DbResult<()> {
    let workspace: Uuid = sqlx::query_scalar("SELECT workspace_id FROM tasks WHERE id=$1")
        .bind(task)
        .fetch_one(&mut *connection)
        .await?;
    super::actions::authorize(connection, workspace, actor, write).await
}

pub async fn queue_task_authorized(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Mutation<TaskRow>, MutationError> {
    let mut transaction = pool.begin().await?;
    if lock_task_writer(&mut transaction, id, user_id)
        .await?
        .is_none()
    {
        return Ok(Mutation::NotFound);
    }
    // Re-queueing a task the worker is already running would reset the status
    // underneath it, so an active run is a conflict rather than a refusal.
    if run_is_active(&mut transaction, id, true).await? {
        return Err(MutationError::ActiveRun);
    }
    let Some(task) = queue_task_in(&mut transaction, id).await? else {
        return Ok(Mutation::NotFound);
    };
    transaction.commit().await?;
    Ok(Mutation::Applied(task))
}

async fn queue_task_in(connection: &mut PgConnection, id: Uuid) -> DbResult<Option<TaskRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE tasks
        SET status = 'queued',
            queued_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at, created_by
        "#,
        id
    )
    .fetch_optional(&mut *connection)
    .await?;

    match row {
        Some(row) => {
            let mut task = map_task_row!(row);
            task.project_ids = get_task_project_ids_in(connection, task.id).await?;
            Ok(Some(task))
        }
        None => Ok(None),
    }
}

/// Create a new task run
pub async fn create_task_run(pool: &PgPool, task_id: Uuid) -> DbResult<TaskRunRow> {
    let mut transaction = pool.begin().await?;
    let run = insert_task_run(&mut transaction, task_id, None).await?;
    transaction.commit().await?;
    Ok(run)
}

pub async fn create_task_run_authorized(
    pool: &PgPool,
    user_id: Uuid,
    task_id: Uuid,
) -> DbResult<Mutation<RunMutation>> {
    let mut transaction = pool.begin().await?;
    if lock_task_writer(&mut transaction, task_id, user_id)
        .await?
        .is_none()
    {
        return Ok(Mutation::NotFound);
    }

    let active = sqlx::query_as::<_, TaskRunRow>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT id, task_id, triggered_by, status, current_phase, progress_percent,
               started_at, completed_at, error_message, artifacts, pending_question,
               pending_wait
        FROM task_runs
        WHERE task_id = $1 AND status IN {ACTIVE_RUN_STATUSES}
        ORDER BY started_at DESC, id
        LIMIT 1
        "#
    )))
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some(run) = active {
        return Ok(Mutation::Applied(RunMutation::Active(run)));
    }

    let run = insert_task_run(&mut transaction, task_id, Some(user_id)).await?;
    transaction.commit().await?;
    Ok(Mutation::Applied(RunMutation::Created(run)))
}

/// Admit one run while holding the task row, so the insert and
/// `tasks.active_run_id` cannot disagree. Authorization belongs to the caller.
async fn insert_task_run(
    connection: &mut PgConnection,
    task_id: Uuid,
    triggered_by: Option<Uuid>,
) -> DbResult<TaskRunRow> {
    sqlx::query("SELECT id FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id)
        .fetch_one(&mut *connection)
        .await?;
    let row = sqlx::query!(
        "INSERT INTO task_runs (task_id, status, triggered_by) VALUES ($1, 'running', $2) RETURNING id, task_id, status, current_phase, progress_percent, started_at, completed_at, error_message, artifacts, triggered_by, pending_question, pending_wait",
        task_id,
        triggered_by
    )
    .fetch_one(&mut *connection)
    .await?;
    sqlx::query("UPDATE tasks SET active_run_id = $2, status = 'queued', queued_at = NOW(), completed_at = NULL, updated_at = NOW() WHERE id = $1")
        .bind(task_id)
        .bind(row.id)
        .execute(&mut *connection)
        .await?;
    Ok(TaskRunRow {
        id: row.id,
        task_id: row.task_id,
        triggered_by: row.triggered_by,
        status: row.status,
        current_phase: row.current_phase,
        progress_percent: row.progress_percent,
        started_at: row.started_at,
        completed_at: row.completed_at,
        error_message: row.error_message,
        artifacts: row.artifacts,
        pending_question: row.pending_question,
        pending_wait: row.pending_wait,
    })
}

/// Create a task attributed to an actor, refusing rather than reporting absence.
///
/// The counterpart to [`create_task_run_as`]: callers here are workers and
/// fixtures that already know the actor is a member, so a denial is a fault to
/// propagate, not a `NotFound` for a route to translate.
#[allow(clippy::too_many_arguments)]
pub async fn create_task_as(
    pool: &PgPool,
    workspace_id: Uuid,
    project_ids: &[Uuid],
    title: &str,
    description: &str,
    acceptance_criteria: Option<&str>,
    priority: Option<i32>,
    is_agentic: bool,
    source_id: Option<Uuid>,
    actor: Option<Uuid>,
) -> DbResult<TaskRow> {
    let input = Create {
        workspace_id,
        project_ids,
        title,
        description,
        acceptance_criteria,
        priority,
        is_agentic,
        source_id,
        created_by: actor,
    };
    let Some(actor) = actor else {
        let mut transaction = pool.begin().await?;
        lock_projects(&mut transaction, workspace_id, project_ids)
            .await
            .map_err(MutationError::into_database)?;
        let task = insert_task(&mut transaction, &input).await?;
        transaction.commit().await?;
        return Ok(task);
    };
    match create_task_authorized(pool, actor, input).await {
        Ok(Mutation::Applied(task)) => Ok(task),
        Ok(Mutation::NotFound) => Err(super::actions::invalid("Workspace access denied")),
        Err(error) => Err(MutationError::into_database(error)),
    }
}

/// Admit one run for an actor that has not been authorized yet.
pub async fn create_task_run_as(
    pool: &PgPool,
    task_id: Uuid,
    actor: Option<Uuid>,
) -> DbResult<TaskRunRow> {
    let mut transaction = pool.begin().await?;
    let run = create_task_run_in(&mut transaction, task_id, actor).await?;
    transaction.commit().await?;
    Ok(run)
}

pub(super) async fn create_task_run_in(
    connection: &mut PgConnection,
    task_id: Uuid,
    actor: Option<Uuid>,
) -> DbResult<TaskRunRow> {
    if let Some(actor) = actor {
        authorize_task_in(connection, task_id, actor, true).await?;
    }
    insert_task_run(connection, task_id, actor).await
}

/// Claim a newly admitted run exactly once, before waiting for capacity.
pub async fn claim_task_run(pool: &PgPool, run_id: Uuid, owner: Uuid) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE task_runs SET owner = $2, heartbeat_at = NOW() WHERE id = $1 AND status = 'running' AND owner IS NULL AND heartbeat_at > NOW() - INTERVAL '60 seconds'")
        .bind(run_id).bind(owner).execute(pool).await?.rows_affected() == 1)
}

/// Refresh only the live lease held by this execution.
pub async fn heartbeat_task_run(pool: &PgPool, run_id: Uuid, owner: Uuid) -> DbResult<bool> {
    Ok(sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE task_runs SET heartbeat_at = NOW() WHERE id = $1 AND owner = $2 AND status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at > NOW() - INTERVAL '60 seconds'")))
        .bind(run_id).bind(owner).execute(pool).await?.rows_affected() == 1)
}

/// Park a live run on a question without giving up its lease or its slot.
///
/// Fenced on `'running'` so the same question cannot park a run twice, and on a
/// fresh heartbeat so a run the sweeper is about to orphan is never revived.
pub async fn park_task_run(
    pool: &PgPool,
    run: Uuid,
    owner: Uuid,
    pending_question: serde_json::Value,
) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE task_runs SET status = 'waiting', current_phase = 'waiting', pending_question = $3, heartbeat_at = NOW() WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status = 'running' AND heartbeat_at > NOW() - INTERVAL '60 seconds'")
        .bind(run).bind(owner).bind(pending_question).execute(pool).await?.rows_affected() == 1)
}

/// Park a live run on something outside the loop, without giving up its lease
/// or its slot.
///
/// The sibling of [`park_task_run`], fenced identically, taking the phase with
/// it so nothing reads a stale one beside a waiting run, and deliberately
/// leaving `pending_question` NULL: a run waiting on a job or a check has
/// nothing to answer, so `answer_run` keeps refusing it.
pub async fn park_task_run_waiting(
    pool: &PgPool,
    run: Uuid,
    owner: Uuid,
    pending_wait: serde_json::Value,
) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE task_runs SET status = 'waiting', current_phase = 'waiting', pending_wait = $3, heartbeat_at = NOW() WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status = 'running' AND heartbeat_at > NOW() - INTERVAL '60 seconds'")
        .bind(run).bind(owner).bind(pending_wait).execute(pool).await?.rows_affected() == 1)
}

/// Return a parked run to execution, clearing what it waited on.
///
/// Fenced on `'waiting'`, so a second answer for the same question is a miss
/// rather than a resume of a run that has already moved on. Clears both parks:
/// a run holds at most one, and whichever it held is over.
pub async fn resume_task_run(pool: &PgPool, run: Uuid, owner: Uuid) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE task_runs SET status = 'running', current_phase = NULL, pending_question = NULL, pending_wait = NULL, heartbeat_at = NOW() WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status = 'waiting'")
        .bind(run).bind(owner).execute(pool).await?.rows_affected() == 1)
}

pub async fn start_task_run(pool: &PgPool, run_id: Uuid) -> DbResult<bool> {
    start_owned_task_run(pool, run_id, None).await
}

pub async fn start_owned_task_run(
    pool: &PgPool,
    run_id: Uuid,
    owner: Option<Uuid>,
) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE tasks SET status = 'in_progress', started_at = NOW(), updated_at = NOW() WHERE active_run_id = $1 AND EXISTS (SELECT 1 FROM task_runs WHERE id = $1 AND status = 'running' AND owner IS NOT DISTINCT FROM $2 AND heartbeat_at > NOW() - INTERVAL '60 seconds')")
        .bind(run_id).bind(owner).execute(pool).await?.rows_affected() == 1)
}

pub async fn owns_task_run(pool: &PgPool, run_id: Uuid, owner: Option<Uuid>) -> DbResult<bool> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT EXISTS(SELECT 1 FROM task_runs WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at > NOW() - INTERVAL '60 seconds')")))
        .bind(run_id).bind(owner).fetch_one(pool).await
}

/// Fail stale runs and their owning tasks without racing a newly admitted run.
pub async fn sweep_task_runs(pool: &PgPool) -> DbResult<u64> {
    let mut transaction = pool.begin().await?;
    let locked: Vec<Option<Uuid>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT active_run_id FROM tasks WHERE active_run_id IN (SELECT id FROM task_runs WHERE status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at <= NOW() - INTERVAL '60 seconds') ORDER BY id FOR UPDATE")))
        .fetch_all(&mut *transaction).await?;
    let runs: Vec<Uuid> = locked.into_iter().flatten().collect();
    let failed: Vec<(Uuid, Uuid)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE task_runs SET status = 'failed', error_message = $2, completed_at = NOW(), current_phase = NULLIF(current_phase, 'waiting'), pending_question = NULL, pending_wait = NULL WHERE id = ANY($1) AND status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at <= NOW() - INTERVAL '60 seconds' RETURNING id, task_id")))
        .bind(&runs).bind(ORPHANED).fetch_all(&mut *transaction).await?;
    for (run, task) in &failed {
        sqlx::query("UPDATE tasks SET status = 'blocked', completed_at = NOW(), updated_at = NOW(), active_run_id = NULL WHERE id = $1 AND active_run_id = $2")
            .bind(task).bind(run).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    for (run, _) in &failed {
        task_run::publish_terminal(*run, FAILED, Some(ORPHANED));
    }
    Ok(failed.len() as u64)
}

/// Update task run progress
pub async fn update_task_run_progress(
    pool: &PgPool,
    run_id: Uuid,
    current_phase: Option<&str>,
    progress_percent: Option<i32>,
) -> DbResult<Option<TaskRunRow>> {
    update_owned_task_run_progress(pool, run_id, None, current_phase, progress_percent).await
}

pub async fn update_owned_task_run_progress(
    pool: &PgPool,
    run_id: Uuid,
    owner: Option<Uuid>,
    current_phase: Option<&str>,
    progress_percent: Option<i32>,
) -> DbResult<Option<TaskRunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE task_runs SET current_phase = CASE WHEN status = 'waiting' THEN current_phase ELSE COALESCE($3, current_phase) END, progress_percent = COALESCE($4, progress_percent) WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at > NOW() - INTERVAL '60 seconds' RETURNING id, task_id, status, current_phase, progress_percent, started_at, completed_at, error_message, artifacts, triggered_by, pending_question, pending_wait")))
        .bind(run_id).bind(owner).bind(current_phase).bind(progress_percent).fetch_optional(pool).await
}

/// Legacy callers may only finish an unclaimed run.
pub async fn complete_task_run(
    pool: &PgPool,
    run_id: Uuid,
    status: &str,
    error_message: Option<&str>,
    artifacts: Option<serde_json::Value>,
) -> DbResult<Option<TaskRunRow>> {
    complete_owned_task_run(pool, run_id, None, status, error_message, artifacts).await
}

pub async fn complete_owned_task_run(
    pool: &PgPool,
    run_id: Uuid,
    owner: Option<Uuid>,
    status: &str,
    error_message: Option<&str>,
    artifacts: Option<serde_json::Value>,
) -> DbResult<Option<TaskRunRow>> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "SELECT id FROM tasks WHERE id = (SELECT task_id FROM task_runs WHERE id = $1) FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let row: Option<TaskRunRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("UPDATE task_runs SET status = $3, completed_at = NOW(), error_message = $4, artifacts = COALESCE($5, artifacts), progress_percent = 100, current_phase = NULLIF(current_phase, 'waiting'), pending_question = NULL, pending_wait = NULL WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at > NOW() - INTERVAL '60 seconds' RETURNING id, task_id, status, current_phase, progress_percent, started_at, completed_at, error_message, artifacts, triggered_by, pending_question, pending_wait")))
        .bind(run_id).bind(owner).bind(status).bind(error_message).bind(artifacts).fetch_optional(&mut *transaction).await?;
    if let Some(run) = &row {
        let status = if status == COMPLETED {
            "review"
        } else {
            "blocked"
        };
        sqlx::query("UPDATE tasks SET status = $3, completed_at = NOW(), updated_at = NOW(), active_run_id = NULL WHERE id = $1 AND active_run_id = $2").bind(run.task_id).bind(run.id).bind(status).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    if let Some(run) = &row {
        task_run::publish_terminal(run.id, &run.status, run.error_message.as_deref());
    }
    Ok(row)
}

pub async fn add_owned_task_run_log(
    pool: &PgPool,
    run_id: Uuid,
    owner: Option<Uuid>,
    phase: &str,
    agent: &str,
    level: &str,
    message: &str,
    metadata: Option<serde_json::Value>,
) -> DbResult<bool> {
    Ok(sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO task_run_logs(task_run_id, phase, agent_type, log_level, message, metadata) SELECT id, $3, $4, $5, $6, $7 FROM task_runs WHERE id = $1 AND owner IS NOT DISTINCT FROM $2 AND status IN {ACTIVE_RUN_STATUSES} AND heartbeat_at > NOW() - INTERVAL '60 seconds'")))
        .bind(run_id).bind(owner).bind(phase).bind(agent).bind(level).bind(message).bind(metadata).execute(pool).await?.rows_affected() == 1)
}

/// List task runs for a task
pub async fn list_task_runs(pool: &PgPool, task_id: Uuid) -> DbResult<Vec<TaskRunRow>> {
    list_task_runs_as(pool, task_id, None).await
}

pub async fn list_task_runs_as(
    pool: &PgPool,
    task_id: Uuid,
    actor: Option<Uuid>,
) -> DbResult<Vec<TaskRunRow>> {
    let mut transaction = pool.begin().await?;
    if let Some(actor) = actor {
        authorize_task_in(&mut transaction, task_id, actor, false).await?;
    }
    let rows = sqlx::query!(
        r#"
        SELECT id, task_id, status, current_phase, progress_percent, started_at,
               completed_at, error_message, artifacts, triggered_by, pending_question,
               pending_wait
        FROM task_runs
        WHERE task_id = $1
        ORDER BY started_at DESC
        "#,
        task_id
    )
    .fetch_all(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(rows
        .into_iter()
        .map(|r| TaskRunRow {
            id: r.id,
            task_id: r.task_id,
            triggered_by: r.triggered_by,
            status: r.status,
            current_phase: r.current_phase,
            progress_percent: r.progress_percent,
            started_at: r.started_at,
            completed_at: r.completed_at,
            error_message: r.error_message,
            artifacts: r.artifacts,
            pending_question: r.pending_question,
            pending_wait: r.pending_wait,
        })
        .collect())
}

/// Get task run by ID
pub async fn get_task_run<'connection, E>(
    connection: E,
    run_id: Uuid,
) -> DbResult<Option<TaskRunRow>>
where
    E: sqlx::Executor<'connection, Database = sqlx::Postgres>,
{
    let row = sqlx::query!(
        r#"
        SELECT id, task_id, status, current_phase, progress_percent, started_at,
               completed_at, error_message, artifacts, triggered_by, pending_question,
               pending_wait
        FROM task_runs
        WHERE id = $1
        "#,
        run_id
    )
    .fetch_optional(connection)
    .await?;

    Ok(row.map(|r| TaskRunRow {
        id: r.id,
        task_id: r.task_id,
        triggered_by: r.triggered_by,
        status: r.status,
        current_phase: r.current_phase,
        progress_percent: r.progress_percent,
        started_at: r.started_at,
        completed_at: r.completed_at,
        error_message: r.error_message,
        artifacts: r.artifacts,
        pending_question: r.pending_question,
        pending_wait: r.pending_wait,
    }))
}

/// Add a log entry to a task run
pub async fn add_task_run_log(
    pool: &PgPool,
    task_run_id: Uuid,
    phase: &str,
    agent_type: &str,
    log_level: &str,
    message: &str,
    metadata: Option<serde_json::Value>,
) -> DbResult<TaskRunLogRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO task_run_logs (task_run_id, phase, agent_type, log_level, message, metadata)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, task_run_id, phase, agent_type, log_level, message, metadata, created_at
        "#,
        task_run_id,
        phase,
        agent_type,
        log_level,
        message,
        metadata
    )
    .fetch_one(pool)
    .await?;

    Ok(TaskRunLogRow {
        id: row.id,
        task_run_id: row.task_run_id,
        phase: row.phase,
        agent_type: row.agent_type,
        log_level: row.log_level,
        message: row.message,
        metadata: row.metadata,
        created_at: row.created_at,
    })
}

/// Get logs for a task run
pub async fn get_task_run_logs<'connection, E>(
    connection: E,
    task_run_id: Uuid,
) -> DbResult<Vec<TaskRunLogRow>>
where
    E: sqlx::Executor<'connection, Database = sqlx::Postgres>,
{
    sqlx::query_as("SELECT id,task_run_id,phase,agent_type,log_level,message,metadata,created_at FROM task_run_logs WHERE task_run_id=$1 ORDER BY created_at ASC,id ASC")
        .bind(task_run_id).fetch_all(connection).await
}

/// Store publication metadata only while the same writer still owns the run.
pub async fn update_task_pr(
    pool: &PgPool,
    execution: &Execution,
    url: &str,
    branch: &str,
    status: &str,
) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE tasks t SET pr_url=$5, branch_name=$6, pr_status=$7, pr_created_at=NOW(), updated_at=NOW() FROM task_runs r, workspace_members m WHERE t.id=$1 AND t.active_run_id=$2 AND r.id=$2 AND r.task_id=t.id AND r.owner=$3 AND r.triggered_by=$4 AND r.status='running' AND r.heartbeat_at > NOW() - INTERVAL '60 seconds' AND t.created_by IS NOT NULL AND m.workspace_id=t.workspace_id AND m.user_id=$4 AND m.is_active AND m.role IN ('member','admin','owner')")
        .bind(execution.task).bind(execution.run).bind(execution.owner).bind(execution.actor).bind(url).bind(branch).bind(status).execute(pool).await?.rows_affected() == 1)
}

pub async fn update_task_branch(
    pool: &PgPool,
    execution: &Execution,
    branch: &str,
) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE tasks t SET branch_name=$5, updated_at=NOW() FROM task_runs r, workspace_members m WHERE t.id=$1 AND t.active_run_id=$2 AND r.id=$2 AND r.task_id=t.id AND r.owner=$3 AND r.triggered_by=$4 AND r.status='running' AND r.heartbeat_at > NOW() - INTERVAL '60 seconds' AND t.created_by IS NOT NULL AND m.workspace_id=t.workspace_id AND m.user_id=$4 AND m.is_active AND m.role IN ('member','admin','owner')")
        .bind(execution.task).bind(execution.run).bind(execution.owner).bind(execution.actor).bind(branch).execute(pool).await?.rows_affected() == 1)
}
