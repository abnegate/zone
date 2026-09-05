//! PostgreSQL vector store implementation using pgvector
//!
//! This module provides a VectorStore implementation backed by PostgreSQL
//! with the pgvector extension for efficient similarity search.

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::content::{ContentChunk, ContentItem};
use crate::error::{ContextError, Result};

use super::{Embedding, SearchFilters, SearchResult, VectorStore};

/// Vector dimension hardcoded to match database schema vector(1024)
pub const VECTOR_DIMENSION: usize = 1024;

/// Pad shorter embedding models (e.g. nomic 768) to the schema width.
/// Cosine similarity is preserved when query and document use the same padding.
pub fn align_vector(vector: &[f32]) -> Result<Vec<f32>> {
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(ContextError::VectorStore(
            "Embedding contains non-finite values".to_string(),
        ));
    }
    if vector.len() == VECTOR_DIMENSION {
        return Ok(vector.to_vec());
    }
    if vector.len() < VECTOR_DIMENSION {
        let mut padded = vector.to_vec();
        padded.resize(VECTOR_DIMENSION, 0.0);
        return Ok(padded);
    }
    Err(ContextError::EmbeddingDimensionMismatch {
        expected: VECTOR_DIMENSION,
        actual: vector.len(),
    })
}

/// Maximum search limit to prevent excessive resource usage
const MAX_SEARCH_LIMIT: usize = 1000;

/// PostgreSQL vector store using pgvector extension
pub struct PgVectorStore {
    pool: PgPool,
}

impl PgVectorStore {
    /// Create a new PgVectorStore with the given pool
    ///
    /// Note: The dimension is hardcoded to 1024 to match the database schema.
    /// If you need a different dimension, you must update the database schema.
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get the configured dimension (always 1024)
    pub fn dimension(&self) -> usize {
        VECTOR_DIMENSION
    }

