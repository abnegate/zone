//! Knowledge base database queries
//!
//! Provides persistence for user-defined knowledge entries including web link support.
//! Web links can be added with optional auto-refresh for keeping content up-to-date.

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Expected embedding vector dimension
const EMBEDDING_DIMENSION: usize = 1536;

/// Knowledge entry row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KnowledgeRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub token_count: i32,
    pub is_active: bool,
    /// Optional source URL for web-linked knowledge
    pub source_url: Option<String>,
    /// When the URL content was last fetched
    pub last_fetched_at: Option<NaiveDateTime>,
    /// Hash of content for change detection
    pub content_hash: Option<String>,
    /// Auto-refresh interval in minutes (NULL = no auto-refresh)
    pub refresh_interval_minutes: Option<i32>,
    /// Last fetch error message
    pub last_fetch_error: Option<String>,
}

/// Lightweight knowledge entry for list views (without full content)
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KnowledgeListRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub token_count: i32,
    pub is_active: bool,
    /// Optional source URL for web-linked knowledge
    pub source_url: Option<String>,
    /// When the URL content was last fetched
    pub last_fetched_at: Option<NaiveDateTime>,
    /// Auto-refresh interval in minutes
    pub refresh_interval_minutes: Option<i32>,
    /// Last fetch error (indicates failed state)
    pub last_fetch_error: Option<String>,
}

