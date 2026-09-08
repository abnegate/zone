//! Knowledge base database queries
//!
//! Provides persistence for user-defined knowledge entries including web link support.
//! Web links can be added with optional auto-refresh for keeping content up-to-date.

use chrono::NaiveDateTime;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use zone_context::embeddings::align_vector;

use super::DbResult;

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
    let vector = align_vector(vector).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let vector_str = format!(
        "[{}]",
        vector
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    let id = sqlx::query_scalar(
        r#"
        INSERT INTO knowledge_embeddings (knowledge_entry_id, workspace_id, vector, model)
        VALUES ($1, $2, $3::vector, $4)
        ON CONFLICT (knowledge_entry_id) DO UPDATE
        SET vector = EXCLUDED.vector, model = EXCLUDED.model
        RETURNING id
        "#,
    )
    .bind(knowledge_entry_id)
    .bind(workspace_id)
    .bind(&vector_str)
    .bind(model)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Hit from [`search_knowledge`] over user-authored knowledge entries.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KnowledgeSearchHit {
    pub entry_id: Uuid,
    pub similarity: f64,
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

/// Semantic search over workspace knowledge entries.
pub async fn search_knowledge_entries(
    pool: &PgPool,
    query_embedding: &[f32],
    workspace_id: Uuid,
    limit: i64,
    threshold: f32,
) -> DbResult<Vec<KnowledgeSearchHit>> {
    let vector = align_vector(query_embedding).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let vector_str = format!(
        "[{}]",
        vector
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    sqlx::query_as::<_, KnowledgeSearchHit>(
        r#"
        SELECT entry_id, similarity, title, content, category, tags
        FROM search_knowledge($1::vector, $2, $3, $4)
        "#,
    )
    .bind(&vector_str)
    .bind(workspace_id)
    .bind(limit as i32)
    .bind(f64::from(threshold))
    .fetch_all(pool)
    .await
}

/// Keyword search over workspace knowledge entries using the document GIN index.
pub async fn search_knowledge_keyword(
    pool: &PgPool,
    query: &str,
    workspace_id: Uuid,
    limit: i64,
) -> DbResult<Vec<KnowledgeSearchHit>> {
    let keyword = zone_context::rewrite_query(query).keyword;
    let sanitized = zone_context::embeddings::sanitize_search_query(&keyword);
    if sanitized.trim().is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, KnowledgeSearchHit>(
        r#"
        SELECT
            ke.id as entry_id,
            ts_rank_cd(
                ke.search_vector,
                websearch_to_tsquery('english', $1)
            )::FLOAT8 as similarity,
            ke.title,
            ke.content,
            ke.category,
            ke.tags
        FROM knowledge_entries ke
        WHERE ke.workspace_id = $2
          AND ke.is_active = TRUE
          AND ke.search_vector @@ websearch_to_tsquery('english', $1)
        ORDER BY similarity DESC
        LIMIT $3
        "#,
    )
    .bind(&sanitized)
    .bind(workspace_id)
    .bind(limit as i32)
    .fetch_all(pool)
    .await
}

/// Fuse semantic and keyword knowledge lists with RRF plus identifier boost.
pub fn fuse_knowledge_hits(
    semantic: Vec<KnowledgeSearchHit>,
    keyword: Vec<KnowledgeSearchHit>,
    query: &str,
    limit: usize,
) -> Vec<KnowledgeSearchHit> {
    let mut scores: std::collections::HashMap<
        Uuid,
        (KnowledgeSearchHit, Option<usize>, Option<usize>),
    > = std::collections::HashMap::new();
    for (rank, hit) in semantic.into_iter().enumerate() {
        scores.insert(hit.entry_id, (hit, None, Some(rank + 1)));
    }
    for (rank, hit) in keyword.into_iter().enumerate() {
        scores
            .entry(hit.entry_id)
            .and_modify(|(existing, kw_rank, _)| {
                *kw_rank = Some(rank + 1);
                if existing.content.is_empty() {
                    *existing = hit.clone();
                }
            })
            .or_insert((hit, Some(rank + 1), None));
    }
    let mut fused: Vec<_> = scores
        .into_values()
        .map(|(hit, kw_rank, sem_rank)| {
            let score = zone_context::score_hit(
                query,
                &format!("knowledge://{}", hit.entry_id),
                &hit.title,
                &hit.content,
                sem_rank.map(|_| hit.similarity as f32),
                kw_rank.map(|_| hit.similarity as f32),
                kw_rank,
                sem_rank,
                0.0,
            );
            (hit, score)
        })
        .collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused.into_iter().take(limit).map(|(hit, _)| hit).collect()
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

    #[test]
    fn fuse_knowledge_prefers_identifier_keyword_hits() {
        let symbol = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let neighbor = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let fused = fuse_knowledge_hits(
            vec![KnowledgeSearchHit {
                entry_id: neighbor,
                similarity: 0.81,
                title: "Auth notes".into(),
                content: "generic login form".into(),
                category: None,
                tags: Vec::new(),
            }],
            vec![KnowledgeSearchHit {
                entry_id: symbol,
                similarity: 0.04,
                title: "Blob skip".into(),
                content: "should_skip_blob returns true for unchanged SHAs".into(),
                category: None,
                tags: Vec::new(),
            }],
            "What does should_skip_blob do?",
            5,
        );
        assert_eq!(fused[0].entry_id, symbol);
    }

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
    /// Content hash when stored; used as the immutable document revision.
    pub revision: Option<String>,
}

macro_rules! documents {
    () => { r#"
    SELECT id, title, content, 'knowledge'::text AS source, NULL::uuid AS source_id,
           COALESCE(source_url, 'knowledge://' || id::text) AS uri,
           updated_at, last_fetched_at AS fetched_at, source_url IS NULL AS editable,
           content_hash AS revision
    FROM knowledge_entries
    WHERE workspace_id = $1 AND is_active = TRUE
    UNION ALL
    SELECT item.id, item.title, CASE WHEN item.metadata_only THEN NULL ELSE item.content END AS content, source.name AS source, source.id AS source_id,
           item.uri, item.modified_at AS updated_at, item.fetched_at,
           FALSE AS editable, item.content_hash AS revision
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
        "SELECT id, title, NULL::text AS content, source, source_id, uri, updated_at, fetched_at, editable, revision FROM (", documents!(), ") document
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

/// Category marking a knowledge entry as a promoted standing instruction.
pub const STANDING_INSTRUCTION_CATEGORY: &str = "standing-instruction";

const PROMOTION_TAG: &str = "promoted";
const OCCURRENCES_TAG: &str = "occurrences";
const CHATS_TAG: &str = "chats";
const CONFIRMED_TAG: &str = "confirmed";

const MAX_STANDING_INSTRUCTIONS: i64 = 40;
const CHARACTERS_PER_TOKEN: usize = 4;

fn tag(key: &str, value: &str) -> String {
    format!("{key}:{value}")
}

fn tag_value<'a>(tags: &'a [String], key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    tags.iter().find_map(|entry| entry.strip_prefix(&prefix))
}

/// Provenance recorded alongside a promoted answer so the entry stays inspectable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionProvenance {
    pub fingerprint: String,
    pub occurrences: usize,
    pub distinct_chats: usize,
    pub last_confirmed: chrono::NaiveDate,
}

impl PromotionProvenance {
    pub fn tags(&self) -> Vec<String> {
        vec![
            STANDING_INSTRUCTION_CATEGORY.to_string(),
            tag(PROMOTION_TAG, &self.fingerprint),
            tag(OCCURRENCES_TAG, &self.occurrences.to_string()),
            tag(CHATS_TAG, &self.distinct_chats.to_string()),
            tag(CONFIRMED_TAG, &self.last_confirmed.to_string()),
        ]
    }

    pub fn from_tags(tags: &[String]) -> Option<Self> {
        Some(Self {
            fingerprint: tag_value(tags, PROMOTION_TAG)?.to_string(),
            occurrences: tag_value(tags, OCCURRENCES_TAG)?.parse().ok()?,
            distinct_chats: tag_value(tags, CHATS_TAG)?.parse().ok()?,
            last_confirmed: tag_value(tags, CONFIRMED_TAG)?.parse().ok()?,
        })
    }
}

/// A standing instruction ready to be written to the knowledge store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingInstruction {
    pub workspace_id: Uuid,
    pub title: String,
    pub content: String,
    pub provenance: PromotionProvenance,
}

/// Stored standing instruction as read back for prompt assembly.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LearnedEntryRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub updated_at: Option<NaiveDateTime>,
}

/// What an upsert did, so callers can log promotions without re-reading the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeUpsertOutcome {
    Created,
    Superseded,
    Unchanged,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct KnowledgeUpsertRow {
    id: Uuid,
    created: bool,
    superseded: bool,
}

fn advisory_lock_key(workspace_id: Uuid, fingerprint: &str) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(workspace_id.as_bytes());
    hasher.update(fingerprint.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// One learned entry, addressed by the fingerprint of whatever it was derived from.
struct LearnedEntry<'a> {
    workspace_id: Uuid,
    category: &'a str,
    fingerprint: &'a str,
    identity_tag: &'a str,
    title: &'a str,
    content: &'a str,
    tags: &'a [String],
}

/// Create or supersede the knowledge entry a learning pass derived.
///
/// The fingerprint is the identity key, so re-deriving the same fact updates one row
/// instead of accumulating near-duplicates, and an unchanged derivation reports
/// [`KnowledgeUpsertOutcome::Unchanged`] without writing. The advisory lock keeps
/// concurrent server instances from racing the existence check.
async fn upsert_learned_entry(
    pool: &PgPool,
    entry: LearnedEntry<'_>,
) -> DbResult<(Uuid, KnowledgeUpsertOutcome)> {
    let token_count = entry.content.chars().count().div_ceil(CHARACTERS_PER_TOKEN) as i32;

    let mut transaction = pool.begin().await?;

    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_lock_key(entry.workspace_id, entry.fingerprint))
        .execute(&mut *transaction)
        .await?;

    let row: KnowledgeUpsertRow = sqlx::query_as(
        r#"
        WITH existing AS (
            SELECT id, title, content, tags, is_active
            FROM knowledge_entries
            WHERE workspace_id = $1 AND category = $2 AND $3 = ANY(tags)
            ORDER BY created_at, id
            LIMIT 1
        ),
        superseded AS (
            UPDATE knowledge_entries AS entry
            SET title = $4,
                content = $5,
                tags = $6,
                token_count = $7,
                is_active = TRUE,
                updated_at = NOW()
            FROM existing
            WHERE entry.id = existing.id
              AND (existing.title IS DISTINCT FROM $4
                   OR existing.content IS DISTINCT FROM $5
                   OR existing.tags IS DISTINCT FROM $6
                   OR existing.is_active IS DISTINCT FROM TRUE)
            RETURNING entry.id
        ),
        created AS (
            INSERT INTO knowledge_entries (
                workspace_id, title, content, category, tags, token_count
            )
            SELECT $1, $4, $5, $2, $6, $7
            WHERE NOT EXISTS (SELECT 1 FROM existing)
            RETURNING id
        )
        SELECT
            COALESCE(
                (SELECT id FROM created),
                (SELECT id FROM superseded),
                (SELECT id FROM existing)
            ) AS id,
            EXISTS (SELECT 1 FROM created) AS created,
            EXISTS (SELECT 1 FROM superseded) AS superseded
        "#,
    )
    .bind(entry.workspace_id)
    .bind(entry.category)
    .bind(entry.identity_tag)
    .bind(entry.title)
    .bind(entry.content)
    .bind(entry.tags)
    .bind(token_count)
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;

    let outcome = match (row.created, row.superseded) {
        (true, _) => KnowledgeUpsertOutcome::Created,
        (_, true) => KnowledgeUpsertOutcome::Superseded,
        _ => KnowledgeUpsertOutcome::Unchanged,
    };

    Ok((row.id, outcome))
}

