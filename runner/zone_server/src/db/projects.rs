//! Project database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Project row from database
#[derive(Debug, Clone)]
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
}

/// List projects with optional status filter
pub async fn list_projects(pool: &PgPool, status: Option<&str>) -> DbResult<Vec<ProjectRow>> {
    if let Some(status) = status {
        let rows = sqlx::query!(
            r#"
            SELECT id, workspace_id, source_id, name, description, status,
                   github_repo_url, github_access_token, created_at, updated_at
            FROM projects
            WHERE status = $1
            ORDER BY created_at DESC
            "#,
            status
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ProjectRow {
                id: r.id,
                workspace_id: r.workspace_id,
                source_id: r.source_id,
                name: r.name,
                description: r.description,
                status: r.status,
                github_repo_url: r.github_repo_url,
                github_access_token: r.github_access_token,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    } else {
        let rows = sqlx::query!(
            r#"
            SELECT id, workspace_id, source_id, name, description, status,
                   github_repo_url, github_access_token, created_at, updated_at
            FROM projects
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ProjectRow {
                id: r.id,
                workspace_id: r.workspace_id,
                source_id: r.source_id,
                name: r.name,
                description: r.description,
                status: r.status,
                github_repo_url: r.github_repo_url,
                github_access_token: r.github_access_token,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

/// Get project by ID
pub async fn get_project(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, workspace_id, source_id, name, description, status,
               github_repo_url, github_access_token, created_at, updated_at
        FROM projects
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ProjectRow {
        id: r.id,
        workspace_id: r.workspace_id,
        source_id: r.source_id,
        name: r.name,
        description: r.description,
        status: r.status,
        github_repo_url: r.github_repo_url,
        github_access_token: r.github_access_token,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new project
pub async fn create_project(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
    workspace_id: Option<Uuid>,
) -> DbResult<ProjectRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO projects (name, description, workspace_id, status)
        VALUES ($1, $2, $3, 'active')
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at
        "#,
        name,
        description,
        workspace_id
    )
    .fetch_one(pool)
    .await?;

    Ok(ProjectRow {
        id: row.id,
        workspace_id: row.workspace_id,
        source_id: row.source_id,
        name: row.name,
        description: row.description,
        status: row.status,
        github_repo_url: row.github_repo_url,
        github_access_token: row.github_access_token,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update a project
pub async fn update_project(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    status: Option<&str>,
) -> DbResult<Option<ProjectRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE projects
        SET name = COALESCE($2, name),
            description = COALESCE($3, description),
            status = COALESCE($4, status),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at
        "#,
        id,
        name,
        description,
        status
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ProjectRow {
        id: r.id,
        workspace_id: r.workspace_id,
        source_id: r.source_id,
        name: r.name,
        description: r.description,
        status: r.status,
        github_repo_url: r.github_repo_url,
        github_access_token: r.github_access_token,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
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
) -> DbResult<Option<ProjectRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE projects
        SET github_repo_url = $2,
            github_access_token = $3,
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at
        "#,
        id,
        repo_url,
        access_token
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ProjectRow {
        id: r.id,
        workspace_id: r.workspace_id,
        source_id: r.source_id,
        name: r.name,
        description: r.description,
        status: r.status,
        github_repo_url: r.github_repo_url,
        github_access_token: r.github_access_token,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Unlink GitHub repository from a project
pub async fn unlink_github(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE projects
        SET github_repo_url = NULL,
            github_access_token = NULL,
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ProjectRow {
        id: r.id,
        workspace_id: r.workspace_id,
        source_id: r.source_id,
        name: r.name,
        description: r.description,
        status: r.status,
        github_repo_url: r.github_repo_url,
        github_access_token: r.github_access_token,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}
