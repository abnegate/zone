//! Workspace theme database queries

use chrono::NaiveDateTime;
use sqlx::{Executor, PgConnection, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;

#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

type AccessResult<T> = Result<T, AccessError>;

pub struct Update<'a> {
    pub primary_color_light: Option<&'a str>,
    pub secondary_color_light: Option<&'a str>,
    pub primary_color_dark: Option<&'a str>,
    pub secondary_color_dark: Option<&'a str>,
    pub font_family: Option<&'a str>,
    pub font_size_base: Option<&'a str>,
    pub border_radius: Option<&'a str>,
}

async fn authorize(
    connection: &mut PgConnection,
    workspace_id: Uuid,
    user_id: Uuid,
    write: bool,
) -> AccessResult<()> {
    // Hold the membership row through the protected query so revocation and
    // the theme mutation have a definite transaction order.
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND is_active FOR SHARE",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(connection)
    .await?;

    match role.as_deref() {
        Some("owner" | "admin" | "member") => Ok(()),
        Some("viewer") if !write => Ok(()),
        _ if write => Err(AccessError::Forbidden(
            "You do not have write access to this workspace",
        )),
        _ => Err(AccessError::NotFound("Workspace not found")),
    }
}

/// Workspace theme row from database
#[derive(Debug, Clone, sqlx::FromRow)]
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

async fn get<'e, E>(executor: E, workspace_id: Uuid) -> DbResult<Option<WorkspaceThemeRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<WorkspaceThemeRow> = sqlx::query_as(
        r#"
        SELECT id, workspace_id, primary_color_light, secondary_color_light,
               primary_color_dark, secondary_color_dark, font_family, font_size_base,
               border_radius, created_at, updated_at
        FROM workspace_themes
        WHERE workspace_id = $1
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?;

    Ok(row)
}

/// Read a theme while holding the caller's membership row.
pub async fn get_authorized(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> AccessResult<Option<WorkspaceThemeRow>> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, false).await?;
    let theme = get(&mut *transaction, workspace_id).await?;
    transaction.commit().await?;
    Ok(theme)
}

async fn upsert<'e, E>(
    executor: E,
    workspace_id: Uuid,
    update: &Update<'_>,
) -> DbResult<WorkspaceThemeRow>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: WorkspaceThemeRow = sqlx::query_as(
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
    )
    .bind(workspace_id)
    .bind(update.primary_color_light)
    .bind(update.secondary_color_light)
    .bind(update.primary_color_dark)
    .bind(update.secondary_color_dark)
    .bind(update.font_family)
    .bind(update.font_size_base)
    .bind(update.border_radius)
    .fetch_one(executor)
    .await?;

    Ok(row)
}

/// Upsert a theme while holding the caller's writer membership row.
pub async fn upsert_authorized(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    update: Update<'_>,
) -> AccessResult<WorkspaceThemeRow> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    let theme = upsert(&mut *transaction, workspace_id, &update).await?;
    transaction.commit().await?;
    Ok(theme)
}

async fn delete<'e, E>(executor: E, workspace_id: Uuid) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query("DELETE FROM workspace_themes WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(executor)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete a theme while holding the caller's writer membership row.
pub async fn delete_authorized(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> AccessResult<bool> {
    let mut transaction = pool.begin().await?;
    authorize(&mut transaction, workspace_id, user_id, true).await?;
    let deleted = delete(&mut *transaction, workspace_id).await?;
    transaction.commit().await?;
    Ok(deleted)
}