/// Create or supersede the standing instruction identified by its promotion fingerprint.
pub async fn upsert_standing_instruction(
    pool: &PgPool,
    instruction: &StandingInstruction,
) -> DbResult<(Uuid, KnowledgeUpsertOutcome)> {
    let tags = instruction.provenance.tags();
    let identity_tag = tag(PROMOTION_TAG, &instruction.provenance.fingerprint);

    upsert_learned_entry(
        pool,
        LearnedEntry {
            workspace_id: instruction.workspace_id,
            category: STANDING_INSTRUCTION_CATEGORY,
            fingerprint: &instruction.provenance.fingerprint,
            identity_tag: &identity_tag,
            title: &instruction.title,
            content: &instruction.content,
            tags: &tags,
        },
    )
    .await
}

/// Active standing instructions for a workspace, most recently confirmed first.
pub async fn list_standing_instructions(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<Vec<LearnedEntryRow>> {
    sqlx::query_as::<_, LearnedEntryRow>(
        r#"
        SELECT id, workspace_id, title, content, tags, updated_at
        FROM knowledge_entries
        WHERE workspace_id = $1 AND category = $2 AND is_active = TRUE
        ORDER BY updated_at DESC NULLS LAST, id
        LIMIT $3
        "#,
    )
    .bind(workspace_id)
    .bind(STANDING_INSTRUCTION_CATEGORY)
    .bind(MAX_STANDING_INSTRUCTIONS)
    .fetch_all(pool)
    .await
}

/// Withdraw a promoted instruction. Promotion is reversible: a later scan re-creates it
/// only while the question keeps recurring.
pub async fn retire_standing_instruction(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE knowledge_entries
        SET is_active = FALSE, updated_at = NOW()
        WHERE id = $1 AND category = $2 AND is_active = TRUE
        "#,
    )
    .bind(id)
    .bind(STANDING_INSTRUCTION_CATEGORY)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Render standing instructions as a system-prompt section. Empty when there are none.
pub fn render_standing_instructions(instructions: &[LearnedEntryRow]) -> String {
    if instructions.is_empty() {
        return String::new();
    }

    let mut rendered = String::from(
        "\n\n# Standing instructions\n\
         These answers have already been given repeatedly in this workspace. Follow them \
         unless the user's request contradicts one, and say so when you depart from one.\n",
    );

    for instruction in instructions {
        rendered.push_str(&format!(
            "\n## {}\n{}\n",
            instruction.title.trim(),
            instruction.content.trim()
        ));
        if let Some(provenance) = PromotionProvenance::from_tags(&instruction.tags) {
            rendered.push_str(&format!(
                "(promoted from {} matching answers across {} chats, last confirmed {})\n",
                provenance.occurrences, provenance.distinct_chats, provenance.last_confirmed
            ));
        }
    }

    rendered
}

/// Standing instructions for a workspace, ready to append to a system prompt.
pub async fn standing_instructions_prompt(pool: &PgPool, workspace_id: Uuid) -> DbResult<String> {
    let instructions = list_standing_instructions(pool, workspace_id).await?;
    Ok(render_standing_instructions(&instructions))
}

const LEARNED_TAG: &str = "learned";
const OBSERVATIONS_TAG: &str = "observations";
const RUNS_TAG: &str = "runs";
const CONFIDENCE_TAG: &str = "confidence";

const MAX_LEARNED_FACTS: i64 = 40;

/// What a learning pass concluded about a workspace.
///
/// Each variant is its own `knowledge_entries` category, so a kind of lesson can be
/// listed, rendered and retired without touching the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LearnedCategory {
    /// How this repository is arranged, read out of the diffs its runs produced.
    RepositoryConvention,
    /// Which way of working produced well-received changes here.
    StrategyLesson,
}

