//! Source database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Source row from database
#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub config: serde_json::Value,
    pub credentials_encrypted: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub is_active: Option<bool>,
    pub last_verified_at: Option<NaiveDateTime>,
    pub last_error: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Helper macro to map a row to SourceRow
macro_rules! map_source_row {
    ($r:expr) => {
        SourceRow {
            id: $r.id,
            name: $r.name,
            source_type: $r.source_type,
            config: $r.config,
            credentials_encrypted: $r.credentials_encrypted,
            description: $r.description,
            url: $r.url,
            is_active: $r.is_active,
            last_verified_at: $r.last_verified_at,
            last_error: $r.last_error,
            created_at: $r.created_at,
            updated_at: $r.updated_at,
        }
    };
}

/// List sources with optional filters
pub async fn list_sources(
    pool: &PgPool,
    source_type: Option<&str>,
    is_active: Option<bool>,
) -> DbResult<Vec<SourceRow>> {
    match (source_type, is_active) {
        (Some(t), Some(a)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at
                FROM sources
                WHERE source_type = $1 AND is_active = $2
                ORDER BY created_at DESC
                "#,
                t,
                a
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_source_row!(r)).collect())
        }
        (Some(t), None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at
                FROM sources
                WHERE source_type = $1
                ORDER BY created_at DESC
                "#,
                t
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_source_row!(r)).collect())
        }
        (None, Some(a)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at
                FROM sources
                WHERE is_active = $1
                ORDER BY created_at DESC
                "#,
                a
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_source_row!(r)).collect())
        }
        (None, None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at
                FROM sources
                ORDER BY created_at DESC
                "#
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_source_row!(r)).collect())
        }
    }
}

/// Get source by ID
pub async fn get_source(pool: &PgPool, id: Uuid) -> DbResult<Option<SourceRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at
        FROM sources
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SourceRow {
        id: r.id,
        name: r.name,
        source_type: r.source_type,
        config: r.config,
        credentials_encrypted: r.credentials_encrypted,
        description: r.description,
        url: r.url,
        is_active: r.is_active,
        last_verified_at: r.last_verified_at,
        last_error: r.last_error,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new source
pub async fn create_source(
    pool: &PgPool,
    name: &str,
    source_type: &str,
    config: serde_json::Value,
    description: Option<&str>,
    url: Option<&str>,
    credentials_encrypted: Option<&str>,
) -> DbResult<SourceRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO sources (name, source_type, config, description, url, credentials_encrypted)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at
        "#,
        name,
        source_type,
        config,
        description,
        url,
        credentials_encrypted
    )
    .fetch_one(pool)
    .await?;

    Ok(SourceRow {
        id: row.id,
        name: row.name,
        source_type: row.source_type,
        config: row.config,
        credentials_encrypted: row.credentials_encrypted,
        description: row.description,
        url: row.url,
        is_active: row.is_active,
        last_verified_at: row.last_verified_at,
        last_error: row.last_error,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update a source
pub async fn update_source(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    config: Option<serde_json::Value>,
    description: Option<&str>,
    url: Option<&str>,
    is_active: Option<bool>,
) -> DbResult<Option<SourceRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE sources
        SET name = COALESCE($2, name),
            config = COALESCE($3, config),
            description = COALESCE($4, description),
            url = COALESCE($5, url),
            is_active = COALESCE($6, is_active),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at
        "#,
        id,
        name,
        config,
        description,
        url,
        is_active
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SourceRow {
        id: r.id,
        name: r.name,
        source_type: r.source_type,
        config: r.config,
        credentials_encrypted: r.credentials_encrypted,
        description: r.description,
        url: r.url,
        is_active: r.is_active,
        last_verified_at: r.last_verified_at,
        last_error: r.last_error,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Delete a source
pub async fn delete_source(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM sources WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Update verification status
pub async fn update_verification(
    pool: &PgPool,
    id: Uuid,
    last_error: Option<&str>,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE sources
        SET last_verified_at = NOW(),
            last_error = $2,
            updated_at = NOW()
        WHERE id = $1
        "#,
        id,
        last_error
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Update credentials
pub async fn update_credentials(
    pool: &PgPool,
    id: Uuid,
    credentials_encrypted: &str,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE sources
        SET credentials_encrypted = $2,
            updated_at = NOW()
        WHERE id = $1
        "#,
        id,
        credentials_encrypted
    )
    .execute(pool)
    .await?;

    Ok(())
}