    /// Store a content item in the database
    ///
    /// Returns the UUID of the content item (existing or newly created)
    pub async fn store_content_item(&self, item: &ContentItem) -> Result<Uuid> {
        let content_hash = item.content_hash();
        let category = format!("{:?}", item.category).to_lowercase();

        // Convert timestamps to NaiveDateTime for PostgreSQL
        let modified_at = item.modified_at.map(|dt| dt.naive_utc());
        let fetched_at = item.fetched_at.naive_utc();

        #[derive(sqlx::FromRow)]
        struct IdRow {
            id: Uuid,
        }

        let result: IdRow = sqlx::query_as(
            r#"
            INSERT INTO content_items (
                id, source_id, workspace_id, category, uri, title, content, content_type,
                token_count, metadata_only, content_hash, metadata, modified_at, fetched_at
            )
            VALUES (
                $1, $2, (SELECT workspace_id FROM sources WHERE id = $2),
                $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13
            )
            ON CONFLICT (source_id, uri) DO UPDATE SET
                title = EXCLUDED.title,
                content = EXCLUDED.content,
                content_type = EXCLUDED.content_type,
                token_count = EXCLUDED.token_count,
                metadata_only = EXCLUDED.metadata_only,
                content_hash = EXCLUDED.content_hash,
                metadata = EXCLUDED.metadata,
                modified_at = EXCLUDED.modified_at,
                fetched_at = EXCLUDED.fetched_at,
                workspace_id = COALESCE(content_items.workspace_id, EXCLUDED.workspace_id),
                updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(item.id)
        .bind(item.source_id)
        .bind(&category)
        .bind(&item.uri)
        .bind(&item.title)
        .bind(&item.content)
        .bind(&item.content_type)
        .bind(item.token_count as i32)
        .bind(item.metadata_only)
        .bind(&content_hash)
        .bind(serde_json::to_value(&item.metadata)?)
        .bind(modified_at)
        .bind(fetched_at)
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    /// Store content chunks in the database
    ///
    /// Note: This uses a transaction to ensure all chunks are stored atomically.
    /// The N+1 query pattern (loop with individual INSERTs) is a known limitation
    /// that could be optimized with batch INSERT in the future.
    pub async fn store_content_chunks(&self, chunks: &[ContentChunk]) -> Result<()> {
        // Use a transaction to prevent partial chunk sets on failure
        let mut tx = self.pool.begin().await?;

        for chunk in chunks {
            // Validate conversions before executing query
            let chunk_index: i32 = chunk.chunk_index.try_into().map_err(|_| {
                ContextError::Parse(format!(
                    "chunk_index {} exceeds i32::MAX",
                    chunk.chunk_index
                ))
            })?;
            let token_count: i32 = chunk.token_count.try_into().map_err(|_| {
                ContextError::Parse(format!(
                    "token_count {} exceeds i32::MAX",
                    chunk.token_count
                ))
            })?;
            let start_offset: i32 = chunk.start_offset.try_into().map_err(|_| {
                ContextError::Parse(format!(
                    "start_offset {} exceeds i32::MAX",
                    chunk.start_offset
                ))
            })?;
            let end_offset: i32 = chunk.end_offset.try_into().map_err(|_| {
                ContextError::Parse(format!("end_offset {} exceeds i32::MAX", chunk.end_offset))
            })?;

            sqlx::query(
                r#"
                INSERT INTO content_chunks (id, content_item_id, chunk_index, text, token_count, start_offset, end_offset)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (content_item_id, chunk_index) DO UPDATE SET
                    text = EXCLUDED.text,
                    token_count = EXCLUDED.token_count,
                    start_offset = EXCLUDED.start_offset,
                    end_offset = EXCLUDED.end_offset
                "#
            )
            .bind(chunk.id)
            .bind(chunk.content_item_id)
            .bind(chunk_index)
            .bind(&chunk.text)
            .bind(token_count)
            .bind(start_offset)
            .bind(end_offset)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Existing content hash for incremental indexing
    pub async fn content_item_hash(
        &self,
        source_id: Uuid,
        uri: &str,
    ) -> Result<Option<(Uuid, String)>> {
        #[derive(sqlx::FromRow)]
        struct HashRow {
            id: Uuid,
            content_hash: String,
        }

        let row: Option<HashRow> = sqlx::query_as(
            "SELECT id, content_hash FROM content_items WHERE source_id = $1 AND uri = $2",
        )
        .bind(source_id)
        .bind(uri)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| (row.id, row.content_hash)))
    }

    /// Whether an item already has searchable embeddings
    pub async fn content_item_has_embeddings(&self, content_item_id: Uuid) -> Result<bool> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM embeddings WHERE content_item_id = $1")
                .bind(content_item_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count > 0)
    }

    /// Replace all chunks for an item so embedding IDs stay consistent
    pub async fn replace_content_chunks(
        &self,
        item_id: Uuid,
        chunks: &[ContentChunk],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM content_chunks WHERE content_item_id = $1")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;

        for chunk in chunks {
            let chunk_index: i32 = chunk.chunk_index.try_into().map_err(|_| {
                ContextError::Parse(format!(
                    "chunk_index {} exceeds i32::MAX",
                    chunk.chunk_index
                ))
            })?;
            let token_count: i32 = chunk.token_count.try_into().map_err(|_| {
                ContextError::Parse(format!(
                    "token_count {} exceeds i32::MAX",
                    chunk.token_count
                ))
            })?;
            let start_offset: i32 = chunk.start_offset.try_into().map_err(|_| {
                ContextError::Parse(format!(
                    "start_offset {} exceeds i32::MAX",
                    chunk.start_offset
                ))
            })?;
            let end_offset: i32 = chunk.end_offset.try_into().map_err(|_| {
                ContextError::Parse(format!("end_offset {} exceeds i32::MAX", chunk.end_offset))
            })?;

            sqlx::query(
                r#"
                INSERT INTO content_chunks (id, content_item_id, chunk_index, text, token_count, start_offset, end_offset)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(chunk.id)
            .bind(item_id)
            .bind(chunk_index)
            .bind(&chunk.text)
            .bind(token_count)
            .bind(start_offset)
            .bind(end_offset)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Drop indexed files that were not present in the latest successful fetch
    pub async fn retain_content_uris(&self, source_id: Uuid, uris: &[String]) -> Result<usize> {
        let result = if uris.is_empty() {
            sqlx::query("DELETE FROM content_items WHERE source_id = $1")
                .bind(source_id)
                .execute(&self.pool)
                .await?
        } else {
            sqlx::query("DELETE FROM content_items WHERE source_id = $1 AND NOT (uri = ANY($2))")
                .bind(source_id)
                .bind(uris)
                .execute(&self.pool)
                .await?
        };
        Ok(result.rows_affected() as usize)
    }

    /// Blob SHAs and embedding coverage for incremental Git fetches
    pub async fn list_indexed_blobs(
        &self,
        source_id: Uuid,
    ) -> Result<std::collections::HashMap<String, crate::content::IndexedBlob>> {
        #[derive(sqlx::FromRow)]
        struct BlobRow {
            uri: String,
            blob_sha: Option<String>,
            has_embeddings: bool,
        }

        let rows: Vec<BlobRow> = sqlx::query_as(
            r#"
            SELECT
                ci.uri,
                ci.metadata->>'commit_hash' AS blob_sha,
                EXISTS(
                    SELECT 1 FROM embeddings e WHERE e.content_item_id = ci.id
                ) AS has_embeddings
            FROM content_items ci
            WHERE ci.source_id = $1
            "#,
        )
        .bind(source_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.uri,
                    crate::content::IndexedBlob {
                        blob_sha: row.blob_sha,
                        has_embeddings: row.has_embeddings,
                    },
                )
            })
            .collect())
    }

    /// Last persisted source version (tree SHA / fingerprint)
    pub async fn load_sync_version(&self, source_id: Uuid) -> Result<Option<String>> {
        let version: Option<Option<String>> =
            sqlx::query_scalar("SELECT version FROM source_sync_state WHERE source_id = $1")
                .bind(source_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(version.flatten())
    }

    /// Persist the source version after a successful index pass
    pub async fn save_sync_version(&self, source_id: Uuid, version: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO source_sync_state (source_id, last_sync_at, version, extra)
            VALUES ($1, NOW(), $2, '{}'::jsonb)
            ON CONFLICT (source_id) DO UPDATE SET
                last_sync_at = NOW(),
                version = EXCLUDED.version,
                updated_at = NOW()
            "#,
        )
        .bind(source_id)
        .bind(version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a content item by ID
    pub async fn get_content_item(&self, id: Uuid) -> Result<Option<ContentItem>> {
        #[derive(sqlx::FromRow)]
        struct ContentItemRow {
            id: Uuid,
            source_id: Uuid,
            category: String,
            uri: String,
            title: String,
            content: Option<String>,
            content_type: String,
            token_count: i32,
            metadata_only: bool,
            #[allow(dead_code)] // Retrieved from DB but not used in reconstruction
            content_hash: String,
            metadata: serde_json::Value,
            modified_at: Option<chrono::NaiveDateTime>,
            fetched_at: chrono::NaiveDateTime,
        }

        let record: Option<ContentItemRow> = sqlx::query_as(
            r#"
            SELECT
                id, source_id, category, uri, title, content, content_type,
                token_count, metadata_only, content_hash, metadata, modified_at, fetched_at
            FROM content_items
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(record) = record {
            use std::str::FromStr;

            // Parse category using FromStr implementation
            let category = crate::content::ContentCategory::from_str(&record.category)
                .map_err(ContextError::Parse)?;

            let metadata: crate::content::ContentMetadata =
                serde_json::from_value(record.metadata)?;

            // Validate i32 to usize conversion (negative values would be invalid)
            if record.token_count < 0 {
                return Err(ContextError::Parse(format!(
                    "Invalid negative token_count: {}",
                    record.token_count
                )));
            }
            let token_count = record.token_count as usize;

            // Convert NaiveDateTime back to DateTime<Utc>
            use chrono::{TimeZone, Utc};
            let modified_at = record.modified_at.map(|dt| Utc.from_utc_datetime(&dt));
            let fetched_at = Utc.from_utc_datetime(&record.fetched_at);

            Ok(Some(ContentItem {
                id: record.id,
                source_id: record.source_id,
                category,
                uri: record.uri,
                title: record.title,
                content: record.content,
                content_type: record.content_type,
                token_count,
                metadata_only: record.metadata_only,
                metadata,
                modified_at,
                fetched_at,
            }))
        } else {
            Ok(None)
        }
    }
}

/// Convert a vector to PostgreSQL vector string format
fn vector_to_pg_string(v: &[f32]) -> String {
    if v.is_empty() {
        return "[]".to_string();
    }

    // Pre-allocate capacity: 2 brackets + roughly 8 chars per float + commas
    let estimated_capacity = 2 + v.len() * 9;
    let mut result = String::with_capacity(estimated_capacity);
    result.push('[');

    for (i, f) in v.iter().enumerate() {
        if i > 0 {
            result.push(',');
        }
        result.push_str(&f.to_string());
    }

    result.push(']');
    result
}

#[async_trait]
impl VectorStore for PgVectorStore {
    async fn store(&self, embedding: &Embedding) -> Result<()> {
        let aligned = align_vector(&embedding.vector)?;
        let vector_str = vector_to_pg_string(&aligned);
        let created_at = embedding.created_at.naive_utc();

        sqlx::query(
            r#"
            INSERT INTO embeddings (id, chunk_id, content_item_id, source_id, workspace_id, vector, model, created_at)
            VALUES ($1, $2, $3, $4, (SELECT workspace_id FROM sources WHERE id = $4), $5::vector, $6, $7)
            ON CONFLICT (chunk_id) DO UPDATE SET
                vector = EXCLUDED.vector,
                model = EXCLUDED.model,
                workspace_id = COALESCE(embeddings.workspace_id, EXCLUDED.workspace_id)
            "#
        )
        .bind(embedding.id)
        .bind(embedding.chunk_id)
        .bind(embedding.content_item_id)
        .bind(embedding.source_id)
        .bind(&vector_str)
        .bind(&embedding.model)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn store_batch(&self, embeddings: &[Embedding]) -> Result<()> {
        let aligned: Result<Vec<Vec<f32>>> = embeddings
            .iter()
            .map(|embedding| align_vector(&embedding.vector))
            .collect();
        let aligned = aligned?;

        let mut tx = self.pool.begin().await?;

        for (embedding, vector) in embeddings.iter().zip(aligned.iter()) {
            let vector_str = vector_to_pg_string(vector);
            let created_at = embedding.created_at.naive_utc();

            sqlx::query(
                r#"
                INSERT INTO embeddings (id, chunk_id, content_item_id, source_id, workspace_id, vector, model, created_at)
                VALUES ($1, $2, $3, $4, (SELECT workspace_id FROM sources WHERE id = $4), $5::vector, $6, $7)
                ON CONFLICT (chunk_id) DO UPDATE SET
                    vector = EXCLUDED.vector,
                    model = EXCLUDED.model,
                    workspace_id = COALESCE(embeddings.workspace_id, EXCLUDED.workspace_id)
                "#
            )
            .bind(embedding.id)
            .bind(embedding.chunk_id)
            .bind(embedding.content_item_id)
            .bind(embedding.source_id)
            .bind(&vector_str)
            .bind(&embedding.model)
            .bind(created_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        threshold: Option<f32>,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<SearchResult>> {
        let aligned = align_vector(query_embedding)?;

        // Clamp limit to prevent excessive resource usage
        let limit = limit.min(MAX_SEARCH_LIMIT);

        let vector_str = vector_to_pg_string(&aligned);
        let threshold = threshold.unwrap_or(0.7);

        // Extract filter values
        let source_ids = filters.as_ref().and_then(|f| f.source_ids.as_deref());
        let workspace_id = filters.as_ref().and_then(|f| f.workspace_id);
        let categories = filters.as_ref().and_then(|f| {
            f.categories
                .as_ref()
                .map(|cats| cats.iter().map(|c| c.to_lowercase()).collect::<Vec<_>>())
        });
        let min_quality = filters.as_ref().and_then(|f| f.min_quality);
        let since = filters
            .as_ref()
            .and_then(|f| f.since.as_ref().map(|dt| dt.naive_utc()));

        #[derive(sqlx::FromRow)]
        struct SearchResultRow {
            chunk_id: Uuid,
            content_item_id: Uuid,
            source_id: Uuid,
            similarity: f64,
            chunk_text: String,
            item_uri: String,
            item_title: String,
        }

        let records: Vec<SearchResultRow> = sqlx::query_as(
            r#"
            SELECT
                e.chunk_id,
                e.content_item_id,
                e.source_id,
                (1 - (e.vector <=> $1::vector))::FLOAT as similarity,
                cc.text as chunk_text,
                ci.uri as item_uri,
                ci.title as item_title
            FROM embeddings e
            JOIN content_chunks cc ON cc.id = e.chunk_id
            JOIN content_items ci ON ci.id = e.content_item_id
            JOIN sources s ON s.id = e.source_id
            LEFT JOIN heuristic_analysis ha ON ha.content_item_id = ci.id
            WHERE (1 - (e.vector <=> $1::vector)) >= $2
                AND ($3::uuid[] IS NULL OR e.source_id = ANY($3))
                AND ($4::uuid IS NULL OR s.workspace_id = $4)
                AND ($5::text[] IS NULL OR ci.category = ANY($5))
                AND ($7::float IS NULL OR ha.quality->>'score' IS NULL OR (ha.quality->>'score')::float >= $7)
                AND ($8::timestamp IS NULL OR ci.fetched_at >= $8)
            ORDER BY e.vector <=> $1::vector
            LIMIT $6
            "#
        )
        .bind(&vector_str)
        .bind(threshold as f64)
        .bind(source_ids)
        .bind(workspace_id)
        .bind(categories.as_deref())
        .bind(limit as i64)
        .bind(min_quality.map(|q| q as f64))
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        Ok(records
            .into_iter()
            .map(|r| SearchResult {
                chunk_id: r.chunk_id,
                content_item_id: r.content_item_id,
                source_id: r.source_id,
                similarity: r.similarity as f32,
                chunk_text: r.chunk_text,
                item_uri: r.item_uri,
                item_title: r.item_title,
            })
            .collect())
    }

    async fn delete_by_content_item(&self, content_item_id: Uuid) -> Result<usize> {
        let result = sqlx::query(
            r#"
            DELETE FROM embeddings
            WHERE content_item_id = $1
            "#,
        )
        .bind(content_item_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn delete_by_source(&self, source_id: Uuid) -> Result<usize> {
        let result = sqlx::query(
            r#"
            DELETE FROM embeddings
            WHERE source_id = $1
            "#,
        )
        .bind(source_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_to_pg_string() {
        let vec = vec![0.1, 0.2, 0.3];
        let result = vector_to_pg_string(&vec);
        assert_eq!(result, "[0.1,0.2,0.3]");
    }

    #[test]
    fn test_vector_to_pg_string_empty() {
        let vec: Vec<f32> = vec![];
        let result = vector_to_pg_string(&vec);
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_vector_to_pg_string_single() {
        let vec = vec![1.5];
        let result = vector_to_pg_string(&vec);
        assert_eq!(result, "[1.5]");
    }

    #[test]
    fn test_vector_to_pg_string_negative() {
        let vec = vec![-0.5, 0.5, -1.0];
        let result = vector_to_pg_string(&vec);
        assert_eq!(result, "[-0.5,0.5,-1]");
    }

    #[test]
    fn test_align_vector_pads_shorter_models() {
        let padded = align_vector(&[0.5; 768]).expect("pad 768");
        assert_eq!(padded.len(), VECTOR_DIMENSION);
        assert_eq!(&padded[..768], &[0.5; 768]);
        assert!(padded[768..].iter().all(|value| *value == 0.0));
    }

    #[test]
    fn test_align_vector_rejects_wider_models() {
        let result = align_vector(&vec![0.1; 3072]);
        assert!(matches!(
            result,
            Err(ContextError::EmbeddingDimensionMismatch {
                expected: VECTOR_DIMENSION,
                actual: 3072
            })
        ));
    }

    // Note: Database-dependent tests are in integration tests
    // These unit tests only verify non-database logic

    #[tokio::test]
    async fn test_pgvector_dimension() {
        // Test the dimension method without requiring a database connection
        // We'll use connect_lazy which doesn't validate the connection
        use sqlx::postgres::PgPoolOptions;

        #[cfg(test)]
        const TEST_DATABASE_URL: &str = "postgres://localhost/test";

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy(TEST_DATABASE_URL)
            .expect("Should create lazy pool");

        let store = PgVectorStore::new(pool);
        assert_eq!(store.dimension(), VECTOR_DIMENSION);
        assert_eq!(store.dimension(), 1024);
    }

    #[test]
    fn test_search_filters_integration() {
        // Test SearchFilters can be constructed properly
        let filters = SearchFilters {
            source_ids: Some(vec![Uuid::new_v4()]),
            workspace_id: None,
            categories: Some(vec!["file".to_string(), "web".to_string()]),
            min_quality: Some(0.8),
            since: None,
        };

        assert!(filters.source_ids.is_some());
        assert_eq!(filters.categories.as_ref().unwrap().len(), 2);
        assert_eq!(filters.min_quality, Some(0.8));
    }
}