impl LearnedCategory {
    pub const ALL: [LearnedCategory; 2] = [
        LearnedCategory::RepositoryConvention,
        LearnedCategory::StrategyLesson,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LearnedCategory::RepositoryConvention => "repository-convention",
            LearnedCategory::StrategyLesson => "strategy-lesson",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == text)
    }

    /// The heading the facts appear under in a system prompt.
    pub fn heading(self) -> &'static str {
        match self {
            LearnedCategory::RepositoryConvention => "Repository conventions",
            LearnedCategory::StrategyLesson => "What has worked here",
        }
    }

    fn preamble(self) -> &'static str {
        match self {
            LearnedCategory::RepositoryConvention => {
                "These were read out of changes this repository has already accepted. \
                 Follow them unless the task says otherwise, and say so when you depart from one."
            }
            LearnedCategory::StrategyLesson => {
                "These describe how past changes to this repository were made and how well \
                 they were received. They are observations, not instructions."
            }
        }
    }
}

impl std::fmt::Display for LearnedCategory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Where a learned fact came from, so it can be inspected and argued with.
#[derive(Debug, Clone, PartialEq)]
pub struct LearningProvenance {
    /// Identity of the fact's subject, not of its current answer, so a repository that
    /// changes its mind supersedes one entry instead of gaining a second.
    pub fingerprint: String,
    pub observations: usize,
    pub distinct_runs: usize,
    pub confidence: f32,
    pub last_confirmed: chrono::NaiveDate,
}

