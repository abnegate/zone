//! Database layer for context storage
//!
//! Provides database operations for storing and querying:
//! - Content items and chunks
//! - Embeddings (via pgvector)
//! - Heuristic analysis results
//! - Knowledge base entries

use sqlx::PgPool;
use uuid::Uuid;

use crate::content::{ContentChunk, ContentItem};
use crate::embeddings::{Embedding, PgVectorStore};
use crate::error::Result;
use crate::heuristics::HeuristicAnalysis;

/// Database operations for context storage
pub struct ContextDb {
    pool: PgPool,
    vector_store: PgVectorStore,
}

impl ContextDb {
    /// Create a new database handle
    ///
    /// Note: The embedding dimension is hardcoded to 1536 to match the database schema.
    pub fn new(pool: PgPool) -> Self {
        let vector_store = PgVectorStore::new(pool.clone());
        Self { pool, vector_store }
    }

    /// Get the underlying pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get a reference to the vector store
    pub fn vector_store(&self) -> &PgVectorStore {
        &self.vector_store
    }

    // Content item operations

    /// Store a content item
    pub async fn store_content_item(&self, item: &ContentItem) -> Result<()> {
        self.vector_store.store_content_item(item).await?;
        Ok(())
    }

    /// Get a content item by ID
    pub async fn get_content_item(&self, id: Uuid) -> Result<Option<ContentItem>> {
        self.vector_store.get_content_item(id).await
    }

    /// Delete content items by source ID
    pub async fn delete_content_items_by_source(&self, source_id: Uuid) -> Result<usize> {
        let result = sqlx::query(
            r#"
            DELETE FROM content_items
            WHERE source_id = $1
            "#,
        )
        .bind(source_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    // Content chunk operations

    /// Store content chunks
    pub async fn store_chunks(&self, chunks: &[ContentChunk]) -> Result<()> {
        self.vector_store.store_content_chunks(chunks).await
    }

    /// Get chunks for a content item
    pub async fn get_chunks_for_item(&self, item_id: Uuid) -> Result<Vec<ContentChunk>> {
        #[derive(sqlx::FromRow)]
        struct ChunkRow {
            id: Uuid,
            content_item_id: Uuid,
            chunk_index: i32,
            text: String,
            token_count: i32,
            start_offset: i32,
            end_offset: i32,
        }

        let records: Vec<ChunkRow> = sqlx::query_as(
            r#"
            SELECT id, content_item_id, chunk_index, text, token_count, start_offset, end_offset
            FROM content_chunks
            WHERE content_item_id = $1
            ORDER BY chunk_index
            "#,
        )
        .bind(item_id)
        .fetch_all(&self.pool)
        .await?;

        records
            .into_iter()
            .map(|r| {
                // Validate all i32 to usize conversions (negative values would be invalid)
                if r.chunk_index < 0 {
                    return Err(crate::error::ContextError::Parse(format!(
                        "Invalid negative chunk_index: {}",
                        r.chunk_index
                    )));
                }
                if r.token_count < 0 {
                    return Err(crate::error::ContextError::Parse(format!(
                        "Invalid negative token_count: {}",
                        r.token_count
                    )));
                }
                if r.start_offset < 0 {
                    return Err(crate::error::ContextError::Parse(format!(
                        "Invalid negative start_offset: {}",
                        r.start_offset
                    )));
                }
                if r.end_offset < 0 {
                    return Err(crate::error::ContextError::Parse(format!(
                        "Invalid negative end_offset: {}",
                        r.end_offset
                    )));
                }

                Ok(ContentChunk {
                    id: r.id,
                    content_item_id: r.content_item_id,
                    chunk_index: r.chunk_index as usize,
                    text: r.text,
                    token_count: r.token_count as usize,
                    start_offset: r.start_offset as usize,
                    end_offset: r.end_offset as usize,
                })
            })
            .collect()
    }

    // Embedding operations

    /// Store an embedding
    pub async fn store_embedding(&self, embedding: &Embedding) -> Result<()> {
        use crate::embeddings::VectorStore;
        self.vector_store.store(embedding).await
    }

    /// Store embeddings in batch
    pub async fn store_embeddings_batch(&self, embeddings: &[Embedding]) -> Result<()> {
        use crate::embeddings::VectorStore;
        self.vector_store.store_batch(embeddings).await
    }

    /// Delete embeddings by source ID
    pub async fn delete_embeddings_by_source(&self, source_id: Uuid) -> Result<usize> {
        use crate::embeddings::VectorStore;
        self.vector_store.delete_by_source(source_id).await
    }

    // Analysis operations

    /// Store heuristic analysis
    pub async fn store_analysis(&self, _analysis: &HeuristicAnalysis) -> Result<()> {
        // Will be implemented with migration
        Ok(())
    }

    /// Get analysis for a content item
    pub async fn get_analysis(&self, _item_id: Uuid) -> Result<Option<HeuristicAnalysis>> {
        // Will be implemented with migration
        Ok(None)
    }
}

/// Knowledge base entry
#[derive(Debug, Clone)]
pub struct KnowledgeEntry {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub token_count: usize,
    pub is_active: bool,
}

impl KnowledgeEntry {
    /// Create a new knowledge entry
    pub fn new(workspace_id: Uuid, title: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        let token_count = crate::content::estimate_tokens(&content);
        Self {
            id: Uuid::new_v4(),
            workspace_id,
            title: title.into(),
            content,
            category: None,
            tags: Vec::new(),
            token_count,
            is_active: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knowledge_entry_new() {
        let workspace_id = Uuid::new_v4();
        let entry = KnowledgeEntry::new(workspace_id, "Test Title", "Some content here");

        assert_eq!(entry.workspace_id, workspace_id);
        assert_eq!(entry.title, "Test Title");
        assert_eq!(entry.content, "Some content here");
        assert!(entry.is_active);
        assert!(entry.token_count > 0);
    }
}