/// Get a knowledge entry by ID
pub async fn get_knowledge(pool: &PgPool, id: Uuid) -> DbResult<Option<KnowledgeRow>> {
    sqlx::query_as::<_, KnowledgeRow>(
        r#"
        SELECT id, workspace_id, title, content, category, tags, token_count, is_active,
               source_url, last_fetched_at, content_hash, refresh_interval_minutes, last_fetch_error
        FROM knowledge_entries
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// List knowledge entries for a workspace (returns lightweight list items without full content)
pub async fn list_knowledge(
    pool: &PgPool,
    workspace_id: Uuid,
    category: Option<&str>,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<KnowledgeListRow>> {
    if let Some(category) = category {
        sqlx::query_as::<_, KnowledgeListRow>(
            r#"
            SELECT id, workspace_id, title, category, tags, token_count, is_active,
                   source_url, last_fetched_at, refresh_interval_minutes, last_fetch_error
            FROM knowledge_entries
            WHERE workspace_id = $1 AND category = $2 AND is_active = TRUE
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(workspace_id)
        .bind(category)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, KnowledgeListRow>(
            r#"
            SELECT id, workspace_id, title, category, tags, token_count, is_active,
                   source_url, last_fetched_at, refresh_interval_minutes, last_fetch_error
            FROM knowledge_entries
            WHERE workspace_id = $1 AND is_active = TRUE
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

/// Create a knowledge entry
pub async fn create_knowledge(
    pool: &PgPool,
    workspace_id: Uuid,
    title: &str,
    content: &str,
    category: Option<&str>,
    tags: &[String],
    token_count: i32,
    created_by: Uuid,
) -> DbResult<Uuid> {
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO knowledge_entries (workspace_id, title, content, category, tags, token_count, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#
    )
    .bind(workspace_id)
    .bind(title)
    .bind(content)
    .bind(category)
    .bind(tags)
    .bind(token_count)
    .bind(created_by)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Create a knowledge entry from a web URL
pub async fn create_knowledge_with_url(
    pool: &PgPool,
    workspace_id: Uuid,
    title: &str,
    content: &str,
    source_url: &str,
    category: Option<&str>,
    tags: &[String],
    token_count: i32,
    content_hash: &str,
    refresh_interval_minutes: Option<i32>,
    created_by: Uuid,
) -> DbResult<Uuid> {
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO knowledge_entries (
            workspace_id, title, content, source_url, category, tags, token_count,
            content_hash, refresh_interval_minutes, last_fetched_at, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), $10)
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(title)
    .bind(content)
    .bind(source_url)
    .bind(category)
    .bind(tags)
    .bind(token_count)
    .bind(content_hash)
    .bind(refresh_interval_minutes)
    .bind(created_by)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Soft-delete a knowledge entry (set is_active = false)
pub async fn delete_knowledge(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE knowledge_entries
        SET is_active = FALSE, updated_at = NOW()
        WHERE id = $1 AND is_active = TRUE
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Store embedding for a knowledge entry
pub async fn store_knowledge_embedding(
    pool: &PgPool,
    knowledge_entry_id: Uuid,
    workspace_id: Uuid,
    vector: &[f32],
    model: &str,
) -> DbResult<Uuid> {
    // Validate embedding dimension
    if vector.len() != EMBEDDING_DIMENSION {
        return Err(sqlx::Error::Protocol(format!(
            "Embedding dimension mismatch: expected {}, got {}",
            EMBEDDING_DIMENSION,
            vector.len()
        )));
    }

    let id = sqlx::query_scalar(
        r#"
        INSERT INTO knowledge_embeddings (knowledge_entry_id, workspace_id, vector, model)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (knowledge_entry_id) DO UPDATE
        SET vector = EXCLUDED.vector, model = EXCLUDED.model
        RETURNING id
        "#,
    )
    .bind(knowledge_entry_id)
    .bind(workspace_id)
    .bind(vector)
    .bind(model)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Entry due for refresh
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KnowledgeRefreshDue {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub source_url: String,
    pub content_hash: Option<String>,
    pub refresh_interval_minutes: Option<i32>,
}

/// Find knowledge entries due for refresh
///
/// Returns entries where:
/// - source_url is not null
/// - is_active = true
/// - refresh_interval_minutes is set
/// - last_fetched_at is null OR (now - last_fetched_at) > refresh_interval_minutes
pub async fn list_entries_due_for_refresh(
    pool: &PgPool,
    limit: i64,
) -> DbResult<Vec<KnowledgeRefreshDue>> {
    sqlx::query_as::<_, KnowledgeRefreshDue>(
        r#"
        SELECT id, workspace_id, title, source_url, content_hash, refresh_interval_minutes
        FROM knowledge_entries
        WHERE source_url IS NOT NULL
          AND is_active = TRUE
          AND refresh_interval_minutes IS NOT NULL
          AND refresh_interval_minutes > 0
          AND (
              last_fetched_at IS NULL
              OR last_fetched_at + (refresh_interval_minutes || ' minutes')::interval < NOW()
          )
        ORDER BY last_fetched_at ASC NULLS FIRST
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Update knowledge entry content after successful fetch
pub async fn update_knowledge_content(
    pool: &PgPool,
    id: Uuid,
    content: &str,
    token_count: i32,
    content_hash: &str,
) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE knowledge_entries
        SET content = $2,
            token_count = $3,
            content_hash = $4,
            last_fetched_at = NOW(),
            last_fetch_error = NULL,
            updated_at = NOW()
        WHERE id = $1 AND is_active = TRUE
        "#,
    )
    .bind(id)
    .bind(content)
    .bind(token_count)
    .bind(content_hash)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Record a fetch error for a knowledge entry
pub async fn record_fetch_error(pool: &PgPool, id: Uuid, error: &str) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE knowledge_entries
        SET last_fetch_error = $2,
            last_fetched_at = NOW(),
            updated_at = NOW()
        WHERE id = $1 AND is_active = TRUE
        "#,
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Check if a URL already exists in the workspace's knowledge base
pub async fn url_exists_in_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    source_url: &str,
) -> DbResult<Option<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT id
        FROM knowledge_entries
        WHERE workspace_id = $1 AND source_url = $2 AND is_active = TRUE
        LIMIT 1
        "#,
    )
    .bind(workspace_id)
    .bind(source_url)
    .fetch_optional(pool)
    .await
}

/// Manually trigger refresh for a knowledge entry
pub async fn mark_for_refresh(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    // Set last_fetched_at to epoch to force refresh on next cycle
    let result = sqlx::query(
        r#"
        UPDATE knowledge_entries
        SET last_fetched_at = '1970-01-01 00:00:00',
            updated_at = NOW()
        WHERE id = $1 AND source_url IS NOT NULL AND is_active = TRUE
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::organizations;
    use crate::db::users;
    #[allow(unused_imports)]
    use crate::db::workspace_members;
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
        // Create test user
        let email = format!("test-{}@example.com", Uuid::new_v4());
        let user_id =
            users::create_user(pool, &email, "test_password_hash", Some("Test User"), false)
                .await
                .expect("Failed to create user")
                .id;

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

        (org_id, workspace_id, user_id)
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_create_and_get_knowledge() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        let title = "Test Knowledge";
        let content = "This is test content";
        let category = Some("test");
        let tags = vec!["tag1".to_string(), "tag2".to_string()];
        let token_count = 100;

        let id = create_knowledge(
            &pool,
            workspace_id,
            title,
            content,
            category,
            &tags,
            token_count,
            user_id,
        )
        .await
        .expect("Failed to create knowledge");

        let retrieved = get_knowledge(&pool, id)
            .await
            .expect("Failed to get knowledge")
            .expect("Knowledge not found");

        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.workspace_id, workspace_id);
        assert_eq!(retrieved.title, title);
        assert_eq!(retrieved.content, content);
        assert_eq!(retrieved.category, category.map(|s| s.to_string()));
        assert_eq!(retrieved.tags, tags);
        assert_eq!(retrieved.token_count, token_count);
        assert!(retrieved.is_active);
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_list_knowledge() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Create multiple knowledge entries
        create_knowledge(
            &pool,
            workspace_id,
            "Entry 1",
            "Content 1",
            Some("category1"),
            &[],
            50,
            user_id,
        )
        .await
        .expect("Failed to create knowledge");

        create_knowledge(
            &pool,
            workspace_id,
            "Entry 2",
            "Content 2",
            Some("category2"),
            &[],
            60,
            user_id,
        )
        .await
        .expect("Failed to create knowledge");

        // List all
        let all = list_knowledge(&pool, workspace_id, None, 100, 0)
            .await
            .expect("Failed to list knowledge");
        assert_eq!(all.len(), 2);

        // List by category
        let filtered = list_knowledge(&pool, workspace_id, Some("category1"), 100, 0)
            .await
            .expect("Failed to list knowledge");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Entry 1");
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_delete_knowledge() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        let id = create_knowledge(
            &pool,
            workspace_id,
            "To Delete",
            "Content",
            None,
            &[],
            50,
            user_id,
        )
        .await
        .expect("Failed to create knowledge");

        // Delete it
        let deleted = delete_knowledge(&pool, id)
            .await
            .expect("Failed to delete knowledge");
        assert!(deleted);

        // Verify it's soft-deleted
        let retrieved = get_knowledge(&pool, id)
            .await
            .expect("Failed to get knowledge")
            .expect("Knowledge not found");
        assert!(!retrieved.is_active);

        // Should not appear in list
        let all = list_knowledge(&pool, workspace_id, None, 100, 0)
            .await
            .expect("Failed to list knowledge");
        assert_eq!(all.len(), 0);
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
    async fn document_storage_is_complete_searchable_and_scoped() {
        let pool = create_test_pool().await;
        let (organization, workspace, user) = setup_test_data(&pool).await;
        let (foreign_organization, foreign_workspace, foreign_user) = setup_test_data(&pool).await;
        workspace_members::add_member(
            &pool,
            workspace,
            user,
            workspace_members::WorkspaceRole::Member,
            None,
        )
        .await
        .unwrap();
        workspace_members::add_member(
            &pool,
            foreign_workspace,
            foreign_user,
            workspace_members::WorkspaceRole::Member,
            None,
        )
        .await
        .unwrap();
        let content = format!(
            "  quasarneedle\n{}\n  ",
            "世界 🦀 complete document\n".repeat(2000)
        );
        let id = create_document(&pool, workspace, user, "Long note", &content)
            .await
            .unwrap()
            .unwrap();
        let document = read_document(&pool, workspace, user, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(document.content.as_deref(), Some(content.as_str()));
        assert!(document.editable);
        assert!(document.updated_at.is_some());
        assert!(
            list_knowledge(&pool, workspace, None, 25, 0)
                .await
                .unwrap()
                .iter()
                .any(|entry| entry.id == id)
        );
        let matches = list_documents(&pool, workspace, user, Some("quasarneedle"), 25, 0)
            .await
            .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, id);
        assert!(matches[0].content.is_none());
        assert!(
            read_document(&pool, foreign_workspace, foreign_user, id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !update_document(
                &pool,
                foreign_workspace,
                foreign_user,
                id,
                DocumentUpdate {
                    title: Some("foreign"),
                    content: None
                }
            )
            .await
            .unwrap()
        );
        assert!(
            list_documents(
                &pool,
                foreign_workspace,
                foreign_user,
                Some("quasarneedle"),
                25,
                0
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert!(
            create_document(&pool, foreign_workspace, user, "Denied", "Denied")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            update_document(
                &pool,
                workspace,
                user,
                id,
                DocumentUpdate {
                    title: None,
                    content: Some("  replacementneedle 世界\n")
                }
            )
            .await
            .unwrap()
        );
        let updated = read_document(&pool, workspace, user, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.title, "Long note");
        assert_eq!(
            updated.content.as_deref(),
            Some("  replacementneedle 世界\n")
        );
        assert!(
            list_documents(&pool, workspace, user, Some("quasarneedle"), 25, 0)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list_documents(&pool, workspace, user, Some("replacementneedle"), 25, 0)
                .await
                .unwrap()[0]
                .id,
            id
        );
        sqlx::query(
            "UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            read_document(&pool, workspace, user, id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            !update_document(
                &pool,
                workspace,
                user,
                id,
                DocumentUpdate {
                    title: Some("Denied"),
                    content: None
                }
            )
            .await
            .unwrap()
        );
        assert!(
            create_document(&pool, workspace, user, "Denied", "Denied")
                .await
                .unwrap()
                .is_none()
        );
        workspace_members::remove_member(&pool, workspace, user)
            .await
            .unwrap();
        assert!(
            read_document(&pool, workspace, user, id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            list_documents(&pool, workspace, user, None, 25, 0)
                .await
                .unwrap()
                .is_empty()
        );
        for organization in [organization, foreign_organization] {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(organization)
                .execute(&pool)
                .await
                .unwrap();
        }
        for user in [user, foreign_user] {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user)
                .execute(&pool)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
    async fn document_sources_enforce_workspace_and_metadata_only_state() {
        let pool = create_test_pool().await;
        let (organization, workspace, user) = setup_test_data(&pool).await;
        workspace_members::add_member(
            &pool,
            workspace,
            user,
            workspace_members::WorkspaceRole::Member,
            None,
        )
        .await
        .unwrap();
        let source: Uuid = sqlx::query_scalar("INSERT INTO sources (workspace_id, name, source_type, config) VALUES ($1, 'Repository', 'github', '{}') RETURNING id").bind(workspace).fetch_one(&pool).await.unwrap();
        let content = "full indexed 文件\n".repeat(2000);
        let id: Uuid = sqlx::query_scalar("INSERT INTO content_items (source_id, category, uri, title, content, content_hash) VALUES ($1, 'code', 'src/guide.md', 'Guide', $2, 'hash') RETURNING id").bind(source).bind(&content).fetch_one(&pool).await.unwrap();
        let document = read_document(&pool, workspace, user, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(document.content.as_deref(), Some(content.as_str()));
        assert_eq!(document.source_id, Some(source));
        assert_eq!(document.uri, "src/guide.md");
        assert!(document.fetched_at.is_some());
        assert!(!document.editable);
        assert!(
            !update_document(
                &pool,
                workspace,
                user,
                id,
                DocumentUpdate {
                    title: Some("Denied"),
                    content: None
                }
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE content_items SET metadata_only = TRUE WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            read_document(&pool, workspace, user, id)
                .await
                .unwrap()
                .unwrap()
                .content
                .is_none()
        );
        sqlx::query("UPDATE sources SET is_active = FALSE WHERE id = $1")
            .bind(source)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            read_document(&pool, workspace, user, id)
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
    }
}

/// A complete stored document, with provenance and freshness for citations.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Document {
    pub id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub source: String,
    pub source_id: Option<Uuid>,
    pub uri: String,
    pub updated_at: Option<NaiveDateTime>,
    pub fetched_at: Option<NaiveDateTime>,
    pub editable: bool,
}

macro_rules! documents {
    () => { r#"
    SELECT id, title, content, 'knowledge'::text AS source, NULL::uuid AS source_id,
           COALESCE(source_url, 'knowledge://' || id::text) AS uri,
           updated_at, last_fetched_at AS fetched_at, source_url IS NULL AS editable
    FROM knowledge_entries
    WHERE workspace_id = $1 AND is_active = TRUE
    UNION ALL
    SELECT item.id, item.title, CASE WHEN item.metadata_only THEN NULL ELSE item.content END AS content, source.name AS source, source.id AS source_id,
           item.uri, item.modified_at AS updated_at, item.fetched_at,
           FALSE AS editable
    FROM content_items item
    JOIN sources source ON source.id = item.source_id
    WHERE source.workspace_id = $1 AND source.is_active = TRUE
      AND (item.workspace_id IS NULL OR item.workspace_id = $1)
"# };
}

/// List stored documents or find documents by full-text query.
/// Membership is checked in the same database statement as the read.
pub async fn list_documents(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    query: Option<&str>,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<Document>> {
    sqlx::query_as::<_, Document>(concat!(
        "SELECT id, title, NULL::text AS content, source, source_id, uri, updated_at, fetched_at, editable FROM (", documents!(), ") document
         WHERE check_workspace_membership($2, $1)
           AND ($3::text IS NULL OR to_tsvector('english', title || ' ' || COALESCE(content, ''))
                @@ plainto_tsquery('english', $3))
         ORDER BY updated_at DESC NULLS LAST, id LIMIT $4 OFFSET $5"
    ))
    .bind(workspace_id)
    .bind(user_id)
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Read exact stored content without snippet or character limits.
pub async fn read_document(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    id: Uuid,
) -> DbResult<Option<Document>> {
    sqlx::query_as::<_, Document>(concat!(
        "SELECT * FROM (",
        documents!(),
        ") document WHERE id = $3 AND check_workspace_membership($2, $1)"
    ))
    .bind(workspace_id)
    .bind(user_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Persist a workspace note; its full-text index updates atomically with the row.
pub async fn create_document(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    title: &str,
    content: &str,
) -> DbResult<Option<Uuid>> {
    sqlx::query_scalar(
        "INSERT INTO knowledge_entries (workspace_id, title, content, token_count, created_by)
         SELECT $1, $3, $4, ceil(length($4::text)::numeric / 4)::integer, $2
         WHERE get_workspace_role($2, $1) IN ('member', 'admin', 'owner') RETURNING id",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(title)
    .bind(content)
    .fetch_optional(pool)
    .await
}

/// Sparse edits to local notes; imported documents must be edited at their source.
#[derive(Debug, Default)]
pub struct DocumentUpdate<'a> {
    pub title: Option<&'a str>,
    pub content: Option<&'a str>,
}

pub async fn update_document(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    id: Uuid,
    update: DocumentUpdate<'_>,
) -> DbResult<bool> {
    let mut transaction = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE knowledge_entries
         SET title = COALESCE($4, title), content = COALESCE($5, content),
             token_count = CASE WHEN $5::text IS NULL THEN token_count
                                ELSE ceil(length($5::text)::numeric / 4)::integer END,
             content_hash = CASE WHEN $5::text IS NULL THEN content_hash ELSE NULL END,
             updated_at = NOW()
         WHERE id = $3 AND workspace_id = $1 AND is_active = TRUE AND source_url IS NULL
           AND get_workspace_role($2, $1) IN ('member', 'admin', 'owner')",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(id)
    .bind(update.title)
    .bind(update.content)
    .execute(&mut *transaction)
    .await?
    .rows_affected()
        > 0;
    if changed && update.content.is_some() {
        sqlx::query(
            "DELETE FROM knowledge_embeddings WHERE knowledge_entry_id = $1 AND workspace_id = $2",
        )
        .bind(id)
        .bind(workspace_id)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(changed)
}