impl LearningProvenance {
    /// Confidence is rendered to two places so an unchanged derivation produces
    /// byte-identical tags and the upsert reports no change.
    pub fn tags(&self, category: LearnedCategory) -> Vec<String> {
        vec![
            category.as_str().to_string(),
            tag(LEARNED_TAG, &self.fingerprint),
            tag(OBSERVATIONS_TAG, &self.observations.to_string()),
            tag(RUNS_TAG, &self.distinct_runs.to_string()),
            tag(CONFIDENCE_TAG, &format!("{:.2}", self.confidence)),
            tag(CONFIRMED_TAG, &self.last_confirmed.to_string()),
        ]
    }

    pub fn from_tags(tags: &[String]) -> Option<Self> {
        Some(Self {
            fingerprint: tag_value(tags, LEARNED_TAG)?.to_string(),
            observations: tag_value(tags, OBSERVATIONS_TAG)?.parse().ok()?,
            distinct_runs: tag_value(tags, RUNS_TAG)?.parse().ok()?,
            confidence: tag_value(tags, CONFIDENCE_TAG)?.parse().ok()?,
            last_confirmed: tag_value(tags, CONFIRMED_TAG)?.parse().ok()?,
        })
    }
}

/// A fact a learning pass derived, ready to be written to the knowledge store.
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedFact {
    pub workspace_id: Uuid,
    pub category: LearnedCategory,
    pub title: String,
    pub content: String,
    pub provenance: LearningProvenance,
}

