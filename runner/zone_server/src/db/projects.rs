//! Project database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

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
    pub github_installation_id: Option<Uuid>,
}

/// List projects with workspace and optional status filter
pub async fn list_projects(
    pool: &PgPool,
    workspace_id: Uuid,
    status: Option<&str>,
) -> DbResult<Vec<ProjectRow>> {
    if let Some(status) = status {
        sqlx::query_as!(
            ProjectRow,
            r#"
            SELECT id, workspace_id, source_id, name, description, status,
                   github_repo_url, github_access_token, created_at, updated_at,
                   github_installation_id
            FROM projects
            WHERE workspace_id = $1 AND status = $2
            ORDER BY created_at DESC
            "#,
            workspace_id,
            status
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as!(
            ProjectRow,
            r#"
            SELECT id, workspace_id, source_id, name, description, status,
                   github_repo_url, github_access_token, created_at, updated_at,
                   github_installation_id
            FROM projects
            WHERE workspace_id = $1
            ORDER BY created_at DESC
            "#,
            workspace_id
        )
        .fetch_all(pool)
        .await
    }
}

/// Get project by ID
pub async fn get_project(pool: &PgPool, id: Uuid) -> DbResult<Option<ProjectRow>> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, workspace_id, source_id, name, description, status,
               github_repo_url, github_access_token, created_at, updated_at,
               github_installation_id
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
    sqlx::query_as::<_, ProjectRow>(
        r#"
        SELECT id, workspace_id, source_id, name, description, status,
               github_repo_url, github_access_token, created_at, updated_at,
               github_installation_id
        FROM projects
        WHERE id = $1 AND workspace_id = $2
        "#,
    )
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

/// Create a new project
pub async fn create(pool: &PgPool, project: NewProject<'_>) -> DbResult<ProjectRow> {
    sqlx::query_as::<_, ProjectRow>(
        r#"
        INSERT INTO projects (name, description, workspace_id, source_id, status)
        VALUES ($1, $2, $3, $4, COALESCE($5, 'active'))
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id
        "#,
    )
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
    sqlx::query_as::<_, ProjectRow>(
        r#"
        UPDATE projects
        SET source_id = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, source_id, name, description, status,
                  github_repo_url, github_access_token, created_at, updated_at,
                  github_installation_id
        "#,
    )
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
                  github_installation_id
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
                  github_installation_id
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
                  github_installation_id
        "#,
        id
    )
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sources::testing;

    async fn widgets(pool: &PgPool, workspace_id: Uuid) -> ProjectRow {
        create(
            pool,
            NewProject {
                name: "Widgets",
                description: None,
                workspace_id: Some(workspace_id),
                source_id: None,
                status: None,
            },
        )
        .await
        .expect("a project")
    }

    #[tokio::test]
    async fn linking_with_an_installation_stores_it_and_unlinking_clears_it() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let project = widgets(&pool, installation.workspace).await;
        assert_eq!(project.github_installation_id, None);

        let linked = link_github(
            &pool,
            project.id,
            "https://github.com/acme/widgets",
            None,
            Some(installation.id),
        )
        .await
        .expect("the link")
        .expect("the project");
        assert_eq!(linked.github_installation_id, Some(installation.id));
        assert_eq!(linked.github_access_token, None);

        let unlinked = unlink_github(&pool, project.id)
            .await
            .expect("the unlink")
            .expect("the project");
        assert_eq!(unlinked.github_installation_id, None);
        assert_eq!(unlinked.github_repo_url, None);
    }

    #[tokio::test]
    async fn every_project_read_and_write_returns_the_installation() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let project = widgets(&pool, installation.workspace).await;
        link_github(
            &pool,
            project.id,
            "https://github.com/acme/widgets",
            None,
            Some(installation.id),
        )
        .await
        .expect("the link");
        let expected = Some(installation.id);

        let in_workspace = get_project_in_workspace(&pool, project.id, installation.workspace)
            .await
            .expect("the read")
            .expect("the project");
        assert_eq!(in_workspace.github_installation_id, expected);
        let by_id = get_project(&pool, project.id)
            .await
            .expect("the read")
            .expect("the project");
        assert_eq!(by_id.github_installation_id, expected);
        for status in [None, Some("active")] {
            let listed = list_projects(&pool, installation.workspace, status)
                .await
                .expect("the listing");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].github_installation_id, expected);
        }
        let renamed = update_project(&pool, project.id, Some("Gadgets"), None, None)
            .await
            .expect("the update")
            .expect("the project");
        assert_eq!(renamed.github_installation_id, expected);
        let unsourced = set_source(&pool, project.id, None)
            .await
            .expect("the update")
            .expect("the project");
        assert_eq!(unsourced.github_installation_id, expected);
    }
}
