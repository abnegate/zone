//! Workspace theme database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Workspace theme row from database
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

/// Get theme for a workspace
pub async fn get_theme(pool: &PgPool, workspace_id: Uuid) -> DbResult<Option<WorkspaceThemeRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, workspace_id, primary_color_light, secondary_color_light,
               primary_color_dark, secondary_color_dark, font_family, font_size_base,
               border_radius, created_at, updated_at
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

/// Upsert (create or update) theme for a workspace
#[allow(clippy::too_many_arguments)]
pub async fn upsert_theme(
    pool: &PgPool,
    workspace_id: Uuid,
    primary_color_light: Option<&str>,
    secondary_color_light: Option<&str>,
    primary_color_dark: Option<&str>,
    secondary_color_dark: Option<&str>,
    font_family: Option<&str>,
    font_size_base: Option<&str>,
    border_radius: Option<&str>,
) -> DbResult<WorkspaceThemeRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO workspace_themes (
            workspace_id, primary_color_light, secondary_color_light,
            primary_color_dark, secondary_color_dark, font_family, font_size_base,
            border_radius
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (workspace_id) DO UPDATE SET
            primary_color_light = COALESCE($2, workspace_themes.primary_color_light),
            secondary_color_light = COALESCE($3, workspace_themes.secondary_color_light),
            primary_color_dark = COALESCE($4, workspace_themes.primary_color_dark),
            secondary_color_dark = COALESCE($5, workspace_themes.secondary_color_dark),
            font_family = COALESCE($6, workspace_themes.font_family),
            font_size_base = COALESCE($7, workspace_themes.font_size_base),
            border_radius = COALESCE($8, workspace_themes.border_radius),
            updated_at = NOW()
        RETURNING id, workspace_id, primary_color_light, secondary_color_light,
                  primary_color_dark, secondary_color_dark, font_family, font_size_base,
                  border_radius, created_at, updated_at
        "#,
        workspace_id,
        primary_color_light,
        secondary_color_light,
        primary_color_dark,
        secondary_color_dark,
        font_family,
        font_size_base,
        border_radius
    )
    .fetch_one(pool)
    .await?;

    Ok(WorkspaceThemeRow {
        id: row.id,
        workspace_id: row.workspace_id,
        primary_color_light: row.primary_color_light,
        secondary_color_light: row.secondary_color_light,
        primary_color_dark: row.primary_color_dark,
        secondary_color_dark: row.secondary_color_dark,
        font_family: row.font_family,
        font_size_base: row.font_size_base,
        border_radius: row.border_radius,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Delete theme for a workspace
pub async fn delete_theme(pool: &PgPool, workspace_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM workspace_themes WHERE workspace_id = $1",
        workspace_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
