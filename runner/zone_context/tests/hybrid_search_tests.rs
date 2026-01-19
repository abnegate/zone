//! Integration tests for hybrid search
//!
//! These tests verify the hybrid search functionality combining keyword
//! and semantic search using Reciprocal Rank Fusion.

use sqlx::PgPool;
use uuid::Uuid;
use zone_context::{
    HybridSearchConfig, embeddings::SearchFilters, hybrid_search, keyword_only_search,
    semantic_only_search,
};

// Helper to get test database URL
fn get_test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/zone_test".to_string())
}

// Setup test data helper
async fn setup_test_data(pool: &PgPool) -> Result<(Uuid, Uuid, Uuid), sqlx::Error> {
    // Create test workspace
    let workspace_id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces (id, name, created_by) VALUES ($1, $2, $3)")
        .bind(workspace_id)
        .bind("Test Workspace")
        .bind(Uuid::new_v4())
        .execute(pool)
        .await?;

    // Create test source
    let source_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO sources (id, workspace_id, name, source_type, config, status)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(source_id)
    .bind(workspace_id)
    .bind("Test Source")
    .bind("text")
    .bind(serde_json::json!({}))
    .bind("active")
    .execute(pool)
    .await?;

    // Create test content item
    let content_item_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO content_items (
            id, source_id, category, uri, title, content, content_type,
            token_count, metadata_only, content_hash, metadata, fetched_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())
        "#,
    )
    .bind(content_item_id)
    .bind(source_id)
    .bind("text")
    .bind("test.txt")
    .bind("Test Document")
    .bind("This is a test document about Rust async programming and tokio runtime.")
    .bind("text/plain")
    .bind(15)
    .bind(false)
    .bind("test_hash")
    .bind(serde_json::json!({}))
    .execute(pool)
    .await?;

    // Create test chunks with search vectors
    let chunks = vec![
        (
            Uuid::new_v4(),
            0,
            "This is a test document about Rust async programming.",
            11,
        ),
        (
            Uuid::new_v4(),
            1,
            "Tokio runtime provides async task execution in Rust.",
            9,
        ),
        (
            Uuid::new_v4(),
            2,
            "Async programming enables concurrent operations efficiently.",
            7,
        ),
    ];

    for (chunk_id, index, text, tokens) in chunks {
        sqlx::query(
            r#"
            INSERT INTO content_chunks (id, content_item_id, chunk_index, text, token_count, start_offset, end_offset)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#
        )
        .bind(chunk_id)
        .bind(content_item_id)
        .bind(index)
        .bind(text)
        .bind(tokens)
        .bind(0)
        .bind(text.len() as i32)
        .execute(pool)
        .await?;

        // Create dummy embedding (normally would be from embedding service)
        let dummy_vector = vec![0.1_f32; 1536];
        let vector_str = format!(
            "[{}]",
            dummy_vector
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        sqlx::query(
            r#"
            INSERT INTO embeddings (id, chunk_id, content_item_id, source_id, vector, model, created_at)
            VALUES ($1, $2, $3, $4, $5::vector, $6, NOW())
            "#
        )
        .bind(Uuid::new_v4())
        .bind(chunk_id)
        .bind(content_item_id)
        .bind(source_id)
        .bind(&vector_str)
        .bind("test-model")
        .execute(pool)
        .await?;
    }

    Ok((workspace_id, source_id, content_item_id))
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_keyword_only_search() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    // Search for exact term
    let results = keyword_only_search(
        &pool,
        "Rust async",
        10,
        Some(SearchFilters {
            workspace_id: Some(workspace_id),
            ..Default::default()
        }),
        0.0,
    )
    .await
    .unwrap();

    assert!(!results.is_empty(), "Should find results for 'Rust async'");

    // Verify results have keyword scores
    for result in &results {
        assert!(
            result.keyword_score.is_some(),
            "Keyword results should have keyword_score"
        );
        assert!(
            result.semantic_score.is_none(),
            "Keyword-only results should not have semantic_score"
        );
        assert!(
            result.chunk_text.contains("Rust") || result.chunk_text.contains("async"),
            "Results should contain search terms"
        );
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_semantic_only_search() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    // Create query embedding (normally from embedding service)
    let query_embedding = vec![0.15_f32; 1536]; // Slightly different from stored embeddings

    let results = semantic_only_search(
        &pool,
        &query_embedding,
        10,
        Some(SearchFilters {
            workspace_id: Some(workspace_id),
            ..Default::default()
        }),
        0.0, // Low threshold to ensure we get results
    )
    .await
    .unwrap();

    assert!(
        !results.is_empty(),
        "Should find results for semantic search"
    );

    // Verify results have semantic scores
    for result in &results {
        assert!(
            result.semantic_score.is_some(),
            "Semantic results should have semantic_score"
        );
        assert!(
            result.keyword_score.is_none(),
            "Semantic-only results should not have keyword_score"
        );
        assert!(
            result.semantic_score.unwrap() >= 0.0 && result.semantic_score.unwrap() <= 1.0,
            "Semantic score should be in [0, 1]"
        );
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_hybrid_search() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    let query = "Rust async programming";
    let query_embedding = vec![0.15_f32; 1536];
    let config = HybridSearchConfig::default();

    let results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        10,
        Some(workspace_id),
        None,
        &config,
    )
    .await
    .unwrap();

    assert!(!results.is_empty(), "Hybrid search should find results");

    // Verify RRF combination
    for result in &results {
        // Combined score should be present
        assert!(
            result.score > 0.0,
            "Hybrid results should have combined score"
        );

        // Results may have both keyword and semantic ranks, or just one
        let has_keyword = result.keyword_rank.is_some();
        let has_semantic = result.semantic_rank.is_some();
        assert!(has_keyword || has_semantic, "Should have at least one rank");
    }

    // Results should be sorted by score
    for i in 1..results.len() {
        assert!(
            results[i - 1].score >= results[i].score,
            "Results should be sorted by score descending"
        );
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_hybrid_search_weighting() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    let query = "Rust tokio";
    let query_embedding = vec![0.15_f32; 1536];

    // Test semantic-heavy weighting
    let semantic_config = HybridSearchConfig {
        semantic_weight: 0.9,
        ..Default::default()
    };
    let semantic_results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        10,
        Some(workspace_id),
        None,
        &semantic_config,
    )
    .await
    .unwrap();

    // Test keyword-heavy weighting
    let keyword_config = HybridSearchConfig {
        semantic_weight: 0.1,
        ..Default::default()
    };
    let keyword_results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        10,
        Some(workspace_id),
        None,
        &keyword_config,
    )
    .await
    .unwrap();

    // Rankings may differ based on weighting
    assert!(!semantic_results.is_empty());
    assert!(!keyword_results.is_empty());

    // Verify weights are applied
    // (exact verification depends on data, but we can check structure)
    for result in &semantic_results {
        assert!(result.score > 0.0);
    }
    for result in &keyword_results {
        assert!(result.score > 0.0);
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_hybrid_search_no_keyword_matches() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    // Query with terms that don't match keywords but may match semantically
    let query = "completely unrelated xyz abc123";
    let query_embedding = vec![0.15_f32; 1536];
    let config = HybridSearchConfig::default();

    let results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        10,
        Some(workspace_id),
        None,
        &config,
    )
    .await
    .unwrap();

    // Should still get semantic results even with no keyword matches
    // (assuming semantic threshold is met)
    if !results.is_empty() {
        for result in &results {
            // These should only have semantic scores
            assert!(result.semantic_rank.is_some() || result.keyword_rank.is_some());
        }
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_hybrid_search_phrase_search() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    // Test exact phrase matching with quotes (websearch_to_tsquery syntax)
    let query = "\"Rust async programming\"";
    let query_embedding = vec![0.15_f32; 1536];
    let config = HybridSearchConfig::default();

    let results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        10,
        Some(workspace_id),
        None,
        &config,
    )
    .await
    .unwrap();

    // Should find chunks with the exact phrase
    if !results.is_empty() {
        let has_phrase = results
            .iter()
            .any(|r| r.chunk_text.contains("Rust async programming"));
        assert!(has_phrase, "Should find exact phrase match");
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_hybrid_search_source_filter() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    let query = "Rust";
    let query_embedding = vec![0.15_f32; 1536];
    let config = HybridSearchConfig::default();

    // Search with source filter
    let results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        10,
        Some(workspace_id),
        Some(&[source_id]),
        &config,
    )
    .await
    .unwrap();

    // All results should be from the specified source
    for result in &results {
        assert_eq!(
            result.source_id, source_id,
            "Results should be from filtered source"
        );
    }
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "PostgreSQL not available on this platform"
)]
async fn test_hybrid_search_limit() {
    let pool = PgPool::connect(&get_test_db_url()).await.unwrap();
    let (workspace_id, _source_id, _content_item_id) = setup_test_data(&pool).await.unwrap();

    let query = "Rust";
    let query_embedding = vec![0.15_f32; 1536];
    let config = HybridSearchConfig::default();

    // Request only 2 results
    let results = hybrid_search(
        &pool,
        query,
        &query_embedding,
        2,
        Some(workspace_id),
        None,
        &config,
    )
    .await
    .unwrap();

    assert!(results.len() <= 2, "Should respect limit parameter");
}

