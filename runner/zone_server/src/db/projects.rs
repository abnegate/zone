//! Project database queries

mod order;

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

pub use order::Reorder;

/// The columns every query returns, ending with when a member last worked in
/// the project: the latest message one typed into its chats, or the latest task
/// one added to it. Messages posted by reminders, tools or automation carry a
/// `source`, and tasks automation planned are marked `auto_created`, so neither
/// counts.
macro_rules! columns {
    () => {
        r#"id, workspace_id, source_id, name, description, status,
           github_repo_url, github_access_token, created_at, updated_at, "position",
           GREATEST(
               (SELECT MAX(message.created_at)
                FROM chats chat
                JOIN messages message ON message.chat_id = chat.id
                WHERE chat.project_id = projects.id
                  AND message.role = 'user'
                  AND NOT COALESCE(message.metadata ? 'source', FALSE)),
               (SELECT MAX(task.created_at)
                FROM task_projects link
                JOIN tasks task ON task.id = link.task_id
                WHERE link.project_id = projects.id
                  AND task.created_by IS NOT NULL
                  AND NOT EXISTS (
                      SELECT 1 FROM task_automation automation
                      WHERE automation.task_id = task.id AND automation.auto_created
                  ))
           ) AS last_used_at"#
    };
}

/// Project row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectRow {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub github_repo_url: Option<String>,
    pub github_access_token: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    /// Where the workspace placed the project in its own order, lowest first.
    pub position: i32,
    pub last_used_at: Option<NaiveDateTime>,
}

/// A workspace's projects in the workspace's own order, optionally of one status
pub async fn list_projects(
    pool: &PgPool,
    workspace_id: Uuid,
    status: Option<&str>,
) -> DbResult<Vec<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        "SELECT ",
        columns!(),
        r#" FROM projects
            WHERE workspace_id = $1 AND ($2::text IS NULL OR status = $2)
            ORDER BY "position", created_at DESC, id"#
    ))
    .bind(workspace_id)
    .bind(status)
    .fetch_all(pool)
    .await
}

/// Get project by ID
pub async fn get_project(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        "SELECT ",
        columns!(),
        " FROM projects WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Get a project only when it belongs to the specified workspace.
pub async fn get_project_in_workspace(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        "SELECT ",
        columns!(),
        " FROM projects WHERE id = $1 AND workspace_id = $2"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

/// Everything a project is created with; `status` defaults to active.
#[derive(Debug, Clone, Copy)]
pub struct NewProject<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub workspace_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub status: Option<&'a str>,
}

/// Create a new project, first in its workspace's order
pub async fn create(pool: &PgPool, project: NewProject<'_>) -> DbResult<ProjectRow> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        r#"INSERT INTO projects (name, description, workspace_id, source_id, status, "position")
           VALUES ($1, $2, $3, $4, COALESCE($5, 'active'),
                   COALESCE((SELECT MIN("position") FROM projects WHERE workspace_id = $3), 1) - 1)
           RETURNING "#,
        columns!()
    ))
    .bind(project.name)
    .bind(project.description)
    .bind(project.workspace_id)
    .bind(project.source_id)
    .bind(project.status)
    .fetch_one(pool)
    .await
}

/// Create a new active project with no source
pub async fn create_project(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
    workspace_id: Option<Uuid>,
) -> DbResult<ProjectRow> {
    create(
        pool,
        NewProject {
            name,
            description,
            workspace_id,
            source_id: None,
            status: None,
        },
    )
    .await
}

/// Point a project at one of its workspace's sources, or at none
pub async fn set_source(
    pool: &PgPool,
    id: Uuid,
    source_id: Option<Uuid>,
) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        "UPDATE projects SET source_id = $2, updated_at = NOW() WHERE id = $1 RETURNING ",
        columns!()
    ))
    .bind(id)
    .bind(source_id)
    .fetch_optional(pool)
    .await
}

/// Update a project
pub async fn update_project(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    status: Option<&str>,
) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        r#"UPDATE projects
           SET name = COALESCE($2, name),
               description = COALESCE($3, description),
               status = COALESCE($4, status),
               updated_at = NOW()
           WHERE id = $1
           RETURNING "#,
        columns!()
    ))
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(status)
    .fetch_optional(pool)
    .await
}

/// Delete a project
pub async fn delete_project(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Link a GitHub repository to a project
pub async fn link_github(
    pool: &PgPool,
    id: Uuid,
    repo_url: &str,
    access_token: Option<&str>,
) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        r#"UPDATE projects
           SET github_repo_url = $2, github_access_token = $3, updated_at = NOW()
           WHERE id = $1
           RETURNING "#,
        columns!()
    ))
    .bind(id)
    .bind(repo_url)
    .bind(access_token)
    .fetch_optional(pool)
    .await
}

/// Unlink GitHub repository from a project
pub async fn unlink_github(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(concat!(
        r#"UPDATE projects
           SET github_repo_url = NULL, github_access_token = NULL, updated_at = NOW()
           WHERE id = $1
           RETURNING "#,
        columns!()
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Put the given projects of a workspace in the given order, in one transaction.
///
/// The projects named take, in the order given, the places they held between
/// them; every other project keeps its place. So a list filtered by status or
/// search can be reordered without moving what it hides.
pub async fn reorder(pool: &PgPool, workspace_id: Uuid, ids: &[Uuid]) -> DbResult<Reorder> {
    let mut transaction = pool.begin().await?;
    let current: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM projects
           WHERE workspace_id = $1
           ORDER BY "position", created_at DESC, id
           FOR UPDATE"#,
    )
    .bind(workspace_id)
    .fetch_all(&mut *transaction)
    .await?;

    let Some(arranged) = order::arrange(&current, ids) else {
        return Ok(Reorder::Unknown);
    };
    let positions: Vec<i32> = (1..).take(arranged.len()).collect();
    sqlx::query(
        r#"UPDATE projects
           SET "position" = placed."position"
           FROM UNNEST($1::uuid[], $2::integer[]) AS placed(id, "position")
           WHERE projects.id = placed.id"#,
    )
    .bind(&arranged)
    .bind(&positions)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Reorder::Reordered)
}

#[cfg(test)]
mod tests;
