//! Workspace host-directory list.

use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

pub async fn list(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<String>> {
    let directories: Option<Vec<String>> = sqlx::query_scalar(
        r#"
        SELECT directories
        FROM workspace_host_directories
        WHERE workspace_id = $1
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(directories.unwrap_or_default())
}

pub async fn upsert(
    pool: &PgPool,
    workspace_id: Uuid,
    directories: &[String],
) -> DbResult<Vec<String>> {
    let directories: Vec<String> = sqlx::query_scalar(
        r#"
        INSERT INTO workspace_host_directories (workspace_id, directories)
        VALUES ($1, $2)
        ON CONFLICT (workspace_id) DO UPDATE SET
            directories = EXCLUDED.directories,
            updated_at = NOW()
        RETURNING directories
        "#,
    )
    .bind(workspace_id)
    .bind(directories)
    .fetch_one(pool)
    .await?;

    Ok(directories)
}