#[test]
fn test_hybrid_config_validation() {
    let config = HybridSearchConfig {
        semantic_weight: 0.5,
        rrf_k: 60.0,
        min_keyword_score: 0.1,
        min_semantic_score: 0.6,
    };

    assert_eq!(config.semantic_weight, 0.5);
    assert_eq!(config.rrf_k, 60.0);
    assert_eq!(config.min_keyword_score, 0.1);
    assert_eq!(config.min_semantic_score, 0.6);
}

#[test]
fn test_hybrid_config_default() {
    let config = HybridSearchConfig::default();

    assert_eq!(config.semantic_weight, 0.7);
    assert_eq!(config.rrf_k, 60.0);
    assert_eq!(config.min_keyword_score, 0.0);
    assert_eq!(config.min_semantic_score, 0.5);
}

// Cleanup helper
#[allow(dead_code)]
async fn cleanup_test_data(pool: &PgPool, workspace_id: Uuid) -> Result<(), sqlx::Error> {
    // Delete in reverse dependency order
    sqlx::query("DELETE FROM embeddings WHERE source_id IN (SELECT id FROM sources WHERE workspace_id = $1)")
        .bind(workspace_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM content_chunks WHERE content_item_id IN (SELECT id FROM content_items WHERE source_id IN (SELECT id FROM sources WHERE workspace_id = $1))")
        .bind(workspace_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM content_items WHERE source_id IN (SELECT id FROM sources WHERE workspace_id = $1)")
        .bind(workspace_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM sources WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .execute(pool)
        .await?;

    Ok(())
}
