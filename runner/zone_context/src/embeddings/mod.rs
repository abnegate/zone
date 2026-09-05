//! Embedding generation and vector storage
//!
//! This module provides the `EmbeddingService` trait for generating embeddings
//! and `VectorStore` for storing and searching embeddings in PostgreSQL with pgvector.

pub mod eval;
pub mod hybrid;
pub mod pgvector;
pub mod providers;
pub mod query;

pub use hybrid::{
    HybridSearchConfig, HybridSearchResult, hybrid_search, keyword_only_search,
    semantic_only_search,
};
pub use pgvector::{PgVectorStore, VECTOR_DIMENSION, align_vector};
pub use query::{RewrittenQuery, embed_query_text, rewrite_query, sanitize_search_query};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

/// An embedding vector with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    /// Unique identifier
    pub id: Uuid,
    /// ID of the content chunk this embedding represents
    pub chunk_id: Uuid,
    /// ID of the parent content item
    pub content_item_id: Uuid,
    /// Source ID
    pub source_id: Uuid,
    /// The embedding vector
    pub vector: Vec<f32>,
    /// Dimension of the vector
    pub dimension: usize,
    /// Model used to generate this embedding
    pub model: String,
    /// When this embedding was generated
    pub created_at: DateTime<Utc>,
}

impl Embedding {
    /// Create a new embedding
    pub fn new(
        chunk_id: Uuid,
        content_item_id: Uuid,
        source_id: Uuid,
        vector: Vec<f32>,
        model: impl Into<String>,
    ) -> Self {
        let dimension = vector.len();
        Self {
            id: Uuid::new_v4(),
            chunk_id,
            content_item_id,
            source_id,
            vector,
            dimension,
            model: model.into(),
            created_at: Utc::now(),
        }
    }
}

/// Service for generating embeddings
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Generate embeddings for multiple texts (batch)
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// Get the dimension of embeddings from this service
    fn dimension(&self) -> usize;

    /// Get the model name
    fn model(&self) -> &str;

    /// Maximum tokens per embedding request
    fn max_tokens(&self) -> usize {
        8192
    }
}

/// Search filters for vector queries
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Filter by source IDs
    pub source_ids: Option<Vec<Uuid>>,
    /// Filter by workspace ID
    pub workspace_id: Option<Uuid>,
    /// Filter by content categories
    pub categories: Option<Vec<String>>,
    /// Minimum quality score
    pub min_quality: Option<f32>,
    /// Filter to content modified after this time
    pub since: Option<DateTime<Utc>>,
}

/// Result of a vector similarity search
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Chunk ID
    pub chunk_id: Uuid,
    /// Content item ID
    pub content_item_id: Uuid,
    /// Source ID
    pub source_id: Uuid,
    /// Similarity score (0.0 - 1.0)
    pub similarity: f32,
    /// The chunk text
    pub chunk_text: String,
    /// Content item URI
    pub item_uri: String,
    /// Content item title
    pub item_title: String,
}

/// Vector store for embedding storage and search
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Store an embedding
    async fn store(&self, embedding: &Embedding) -> Result<()>;

    /// Store multiple embeddings in batch
    async fn store_batch(&self, embeddings: &[Embedding]) -> Result<()>;

    /// Search for similar embeddings
    async fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        threshold: Option<f32>,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<SearchResult>>;

    /// Delete embeddings by content item ID
    async fn delete_by_content_item(&self, content_item_id: Uuid) -> Result<usize>;

    /// Delete embeddings by source ID
    async fn delete_by_source(&self, source_id: Uuid) -> Result<usize>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_new() {
        let chunk_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let source_id = Uuid::new_v4();
        let vector = vec![0.1, 0.2, 0.3, 0.4];

        let embedding = Embedding::new(chunk_id, item_id, source_id, vector.clone(), "test-model");

        assert_eq!(embedding.chunk_id, chunk_id);
        assert_eq!(embedding.content_item_id, item_id);
        assert_eq!(embedding.source_id, source_id);
        assert_eq!(embedding.vector, vector);
        assert_eq!(embedding.dimension, 4);
        assert_eq!(embedding.model, "test-model");
    }

    #[test]
    fn test_search_filters_default() {
        let filters = SearchFilters::default();
        assert!(filters.source_ids.is_none());
        assert!(filters.categories.is_none());
        assert!(filters.min_quality.is_none());
        assert!(filters.since.is_none());
    }
}
