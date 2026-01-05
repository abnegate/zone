//! Workspace database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Workspace row from database
#[derive(Debug, Clone)]
pub struct WorkspaceRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// List workspaces for an organization
pub async fn list_workspaces(pool: &PgPool, organization_id: Uuid) -> DbResult<Vec<WorkspaceRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
        FROM workspaces
        WHERE organization_id = $1
        ORDER BY created_at DESC
        "#,
        organization_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| WorkspaceRow {
            id: r.id,
            organization_id: r.organization_id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Get workspace by ID
pub async fn get_workspace(pool: &PgPool, id: Uuid) -> DbResult<Option<WorkspaceRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
        FROM workspaces
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| WorkspaceRow {
        id: r.id,
        organization_id: r.organization_id,
        name: r.name,
        slug: r.slug,
        description: r.description,
        is_active: r.is_active,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new workspace
pub async fn create_workspace(
    pool: &PgPool,
    organization_id: Uuid,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> DbResult<WorkspaceRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO workspaces (organization_id, name, slug, description)
        VALUES ($1, $2, $3, $4)
        RETURNING id, organization_id, name, slug, description, is_active, created_at, updated_at
        "#,
        organization_id,
        name,
        slug,
        description
    )
    .fetch_one(pool)
    .await?;

    Ok(WorkspaceRow {
        id: row.id,
        organization_id: row.organization_id,
        name: row.name,
        slug: row.slug,
        description: row.description,
        is_active: row.is_active,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update a workspace
pub async fn update_workspace(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    is_active: Option<bool>,
) -> DbResult<Option<WorkspaceRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE workspaces
        SET name = COALESCE($2, name),
            description = COALESCE($3, description),
            is_active = COALESCE($4, is_active),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, organization_id, name, slug, description, is_active, created_at, updated_at
        "#,
        id,
        name,
        description,
        is_active
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| WorkspaceRow {
        id: r.id,
        organization_id: r.organization_id,
        name: r.name,
        slug: r.slug,
        description: r.description,
        is_active: r.is_active,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Delete a workspace
pub async fn delete_workspace(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM workspaces WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Workspace theme row
#[derive(Debug, Clone)]
pub struct WorkspaceThemeRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub primary_color_light: Option<String>,
    pub secondary_color_light: Option<String>,
    pub primary_color_dark: Option<String>,
    pub secondary_color_dark: Option<String>,
    pub font_family: Option<String>,
    pub font_size_base: Option<String>,
    pub border_radius: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Get workspace theme
pub async fn get_workspace_theme(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<Option<WorkspaceThemeRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, workspace_id, primary_color_light, secondary_color_light,
               primary_color_dark, secondary_color_dark, font_family,
               font_size_base, border_radius, created_at, updated_at
        FROM workspace_themes
        WHERE workspace_id = $1
        "#,
        workspace_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| WorkspaceThemeRow {
        id: r.id,
        workspace_id: r.workspace_id,
        primary_color_light: r.primary_color_light,
        secondary_color_light: r.secondary_color_light,
        primary_color_dark: r.primary_color_dark,
        secondary_color_dark: r.secondary_color_dark,
        font_family: r.font_family,
        font_size_base: r.font_size_base,
        border_radius: r.border_radius,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Delete workspace theme
pub async fn delete_workspace_theme(pool: &PgPool, workspace_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM workspace_themes WHERE workspace_id = $1",
        workspace_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