/// Create or supersede one learned fact.
pub async fn upsert_learned_fact(
    pool: &PgPool,
    fact: &LearnedFact,
) -> DbResult<(Uuid, KnowledgeUpsertOutcome)> {
    let tags = fact.provenance.tags(fact.category);
    let identity_tag = tag(LEARNED_TAG, &fact.provenance.fingerprint);

    upsert_learned_entry(
        pool,
        LearnedEntry {
            workspace_id: fact.workspace_id,
            category: fact.category.as_str(),
            fingerprint: &fact.provenance.fingerprint,
            identity_tag: &identity_tag,
            title: &fact.title,
            content: &fact.content,
            tags: &tags,
        },
    )
    .await
}

/// Active learned facts of one kind, most recently confirmed first.
pub async fn list_learned_facts(
    pool: &PgPool,
    workspace_id: Uuid,
    category: LearnedCategory,
) -> DbResult<Vec<LearnedEntryRow>> {
    sqlx::query_as::<_, LearnedEntryRow>(
        r#"
        SELECT id, workspace_id, title, content, tags, updated_at
        FROM knowledge_entries
        WHERE workspace_id = $1 AND category = $2 AND is_active = TRUE
        ORDER BY updated_at DESC NULLS LAST, id
        LIMIT $3
        "#,
    )
    .bind(workspace_id)
    .bind(category.as_str())
    .bind(MAX_LEARNED_FACTS)
    .fetch_all(pool)
    .await
}

