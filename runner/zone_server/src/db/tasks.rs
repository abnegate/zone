//! Task database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Task row from database
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: Uuid,
    pub project_id: Uuid,
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
    pub workspace_id: Option<Uuid>,
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

/// Task run row from database
#[derive(Debug, Clone)]
pub struct TaskRunRow {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub current_phase: Option<String>,
    pub progress_percent: Option<i32>,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub artifacts: Option<serde_json::Value>,
}

/// Task run log row
#[derive(Debug, Clone)]
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

/// Helper macro to map a row to TaskRow
macro_rules! map_task_row {
    ($r:expr) => {
        TaskRow {
            id: $r.id,
            project_id: $r.project_id,
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
            workspace_id: $r.workspace_id,
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

/// List tasks with optional filters
pub async fn list_tasks(
    pool: &PgPool,
    project_id: Option<Uuid>,
    status: Option<&str>,
) -> DbResult<Vec<TaskRow>> {
    match (project_id, status) {
        (Some(pid), Some(s)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, project_id, title, description, acceptance_criteria, status, priority,
                       model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                       workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                       pr_url, branch_name, pr_status, pr_created_at
                FROM tasks
                WHERE project_id = $1 AND status = $2
                ORDER BY created_at DESC
                "#,
                pid,
                s
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_task_row!(r)).collect())
        }
        (Some(pid), None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, project_id, title, description, acceptance_criteria, status, priority,
                       model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                       workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                       pr_url, branch_name, pr_status, pr_created_at
                FROM tasks
                WHERE project_id = $1
                ORDER BY created_at DESC
                "#,
                pid
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_task_row!(r)).collect())
        }
        (None, Some(s)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, project_id, title, description, acceptance_criteria, status, priority,
                       model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                       workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                       pr_url, branch_name, pr_status, pr_created_at
                FROM tasks
                WHERE status = $1
                ORDER BY created_at DESC
                "#,
                s
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_task_row!(r)).collect())
        }
        (None, None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, project_id, title, description, acceptance_criteria, status, priority,
                       model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                       workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                       pr_url, branch_name, pr_status, pr_created_at
                FROM tasks
                ORDER BY created_at DESC
                "#
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_task_row!(r)).collect())
        }
    }
}

/// Get task by ID
pub async fn get_task(pool: &PgPool, id: Uuid) -> DbResult<Option<TaskRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, project_id, title, description, acceptance_criteria, status, priority,
               model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
               workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
               pr_url, branch_name, pr_status, pr_created_at
        FROM tasks
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| map_task_row!(r)))
}

/// Create a new task
pub async fn create_task(
    pool: &PgPool,
    project_id: Uuid,
    title: &str,
    description: &str,
    acceptance_criteria: Option<&str>,
    priority: Option<i32>,
    is_agentic: bool,
) -> DbResult<TaskRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO tasks (project_id, title, description, acceptance_criteria, priority, is_agentic)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, project_id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at
        "#,
        project_id,
        title,
        description,
        acceptance_criteria,
        priority,
        is_agentic
    )
    .fetch_one(pool)
    .await?;

    Ok(map_task_row!(row))
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
) -> DbResult<Option<TaskRow>> {
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
        RETURNING id, project_id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at
        "#,
        id,
        title,
        description,
        acceptance_criteria,
        status,
        priority
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| map_task_row!(r)))
}

/// Delete a task
pub async fn delete_task(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM tasks WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Queue a task for execution
pub async fn queue_task(pool: &PgPool, id: Uuid) -> DbResult<Option<TaskRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE tasks
        SET status = 'queued',
            queued_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, project_id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| map_task_row!(r)))
}

/// Create a new task run
pub async fn create_task_run(pool: &PgPool, task_id: Uuid) -> DbResult<TaskRunRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO task_runs (task_id, status)
        VALUES ($1, 'running')
        RETURNING id, task_id, status, current_phase, progress_percent, started_at,
                  completed_at, error_message, artifacts
        "#,
        task_id
    )
    .fetch_one(pool)
    .await?;

    Ok(TaskRunRow {
        id: row.id,
        task_id: row.task_id,
        status: row.status,
        current_phase: row.current_phase,
        progress_percent: row.progress_percent,
        started_at: row.started_at,
        completed_at: row.completed_at,
        error_message: row.error_message,
        artifacts: row.artifacts,
    })
}

