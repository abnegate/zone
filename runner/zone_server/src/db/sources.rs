//! Source database queries

use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Source row from database
#[derive(Debug, Clone, sqlx::FromRow)]
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
    pub workspace_id: Option<Uuid>,
}

/// List sources with optional filters
pub async fn list_sources(
    pool: &PgPool,
    workspace_id: Uuid,
    source_type: Option<&str>,
    is_active: Option<bool>,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<SourceRow>> {
    match (source_type, is_active) {
        (Some(t), Some(a)) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
                FROM sources
                WHERE workspace_id = $1 AND source_type = $2 AND is_active = $3
                ORDER BY created_at DESC
                LIMIT $4 OFFSET $5
                "#,
            )
            .bind(workspace_id)
            .bind(t)
            .bind(a)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (Some(t), None) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
                FROM sources
                WHERE workspace_id = $1 AND source_type = $2
                ORDER BY created_at DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(workspace_id)
            .bind(t)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, Some(a)) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
                FROM sources
                WHERE workspace_id = $1 AND is_active = $2
                ORDER BY created_at DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(workspace_id)
            .bind(a)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
                FROM sources
                WHERE workspace_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(workspace_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }
}

/// Get source by ID
pub async fn get_source(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
        FROM sources
        WHERE id = $1 AND workspace_id = $2
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

/// Create a new source
pub async fn create_source(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    source_type: &str,
    config: serde_json::Value,
    description: Option<&str>,
    url: Option<&str>,
    credentials_encrypted: Option<&str>,
) -> DbResult<SourceRow> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        INSERT INTO sources (workspace_id, name, source_type, config, description, url, credentials_encrypted)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
        "#,
    )
    .bind(workspace_id)
    .bind(name)
    .bind(source_type)
    .bind(config)
    .bind(description)
    .bind(url)
    .bind(credentials_encrypted)
    .fetch_one(pool)
    .await
}

/// Update a source
pub async fn update_source(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    name: Option<&str>,
    config: Option<serde_json::Value>,
    description: Option<&str>,
    url: Option<&str>,
    is_active: Option<bool>,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        UPDATE sources
        SET name = COALESCE($3, name),
            config = COALESCE($4, config),
            description = COALESCE($5, description),
            url = COALESCE($6, url),
            is_active = COALESCE($7, is_active),
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(name)
    .bind(config)
    .bind(description)
    .bind(url)
    .bind(is_active)
    .fetch_optional(pool)
    .await
}