/// Withdraw a learned fact. Learning is reversible: a later pass re-derives it only
/// while the evidence still supports it.
pub async fn retire_learned_fact(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE knowledge_entries
        SET is_active = FALSE, updated_at = NOW()
        WHERE id = $1 AND category = ANY($2) AND is_active = TRUE
        "#,
    )
    .bind(id)
    .bind(
        LearnedCategory::ALL
            .iter()
            .map(|category| category.as_str().to_string())
            .collect::<Vec<_>>(),
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Render learned facts as a system-prompt section. Empty when there are none.
pub fn render_learned_facts(category: LearnedCategory, facts: &[LearnedEntryRow]) -> String {
    if facts.is_empty() {
        return String::new();
    }

    let mut rendered = format!("\n\n# {}\n{}\n", category.heading(), category.preamble());

    for fact in facts {
        rendered.push_str(&format!("\n- {}", fact.content.trim()));
        if let Some(provenance) = LearningProvenance::from_tags(&fact.tags) {
            rendered.push_str(&format!(
                " (seen {} times across {} runs, confidence {:.2}, last confirmed {})",
                provenance.observations,
                provenance.distinct_runs,
                provenance.confidence,
                provenance.last_confirmed
            ));
        }
        rendered.push('\n');
    }

    rendered
}

/// Everything a workspace has learned, ready to append to a system prompt.
pub async fn learned_facts_prompt(pool: &PgPool, workspace_id: Uuid) -> DbResult<String> {
    let mut rendered = String::new();
    for category in LearnedCategory::ALL {
        let facts = list_learned_facts(pool, workspace_id, category).await?;
        rendered.push_str(&render_learned_facts(category, &facts));
    }
    Ok(rendered)
}

#[cfg(test)]
mod learned_fact_tests {
    use super::*;
    use chrono::NaiveDate;

    fn provenance(
        observations: usize,
        distinct_runs: usize,
        confidence: f32,
    ) -> LearningProvenance {
        LearningProvenance {
            fingerprint: "5f2c9a".to_string(),
            observations,
            distinct_runs,
            confidence,
            last_confirmed: NaiveDate::from_ymd_opt(2026, 9, 4).unwrap(),
        }
    }

    fn row(content: &str, tags: Vec<String>) -> LearnedEntryRow {
        LearnedEntryRow {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            title: "Convention".to_string(),
            content: content.to_string(),
            tags,
            updated_at: None,
        }
    }

    #[test]
    fn provenance_round_trips_through_tags() {
        let original = provenance(9, 4, 0.82);
        let parsed =
            LearningProvenance::from_tags(&original.tags(LearnedCategory::RepositoryConvention))
                .expect("provenance should parse back from its own tags");
        assert_eq!(parsed, original);
    }

    #[test]
    fn provenance_tags_are_byte_identical_for_an_unchanged_derivation() {
        let category = LearnedCategory::RepositoryConvention;
        assert_eq!(
            provenance(9, 4, 0.823_456).tags(category),
            provenance(9, 4, 0.821_111).tags(category),
            "confidence noise below the recorded precision must not rewrite the row"
        );
    }

    #[test]
    fn provenance_tags_carry_the_category_marker() {
        for category in LearnedCategory::ALL {
            assert!(
                provenance(5, 3, 0.7)
                    .tags(category)
                    .iter()
                    .any(|entry| entry == category.as_str()),
                "{category} entries must be findable by their category tag"
            );
        }
    }

    #[test]
    fn provenance_rejects_incomplete_tags() {
        assert!(LearningProvenance::from_tags(&[]).is_none());
        assert!(LearningProvenance::from_tags(&["learned:5f2c9a".to_string()]).is_none());
    }

    #[test]
    fn category_names_round_trip() {
        for category in LearnedCategory::ALL {
            assert_eq!(LearnedCategory::parse(category.as_str()), Some(category));
        }
        assert_eq!(LearnedCategory::parse("standing-instruction"), None);
    }

    #[test]
    fn learned_categories_do_not_collide_with_standing_instructions() {
        for category in LearnedCategory::ALL {
            assert_ne!(
                category.as_str(),
                STANDING_INSTRUCTION_CATEGORY,
                "a learned fact must never be listed as a standing instruction"
            );
        }
    }

    #[test]
    fn render_is_empty_without_facts() {
        assert!(render_learned_facts(LearnedCategory::RepositoryConvention, &[]).is_empty());
    }

    #[test]
    fn render_discloses_the_evidence_behind_each_fact() {
        let rendered = render_learned_facts(
            LearnedCategory::RepositoryConvention,
            &[row(
                "Files in `src/db` are named in snake_case.",
                provenance(9, 4, 0.82).tags(LearnedCategory::RepositoryConvention),
            )],
        );

        assert!(rendered.contains("# Repository conventions"));
        assert!(rendered.contains("Files in `src/db` are named in snake_case."));
        assert!(
            rendered.contains("seen 9 times across 4 runs, confidence 0.82"),
            "a learned fact must show what it was derived from: {rendered}"
        );
        assert!(rendered.contains("last confirmed 2026-09-04"));
    }

    #[test]
    fn render_survives_missing_provenance_tags() {
        let rendered =
            render_learned_facts(LearnedCategory::StrategyLesson, &[row("Body", Vec::new())]);
        assert!(rendered.contains("Body"));
        assert!(!rendered.contains("seen "));
    }
}

#[cfg(test)]
mod standing_instruction_tests {
    use super::*;
    use chrono::NaiveDate;

    fn provenance(occurrences: usize, distinct_chats: usize) -> PromotionProvenance {
        PromotionProvenance {
            fingerprint: "abc123".to_string(),
            occurrences,
            distinct_chats,
            last_confirmed: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
        }
    }

    fn row(title: &str, content: &str, tags: Vec<String>) -> LearnedEntryRow {
        LearnedEntryRow {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            title: title.to_string(),
            content: content.to_string(),
            tags,
            updated_at: None,
        }
    }

    #[test]
    fn provenance_round_trips_through_tags() {
        let original = provenance(7, 4);
        let parsed = PromotionProvenance::from_tags(&original.tags())
            .expect("provenance should parse back from its own tags");
        assert_eq!(parsed, original);
    }

    #[test]
    fn provenance_tags_carry_the_category_marker() {
        assert!(
            provenance(5, 3)
                .tags()
                .iter()
                .any(|entry| entry == STANDING_INSTRUCTION_CATEGORY),
            "tags must include the standing-instruction marker"
        );
    }

    #[test]
    fn provenance_rejects_incomplete_tags() {
        assert!(
            PromotionProvenance::from_tags(&["promoted:abc123".to_string()]).is_none(),
            "tags missing occurrence counts must not parse"
        );
        assert!(
            PromotionProvenance::from_tags(&[]).is_none(),
            "empty tags must not parse"
        );
    }

    #[test]
    fn render_is_empty_without_instructions() {
        assert!(render_standing_instructions(&[]).is_empty());
    }

    #[test]
    fn render_includes_instruction_and_provenance() {
        let rendered = render_standing_instructions(&[row(
            "Repeated answer: how do I run the tests",
            "Run cargo test from the runner directory.",
            provenance(6, 4).tags(),
        )]);

        assert!(rendered.contains("# Standing instructions"));
        assert!(rendered.contains("Run cargo test from the runner directory."));
        assert!(
            rendered.contains("promoted from 6 matching answers across 4 chats"),
            "rendered prompt must disclose how often the answer was seen: {rendered}"
        );
        assert!(rendered.contains("last confirmed 2026-09-01"));
    }

    #[test]
    fn render_survives_missing_provenance_tags() {
        let rendered = render_standing_instructions(&[row("Title", "Body", Vec::new())]);
        assert!(rendered.contains("Body"));
        assert!(!rendered.contains("promoted from"));
    }

    #[test]
    fn advisory_lock_key_is_stable_and_scoped() {
        let workspace = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        assert_eq!(
            advisory_lock_key(workspace, "fingerprint"),
            advisory_lock_key(workspace, "fingerprint")
        );
        assert_ne!(
            advisory_lock_key(workspace, "fingerprint"),
            advisory_lock_key(other, "fingerprint")
        );
        assert_ne!(
            advisory_lock_key(workspace, "fingerprint"),
            advisory_lock_key(workspace, "other")
        );
    }
}
