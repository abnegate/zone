//! Organization database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Organization row from database
#[derive(Debug, Clone)]
pub struct OrganizationRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// List all organizations
pub async fn list_organizations(pool: &PgPool) -> DbResult<Vec<OrganizationRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, slug, description, is_active, created_at, updated_at
        FROM organizations
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| OrganizationRow {
            id: r.id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Get organization by ID
pub async fn get_organization(pool: &PgPool, id: Uuid) -> DbResult<Option<OrganizationRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, name, slug, description, is_active, created_at, updated_at
        FROM organizations
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| OrganizationRow {
        id: r.id,
        name: r.name,
        slug: r.slug,
        description: r.description,
        is_active: r.is_active,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new organization
pub async fn create_organization(
    pool: &PgPool,
    name: &str,
    slug: &str,
    description: Option<&str>,
) -> DbResult<OrganizationRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO organizations (name, slug, description)
        VALUES ($1, $2, $3)
        RETURNING id, name, slug, description, is_active, created_at, updated_at
        "#,
        name,
        slug,
        description
    )
    .fetch_one(pool)
    .await?;

    Ok(OrganizationRow {
        id: row.id,
        name: row.name,
        slug: row.slug,
        description: row.description,
        is_active: row.is_active,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update an organization
pub async fn update_organization(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    is_active: Option<bool>,
) -> DbResult<Option<OrganizationRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE organizations
        SET name = COALESCE($2, name),
            description = COALESCE($3, description),
            is_active = COALESCE($4, is_active),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, name, slug, description, is_active, created_at, updated_at
        "#,
        id,
        name,
        description,
        is_active
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| OrganizationRow {
        id: r.id,
        name: r.name,
        slug: r.slug,
        description: r.description,
        is_active: r.is_active,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Delete an organization
pub async fn delete_organization(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM organizations WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Count organizations
pub async fn count_organizations(pool: &PgPool) -> DbResult<i64> {
    let result = sqlx::query_scalar!("SELECT COUNT(*) FROM organizations")
        .fetch_one(pool)
        .await?;

    Ok(result.unwrap_or(0))
}