/// Update task run progress
pub async fn update_task_run_progress(
    pool: &PgPool,
    run_id: Uuid,
    current_phase: Option<&str>,
    progress_percent: Option<i32>,
) -> DbResult<Option<TaskRunRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE task_runs
        SET current_phase = COALESCE($2, current_phase),
            progress_percent = COALESCE($3, progress_percent)
        WHERE id = $1
        RETURNING id, task_id, status, current_phase, progress_percent, started_at,
                  completed_at, error_message, artifacts
        "#,
        run_id,
        current_phase,
        progress_percent
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| TaskRunRow {
        id: r.id,
        task_id: r.task_id,
        status: r.status,
        current_phase: r.current_phase,
        progress_percent: r.progress_percent,
        started_at: r.started_at,
        completed_at: r.completed_at,
        error_message: r.error_message,
        artifacts: r.artifacts,
    }))
}

/// Complete a task run
pub async fn complete_task_run(
    pool: &PgPool,
    run_id: Uuid,
    status: &str,
    error_message: Option<&str>,
    artifacts: Option<serde_json::Value>,
) -> DbResult<Option<TaskRunRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE task_runs
        SET status = $2,
            completed_at = NOW(),
            error_message = $3,
            artifacts = COALESCE($4, artifacts),
            progress_percent = 100
        WHERE id = $1
        RETURNING id, task_id, status, current_phase, progress_percent, started_at,
                  completed_at, error_message, artifacts
        "#,
        run_id,
        status,
        error_message,
        artifacts
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| TaskRunRow {
        id: r.id,
        task_id: r.task_id,
        status: r.status,
        current_phase: r.current_phase,
        progress_percent: r.progress_percent,
        started_at: r.started_at,
        completed_at: r.completed_at,
        error_message: r.error_message,
        artifacts: r.artifacts,
    }))
}

/// List task runs for a task
pub async fn list_task_runs(pool: &PgPool, task_id: Uuid) -> DbResult<Vec<TaskRunRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, task_id, status, current_phase, progress_percent, started_at,
               completed_at, error_message, artifacts
        FROM task_runs
        WHERE task_id = $1
        ORDER BY started_at DESC
        "#,
        task_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| TaskRunRow {
            id: r.id,
            task_id: r.task_id,
            status: r.status,
            current_phase: r.current_phase,
            progress_percent: r.progress_percent,
            started_at: r.started_at,
            completed_at: r.completed_at,
            error_message: r.error_message,
            artifacts: r.artifacts,
        })
        .collect())
}

/// Get task run by ID
pub async fn get_task_run(pool: &PgPool, run_id: Uuid) -> DbResult<Option<TaskRunRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, task_id, status, current_phase, progress_percent, started_at,
               completed_at, error_message, artifacts
        FROM task_runs
        WHERE id = $1
        "#,
        run_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| TaskRunRow {
        id: r.id,
        task_id: r.task_id,
        status: r.status,
        current_phase: r.current_phase,
        progress_percent: r.progress_percent,
        started_at: r.started_at,
        completed_at: r.completed_at,
        error_message: r.error_message,
        artifacts: r.artifacts,
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
pub async fn get_task_run_logs(pool: &PgPool, task_run_id: Uuid) -> DbResult<Vec<TaskRunLogRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, task_run_id, phase, agent_type, log_level, message, metadata, created_at
        FROM task_run_logs
        WHERE task_run_id = $1
        ORDER BY created_at ASC
        "#,
        task_run_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| TaskRunLogRow {
            id: r.id,
            task_run_id: r.task_run_id,
            phase: r.phase,
            agent_type: r.agent_type,
            log_level: r.log_level,
            message: r.message,
            metadata: r.metadata,
            created_at: r.created_at,
        })
        .collect())
}

/// Update task PR information
pub async fn update_task_pr(
    pool: &PgPool,
    task_id: Uuid,
    pr_url: &str,
    branch_name: &str,
    pr_status: &str,
) -> DbResult<Option<TaskRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE tasks
        SET pr_url = $2,
            branch_name = $3,
            pr_status = $4,
            pr_created_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, project_id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at
        "#,
        task_id,
        pr_url,
        branch_name,
        pr_status
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| map_task_row!(r)))
}

/// Update task branch name (for when branch is created before PR)
pub async fn update_task_branch(
    pool: &PgPool,
    task_id: Uuid,
    branch_name: &str,
) -> DbResult<Option<TaskRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE tasks
        SET branch_name = $2,
            pr_status = 'pending',
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, project_id, title, description, acceptance_criteria, status, priority,
                  model_name, dependencies, is_agentic, github_repo_url, source_id, source_ids,
                  workspace_id, worker_id, queued_at, started_at, completed_at, created_at, updated_at,
                  pr_url, branch_name, pr_status, pr_created_at
        "#,
        task_id,
        branch_name
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| map_task_row!(r)))
}