/// Delete a source
pub async fn delete_source(pool: &PgPool, id: Uuid, workspace_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM sources WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(workspace_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Update verification status
pub async fn update_verification(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    last_error: Option<&str>,
) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE sources
        SET last_verified_at = NOW(),
            last_error = $3,
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(last_error)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Update credentials
pub async fn update_credentials(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    credentials_encrypted: &str,
) -> DbResult<()> {
    sqlx::query(
        r#"
        UPDATE sources
        SET credentials_encrypted = $3,
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(credentials_encrypted)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get multiple sources by their IDs
///
/// Returns sources in arbitrary order. Sources that don't exist are silently skipped.
/// CRITICAL: Requires workspace_id to prevent authorization bypass.
pub async fn get_sources_by_ids(
    pool: &PgPool,
    ids: &[Uuid],
    workspace_id: Uuid,
) -> DbResult<Vec<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at, workspace_id
        FROM sources
        WHERE id = ANY($1) AND workspace_id = $2 AND is_active = true
        "#,
    )
    .bind(ids)
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

/// Index status for a source
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum IndexStatus {
    Pending,  // Never indexed
    Indexing, // Currently being indexed
    Indexed,  // Successfully indexed
    Failed,   // Last index attempt failed
    Stale,    // Config changed since last index (not implemented yet)
}

/// Source indexing status information
#[derive(Debug, Clone)]
pub struct SourceIndexStatus {
    pub status: IndexStatus,
    pub last_indexed_at: Option<NaiveDateTime>,
    pub indexed_items_count: Option<i64>,
}

/// Context gathering row (minimal, for index status queries)
#[derive(Debug, sqlx::FromRow)]
struct GatheringRow {
    pub status: String,
    pub completed_at: Option<NaiveDateTime>,
}

/// Get indexing status for a source
pub async fn get_source_index_status(
    pool: &PgPool,
    source_id: Uuid,
) -> DbResult<SourceIndexStatus> {
    // Get latest gathering for this source
    let gathering = sqlx::query_as::<_, GatheringRow>(
        r#"
        SELECT status, completed_at
        FROM context_gatherings
        WHERE $1 = ANY(source_ids)
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?;

    // Count indexed items
    let item_count: Option<i64> =
        sqlx::query_scalar("SELECT COUNT(*) FROM content_items WHERE source_id = $1")
            .bind(source_id)
            .fetch_optional(pool)
            .await?;

    // Determine status
    let status = match gathering {
        Some(ref g) if g.status == "running" => IndexStatus::Indexing,
        Some(ref g) if g.status == "completed" => IndexStatus::Indexed,
        Some(ref g) if g.status == "failed" => IndexStatus::Failed,
        _ => IndexStatus::Pending,
    };

    Ok(SourceIndexStatus {
        status,
        last_indexed_at: gathering.and_then(|g| g.completed_at),
        indexed_items_count: item_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::organizations;
    use crate::db::users;
    use crate::db::workspaces;

    async fn create_test_pool() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/zone_test".to_string()
        });

        sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to test database")
    }

    async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
        // Create organization
        let org_id = organizations::create_organization(
            pool,
            &format!("Test Org {}", Uuid::new_v4()),
            &format!("test-org-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create organization")
        .id;

        // Create workspace
        let workspace_id = workspaces::create_workspace(
            pool,
            org_id,
            &format!("Test Workspace {}", Uuid::new_v4()),
            &format!("test-ws-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create user
        let user_email = format!("test-{}@example.com", Uuid::new_v4());
        let user_id =
            users::create_user(pool, &user_email, "password_hash", Some("Test User"), false)
                .await
                .expect("Failed to create user")
                .id;

        (org_id, workspace_id, user_id)
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_create_source_sets_workspace_id() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, _user_id) = setup_test_data(&pool).await;
        let unique_name = format!("Test Source {}", Uuid::new_v4());

        let source = create_source(
            &pool,
            workspace_id,
            &unique_name,
            "github",
            serde_json::json!({"repo": "test/repo"}),
            Some("Test description"),
            Some("https://github.com/test/repo"),
            None,
        )
        .await
        .expect("Failed to create source");

        assert_eq!(
            source.workspace_id,
            Some(workspace_id),
            "Source should have workspace_id set"
        );
        assert_eq!(source.name, unique_name);
        assert_eq!(source.source_type, "github");
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_list_sources_filters_by_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_id = Uuid::new_v4();
        let source1 = create_source(
            &pool,
            workspace_id1,
            &format!("Source 1 {}", unique_id),
            "github",
            serde_json::json!({"repo": "test/repo1"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source 1");

        // Create source in workspace 2
        let _source2 = create_source(
            &pool,
            workspace_id2,
            &format!("Source 2 {}", unique_id),
            "github",
            serde_json::json!({"repo": "test/repo2"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source 2");

        // List sources for workspace 1
        let sources1 = list_sources(&pool, workspace_id1, None, None, 100, 0)
            .await
            .expect("Failed to list sources for workspace 1");

        // Verify workspace 1 only sees its own source
        assert_eq!(sources1.len(), 1, "Workspace 1 should only see 1 source");
        assert_eq!(sources1[0].id, source1.id);
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_get_source_requires_matching_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_name = format!("Test Source {}", Uuid::new_v4());
        let source = create_source(
            &pool,
            workspace_id1,
            &unique_name,
            "github",
            serde_json::json!({"repo": "test/repo"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source");

        // Try to get source with correct workspace - should succeed
        let result1 = get_source(&pool, source.id, workspace_id1)
            .await
            .expect("Failed to get source");
        assert!(
            result1.is_some(),
            "Should find source with correct workspace_id"
        );

        // Try to get source with wrong workspace - should fail
        let result2 = get_source(&pool, source.id, workspace_id2)
            .await
            .expect("Failed to query source");
        assert!(
            result2.is_none(),
            "Should not find source with incorrect workspace_id"
        );
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_update_source_requires_matching_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_name = format!("Test Source {}", Uuid::new_v4());
        let source = create_source(
            &pool,
            workspace_id1,
            &unique_name,
            "github",
            serde_json::json!({"repo": "test/repo"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source");

        // Try to update with wrong workspace - should fail
        let updated_name = format!("Updated Name {}", Uuid::new_v4());
        let result1 = update_source(
            &pool,
            source.id,
            workspace_id2,
            Some(&updated_name),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to execute update");
        assert!(
            result1.is_none(),
            "Should not update source with incorrect workspace_id"
        );

        // Try to update with correct workspace - should succeed
        let result2 = update_source(
            &pool,
            source.id,
            workspace_id1,
            Some(&updated_name),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to execute update");
        assert!(
            result2.is_some(),
            "Should update source with correct workspace_id"
        );
        assert_eq!(result2.unwrap().name, updated_name);
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_delete_source_requires_matching_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_name = format!("Test Source {}", Uuid::new_v4());
        let source = create_source(
            &pool,
            workspace_id1,
            &unique_name,
            "github",
            serde_json::json!({"repo": "test/repo"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source");

        // Try to delete with wrong workspace - should fail
        let deleted = delete_source(&pool, source.id, workspace_id2)
            .await
            .expect("Failed to execute delete");
        assert!(
            !deleted,
            "Should not delete source with incorrect workspace_id"
        );

        // Verify source still exists
        let still_exists = get_source(&pool, source.id, workspace_id1)
            .await
            .expect("Failed to get source");
        assert!(
            still_exists.is_some(),
            "Source should still exist after failed delete"
        );

        // Try to delete with correct workspace - should succeed
        let deleted = delete_source(&pool, source.id, workspace_id1)
            .await
            .expect("Failed to execute delete");
        assert!(deleted, "Should delete source with correct workspace_id");
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_get_sources_by_ids_filters_by_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create sources in both workspaces
        let unique_id = Uuid::new_v4();
        let source1 = create_source(
            &pool,
            workspace_id1,
            &format!("Source 1 {}", unique_id),
            "github",
            serde_json::json!({"repo": "test/repo1"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source 1");

        let source2 = create_source(
            &pool,
            workspace_id2,
            &format!("Source 2 {}", unique_id),
            "github",
            serde_json::json!({"repo": "test/repo2"}),
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create source 2");

        // Try to get both sources using workspace 1 - should only get source 1
        let sources = get_sources_by_ids(&pool, &[source1.id, source2.id], workspace_id1)
            .await
            .expect("Failed to get sources");

        assert_eq!(
            sources.len(),
            1,
            "Should only get sources from correct workspace"
        );
        assert_eq!(sources[0].id, source1.id);
    }
}
