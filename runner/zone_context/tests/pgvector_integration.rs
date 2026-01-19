//! Integration tests for PgVectorStore
//!
//! These tests require a PostgreSQL database with pgvector extension.
//! Run with: cargo test --test pgvector_integration -- --ignored
//!
//! Database setup:
//! 1. Create a test database
//! 2. Run migrations from zone_server/migrations
//! 3. Set DATABASE_URL environment variable

use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
use zone_context::{
    content::{ContentCategory, ContentChunk, ContentItem},
    embeddings::{Embedding, PgVectorStore, SearchFilters, VectorStore},
};

/// Helper to get database URL from environment
fn get_test_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string())
}

/// Create a test pool
async fn create_test_pool() -> sqlx::PgPool {
    let url = get_test_database_url();
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("Failed to connect to test database. Set DATABASE_URL environment variable.")
}

/// Create a test content item
fn create_test_item(source_id: Uuid) -> ContentItem {
    ContentItem::new(
        source_id,
        ContentCategory::File,
        "/test/file.rs",
        "Test File",
    )
    .with_content("This is test content for embedding")
}

/// Create a test embedding
fn create_test_embedding(
    chunk_id: Uuid,
    content_item_id: Uuid,
    source_id: Uuid,
    dimension: usize,
) -> Embedding {
    // Create a simple normalized vector
    let value = 1.0 / (dimension as f32).sqrt();
    let vector = vec![value; dimension];
    Embedding::new(chunk_id, content_item_id, source_id, vector, "test-model")
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_store_and_search() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    // Create test data
    let source_id = Uuid::new_v4();
    let mut item = create_test_item(source_id);

    // Store content item
    let item_id = store.store_content_item(&item).await.unwrap();
    item.id = item_id;

    // Create and store chunks
    let chunk = ContentChunk::new(item_id, 0, "Test chunk content", 0, 18);
    let chunk_id = chunk.id;
    store.store_content_chunks(&[chunk]).await.unwrap();

    // Create and store embedding
    let embedding = create_test_embedding(chunk_id, item_id, source_id, 1536);
    store.store(&embedding).await.unwrap();

    // Search with the same vector (should return high similarity)
    let query = vec![1.0 / (1536_f32).sqrt(); 1536];
    let results = store.search(&query, 10, Some(0.5), None).await.unwrap();

    assert!(!results.is_empty(), "Should find at least one result");
    assert_eq!(results[0].chunk_id, chunk_id);
    assert!(
        results[0].similarity > 0.9,
        "Similarity should be very high for identical vectors"
    );

    // Clean up
    store.delete_by_source(source_id).await.unwrap();
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_search_with_filters() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    // Create two different sources
    let source_id_1 = Uuid::new_v4();
    let source_id_2 = Uuid::new_v4();

    // Create items for both sources
    let mut item1 = create_test_item(source_id_1);
    let mut item2 = ContentItem::new(
        source_id_2,
        ContentCategory::Web,
        "/test/web.html",
        "Test Web",
    )
    .with_content("Different content");

    let item_id_1 = store.store_content_item(&item1).await.unwrap();
    let item_id_2 = store.store_content_item(&item2).await.unwrap();
    item1.id = item_id_1;
    item2.id = item_id_2;

    // Create chunks
    let chunk1 = ContentChunk::new(item_id_1, 0, "Test chunk 1", 0, 12);
    let chunk2 = ContentChunk::new(item_id_2, 0, "Test chunk 2", 0, 12);
    let chunk_id_1 = chunk1.id;
    let chunk_id_2 = chunk2.id;

    store.store_content_chunks(&[chunk1, chunk2]).await.unwrap();

    // Create embeddings with slightly different vectors
    let embedding1 = create_test_embedding(chunk_id_1, item_id_1, source_id_1, 1536);
    let mut vector2 = vec![1.0 / (1536_f32).sqrt(); 1536];
    vector2[0] = -vector2[0]; // Make it slightly different
    let embedding2 = Embedding::new(chunk_id_2, item_id_2, source_id_2, vector2, "test-model");

    store.store_batch(&[embedding1, embedding2]).await.unwrap();

    // Search with source filter
    let query = vec![1.0 / (1536_f32).sqrt(); 1536];
    let filters = SearchFilters {
        source_ids: Some(vec![source_id_1]),
        workspace_id: None,
        categories: None,
        min_quality: None,
        since: None,
    };

    let results = store
        .search(&query, 10, Some(0.5), Some(filters))
        .await
        .unwrap();

    // Should only return results from source_id_1
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_id, source_id_1);

    // Search with category filter
    let filters = SearchFilters {
        source_ids: None,
        workspace_id: None,
        categories: Some(vec!["web".to_string()]),
        min_quality: None,
        since: None,
    };

    let results = store
        .search(&query, 10, Some(0.5), Some(filters))
        .await
        .unwrap();

    // Should only return web category results
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content_item_id, item_id_2);

    // Clean up
    store.delete_by_source(source_id_1).await.unwrap();
    store.delete_by_source(source_id_2).await.unwrap();
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_delete_operations() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    let source_id = Uuid::new_v4();

    // Create multiple items and embeddings
    let mut items = vec![];
    let mut chunks = vec![];
    let mut embeddings = vec![];

    for i in 0..3 {
        let mut item = ContentItem::new(
            source_id,
            ContentCategory::File,
            format!("/test/file{}.rs", i),
            format!("Test File {}", i),
        )
        .with_content(format!("Content {}", i));

        let item_id = store.store_content_item(&item).await.unwrap();
        item.id = item_id;

        let chunk = ContentChunk::new(item_id, 0, format!("Chunk {}", i), 0, 10);
        let chunk_id = chunk.id;

        store
            .store_content_chunks(std::slice::from_ref(&chunk))
            .await
            .unwrap();

        let embedding = create_test_embedding(chunk_id, item_id, source_id, 1536);
        embeddings.push(embedding.clone());

        items.push(item);
        chunks.push(chunk);
    }

    // Store all embeddings
    store.store_batch(&embeddings).await.unwrap();

    // Verify all are stored
    let query = vec![1.0 / (1536_f32).sqrt(); 1536];
    let results = store.search(&query, 10, Some(0.5), None).await.unwrap();
    assert!(results.len() >= 3, "Should have at least 3 results");

    // Delete by content item
    let deleted = store.delete_by_content_item(items[0].id).await.unwrap();
    assert_eq!(deleted, 1, "Should delete 1 embedding");

    // Verify deletion
    let results = store.search(&query, 10, Some(0.5), None).await.unwrap();
    let remaining = results
        .iter()
        .filter(|r| r.content_item_id == items[0].id)
        .count();
    assert_eq!(remaining, 0, "Item should be deleted");

    // Delete by source (should delete remaining 2)
    let deleted = store.delete_by_source(source_id).await.unwrap();
    assert_eq!(deleted, 2, "Should delete remaining 2 embeddings");

    // Verify all deleted
    let filters = SearchFilters {
        source_ids: Some(vec![source_id]),
        workspace_id: None,
        categories: None,
        min_quality: None,
        since: None,
    };
    let results = store
        .search(&query, 10, Some(0.5), Some(filters))
        .await
        .unwrap();
    assert_eq!(results.len(), 0, "All embeddings should be deleted");
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_dimension_validation() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    let source_id = Uuid::new_v4();
    let item = create_test_item(source_id);
    let item_id = store.store_content_item(&item).await.unwrap();

    let chunk = ContentChunk::new(item_id, 0, "Test chunk", 0, 10);
    let chunk_id = chunk.id;
    store.store_content_chunks(&[chunk]).await.unwrap();

    // Create embedding with wrong dimension
    let wrong_embedding = create_test_embedding(chunk_id, item_id, source_id, 768);

    // Should fail with dimension mismatch
    let result = store.store(&wrong_embedding).await;
    assert!(result.is_err(), "Should fail with wrong dimension");

    // Clean up
    store.delete_by_source(source_id).await.unwrap();
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_get_content_item() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    let source_id = Uuid::new_v4();
    let item = create_test_item(source_id);
    let item_id = store.store_content_item(&item).await.unwrap();

    // Retrieve the item
    let retrieved = store.get_content_item(item_id).await.unwrap();
    assert!(retrieved.is_some(), "Should retrieve the item");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, item_id);
    assert_eq!(retrieved.source_id, source_id);
    assert_eq!(retrieved.uri, "/test/file.rs");
    assert_eq!(retrieved.title, "Test File");

    // Try to get non-existent item
    let non_existent = store.get_content_item(Uuid::new_v4()).await.unwrap();
    assert!(
        non_existent.is_none(),
        "Should return None for non-existent item"
    );

    // Clean up
    store.delete_by_source(source_id).await.unwrap();
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_upsert_behavior() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    let source_id = Uuid::new_v4();
    let item = create_test_item(source_id);
    let item_id = store.store_content_item(&item).await.unwrap();

    let chunk = ContentChunk::new(item_id, 0, "Test chunk", 0, 10);
    let chunk_id = chunk.id;
    store.store_content_chunks(&[chunk]).await.unwrap();

    // Store initial embedding
    let embedding1 = create_test_embedding(chunk_id, item_id, source_id, 1536);
    store.store(&embedding1).await.unwrap();

    // Store updated embedding with same chunk_id (should upsert)
    let mut vector2 = vec![1.0 / (1536_f32).sqrt(); 1536];
    vector2[0] = -vector2[0];
    let embedding2 = Embedding::new(
        chunk_id,
        item_id,
        source_id,
        vector2.clone(),
        "updated-model",
    );
    store.store(&embedding2).await.unwrap();

    // Search should return the updated embedding
    let results = store.search(&vector2, 1, Some(0.5), None).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk_id, chunk_id);
    assert!(results[0].similarity > 0.9, "Should match updated vector");

    // Clean up
    store.delete_by_source(source_id).await.unwrap();
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_pgvector_batch_operations() {
    let pool = create_test_pool().await;
    let store = PgVectorStore::new(pool.clone());

    let source_id = Uuid::new_v4();

    // Create batch of items
    let mut embeddings = vec![];
    for i in 0..10 {
        let mut item = ContentItem::new(
            source_id,
            ContentCategory::File,
            format!("/test/batch{}.rs", i),
            format!("Batch {}", i),
        )
        .with_content(format!("Batch content {}", i));

        let item_id = store.store_content_item(&item).await.unwrap();
        item.id = item_id;

        let chunk = ContentChunk::new(item_id, 0, format!("Batch chunk {}", i), 0, 15);
        let chunk_id = chunk.id;
        store.store_content_chunks(&[chunk]).await.unwrap();

        let embedding = create_test_embedding(chunk_id, item_id, source_id, 1536);
        embeddings.push(embedding);
    }

    // Store all at once
    store.store_batch(&embeddings).await.unwrap();

    // Verify all were stored
    let query = vec![1.0 / (1536_f32).sqrt(); 1536];
    let filters = SearchFilters {
        source_ids: Some(vec![source_id]),
        workspace_id: None,
        categories: None,
        min_quality: None,
        since: None,
    };

    let results = store
        .search(&query, 20, Some(0.5), Some(filters))
        .await
        .unwrap();

    assert_eq!(results.len(), 10, "Should have all 10 embeddings");

    // Clean up
    let deleted = store.delete_by_source(source_id).await.unwrap();
    assert_eq!(deleted, 10, "Should delete all 10 embeddings");
}
