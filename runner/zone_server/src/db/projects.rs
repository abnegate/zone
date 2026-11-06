//! Project database queries

mod order;

use std::collections::HashMap;
use std::fmt;

use abnegate_secret::REDACTED;
use chrono::NaiveDateTime;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::DbResult;

pub use order::Reorder;

/// Project row from database
#[derive(Clone, sqlx::FromRow)]
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
    pub github_installation_id: Option<Uuid>,
    pub position: i32,
}

impl fmt::Debug for ProjectRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProjectRow")
            .field("id", &self.id)
            .field("workspace_id", &self.workspace_id)
            .field("source_id", &self.source_id)
            .field("name", &self.name)
            .field("description", &self.description)
            .field("status", &self.status)
            .field("github_repo_url", &self.github_repo_url)
            .field(
                "github_access_token",
                &self.github_access_token.as_ref().map(|_| REDACTED),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("github_installation_id", &self.github_installation_id)
            .field("position", &self.position)
            .finish()
    }
}

/// A workspace's projects in the workspace's own order, optionally of one status
pub async fn list_projects(
    pool: &PgPool,
    workspace_id: Uuid,
    status: Option<&str>,
) -> DbResult<Vec<ProjectRow>> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, workspace_id, source_id, name, description, status,
               github_repo_url, github_access_token, created_at, updated_at,
               github_installation_id, "position"
        FROM projects
        WHERE workspace_id = $1 AND ($2::text IS NULL OR status = $2)
        ORDER BY "position", created_at DESC, id
        "#,
        workspace_id,
        status
    )
    .fetch_all(pool)
    .await
}

/// When a member last worked in each of a workspace's projects: the latest
/// message one typed into a project chat, or the latest task one added.
///
/// Reminder, tool and automation posts carry a `source`, and only a task whose
/// origin is a member counts. A project nobody worked in is left out.
pub async fn last_used(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<HashMap<Uuid, NaiveDateTime>> {
    let rows = sqlx::query!(
        r#"
        SELECT project.id, GREATEST(typed.used_at, added.used_at) AS used_at
        FROM projects project
        LEFT JOIN LATERAL (
            SELECT MAX(latest.created_at) AS used_at
            FROM chats chat
            CROSS JOIN LATERAL (
                SELECT message.created_at
                FROM messages message
                WHERE message.chat_id = chat.id
                  AND message.role = 'user'
                  AND NOT COALESCE(
                      jsonb_typeof(message.metadata) = 'object' AND message.metadata ? 'source',
                      FALSE
                  )
                ORDER BY message.created_at DESC
                LIMIT 1
            ) latest
            WHERE chat.project_id = project.id
        ) typed ON TRUE
        LEFT JOIN LATERAL (
            SELECT MAX(task.created_at) AS used_at
            FROM task_projects link
            JOIN tasks task ON task.id = link.task_id
            WHERE link.project_id = project.id
              AND task.origin = 'member'
        ) added ON TRUE
        WHERE project.workspace_id = $1
        "#,
        workspace_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.used_at.map(|used_at| (row.id, used_at)))
        .collect())
}

/// Get project by ID
pub async fn get_project(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, workspace_id, source_id, name, description, status,
               github_repo_url, github_access_token, created_at, updated_at,
               github_installation_id, "position"
        FROM projects
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await
}

/// Get a project only when it belongs to the specified workspace.
pub async fn get_project_in_workspace(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, workspace_id, source_id, name, description, status,
               github_repo_url, github_access_token, created_at, updated_at,
               github_installation_id, "position"
        FROM projects
        WHERE id = $1 AND workspace_id = $2
        "#,
        id,
        workspace_id
    )
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

/// Hold a workspace's project order until the transaction ends, so a create
/// or reorder reads positions no other one is about to change.
pub(crate) async fn lock_order(connection: &mut PgConnection, workspace_id: Uuid) -> DbResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('project-order:' || $1::text, 0))")
        .bind(workspace_id)
        .execute(connection)
        .await?;
    Ok(())
}

/// Create a new project, first in its workspace's order
pub async fn create(pool: &PgPool, project: NewProject<'_>) -> DbResult<ProjectRow> {
    let mut transaction = pool.begin().await?;
    if let Some(workspace_id) = project.workspace_id {
        lock_order(&mut transaction, workspace_id).await?;
    }
    let row = sqlx::query_as!(
        ProjectRow,
        r#"
        INSERT INTO projects (name, description, workspace_id, source_id, status, "position")
        VALUES ($1, $2, $3, $4, COALESCE($5, 'active'),
                COALESCE((SELECT MIN("position") FROM projects WHERE workspace_id = $3), 1) - 1)
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id, "position"
        "#,
        project.name,
        project.description,
        project.workspace_id,
        project.source_id,
        project.status
    )
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(row)
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
    sqlx::query_as!(
        ProjectRow,
        r#"
        UPDATE projects
        SET source_id = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id, "position"
        "#,
        id,
        source_id
    )
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
    sqlx::query_as!(
        ProjectRow,
        r#"
        UPDATE projects
        SET name = COALESCE($2, name),
            description = COALESCE($3, description),
            status = COALESCE($4, status),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id, "position"
        "#,
        id,
        name,
        description,
        status
    )
    .fetch_optional(pool)
    .await
}

/// Delete a project
pub async fn delete_project(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM projects WHERE id = $1", id)
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
    github_installation_id: Option<Uuid>,
) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        UPDATE projects
        SET github_repo_url = $2,
            github_access_token = $3,
            github_installation_id = $4,
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id, "position"
        "#,
        id,
        repo_url,
        access_token,
        github_installation_id
    )
    .fetch_optional(pool)
    .await
}

/// Unlink GitHub repository from a project
pub async fn unlink_github(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        UPDATE projects
        SET github_repo_url = NULL,
            github_access_token = NULL,
            github_installation_id = NULL,
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id, "position"
        "#,
        id
    )
    .fetch_optional(pool)
    .await
}

/// Put the given projects of a workspace in the given order, in one transaction.
///
/// The projects named take, in the order given, the places they held between
/// them; every other project keeps its place, so a list filtered by status or
/// search can be reordered without moving what it hides. A project no longer
/// in the workspace, such as one deleted since the list was drawn, is skipped.
pub async fn reorder(pool: &PgPool, workspace_id: Uuid, ids: &[Uuid]) -> DbResult<Reorder> {
    let mut transaction = pool.begin().await?;
    lock_order(&mut transaction, workspace_id).await?;
    let current = sqlx::query_scalar!(
        r#"
        SELECT id FROM projects
        WHERE workspace_id = $1
        ORDER BY "position", created_at DESC, id
        "#,
        workspace_id
    )
    .fetch_all(&mut *transaction)
    .await?;

    let Some(arranged) = order::arrange(&current, ids) else {
        return Ok(Reorder::Duplicated);
    };
    let positions: Vec<i32> = (1..).take(arranged.len()).collect();
    sqlx::query!(
        r#"
        UPDATE projects
        SET "position" = placed."position"
        FROM UNNEST($1::uuid[], $2::integer[]) AS placed(id, "position")
        WHERE projects.id = placed.id
        "#,
        &arranged,
        &positions
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Reorder::Reordered)
}

#[cfg(test)]
mod tests;
