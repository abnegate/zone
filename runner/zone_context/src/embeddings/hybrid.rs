//! Hybrid search combining keyword and semantic retrieval
//!
//! Uses Reciprocal Rank Fusion (RRF) to combine results from:
//! - PostgreSQL full-text search (keyword matching with BM25-like ranking)
//! - pgvector similarity search (semantic matching)
//!
//! # Algorithm
//!
//! 1. Perform keyword search using PostgreSQL full-text search (ts_rank)
//! 2. Perform semantic search using pgvector cosine similarity
//! 3. Combine using Reciprocal Rank Fusion: score = sum(1/(k + rank))
//! 4. Apply configurable weights to favor semantic or keyword results
//!
//! # Example
//!
//! ```rust,ignore
//! let config = HybridSearchConfig::default();
//! let results = hybrid_search(
//!     &pool,
//!     "Rust async programming",
//!     &query_embedding,
//!     10,
//!     Some(workspace_id),
//!     None,
//!     &config,
//! ).await?;
//! ```

use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use crate::embeddings::SearchFilters;

/// Configuration for hybrid search
#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    /// Weight for semantic results (0.0-1.0), keyword weight = 1 - semantic_weight
    pub semantic_weight: f32,
    /// RRF constant (typically 60, from literature)
    pub rrf_k: f32,
    /// Minimum keyword match score (0.0-1.0)
    pub min_keyword_score: f32,
    /// Minimum semantic similarity (0.0-1.0)
    pub min_semantic_score: f32,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            semantic_weight: 0.7, // Favor semantic by default
            rrf_k: 60.0,          // Standard RRF constant
            min_keyword_score: 0.0,
            min_semantic_score: 0.35,
        }
    }
}

impl HybridSearchConfig {
    /// Balance keyword and semantic legs when the query names code symbols.
    pub fn for_query(query: &str) -> Self {
        let mut config = Self::default();
        if !crate::embeddings::rewrite_query(query)
            .identifiers
            .is_empty()
        {
            config.semantic_weight = 0.45;
        }
        config
    }
}

/// Result from hybrid search
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    pub chunk_id: Uuid,
    pub content_item_id: Uuid,
    pub source_id: Uuid,
    pub chunk_text: String,
    pub item_uri: String,
    pub item_title: String,
    /// Final ranking score (learned ranker + cross-encoder after fusion).
    pub score: f32,
    /// Raw RRF / first-stage fusion score, kept for honest API reporting.
    pub fusion_score: f32,
    /// Keyword rank (None if not in keyword results)
    pub keyword_rank: Option<usize>,
    /// Semantic rank (None if not in semantic results)
    pub semantic_rank: Option<usize>,
    /// Keyword match score (ts_rank)
    pub keyword_score: Option<f32>,
    /// Semantic similarity score
    pub semantic_score: Option<f32>,
}

/// Internal result from keyword search
#[derive(Debug, Clone, sqlx::FromRow)]
struct KeywordResult {
    chunk_id: Uuid,
    content_item_id: Uuid,
    source_id: Uuid,
    chunk_text: String,
    item_uri: String,
    item_title: String,
    score: f32,
}

/// Internal result from semantic search
#[derive(Debug, Clone, sqlx::FromRow)]
struct SemanticResult {
    chunk_id: Uuid,
    content_item_id: Uuid,
    source_id: Uuid,
    chunk_text: String,
    item_uri: String,
    item_title: String,
    similarity: f32,
}

/// Intermediate structure for RRF calculation
#[derive(Debug, Clone, Default)]
struct RRFScore {
    chunk_id: Uuid,
    content_item_id: Uuid,
    source_id: Uuid,
    chunk_text: String,
    item_uri: String,
    item_title: String,
    keyword_rank: Option<usize>,
    semantic_rank: Option<usize>,
    keyword_score: Option<f32>,
    semantic_score: Option<f32>,
    keyword_rrf: f32,
    semantic_rrf: f32,
}

/// Perform hybrid search combining keyword and semantic retrieval
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `query` - Text query for keyword search
/// * `query_embedding` - Vector embedding for semantic search
/// * `limit` - Maximum number of results to return
/// * `workspace_id` - Optional workspace filter
/// * `source_ids` - Optional source IDs filter
/// * `config` - Hybrid search configuration
///
/// # Returns
/// Combined and ranked search results using RRF
pub async fn hybrid_search(
    pool: &PgPool,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
    workspace_id: Option<Uuid>,
    source_ids: Option<&[Uuid]>,
    config: &HybridSearchConfig,
) -> sqlx::Result<Vec<HybridSearchResult>> {
    let filters = SearchFilters {
        workspace_id,
        source_ids: source_ids.map(|ids| ids.to_vec()),
        ..Default::default()
    };

    hybrid_search_filtered(pool, query, query_embedding, limit, Some(&filters), config).await
}

/// Hybrid search that honors the same filters as semantic `VectorStore::search`.
pub async fn hybrid_search_filtered(
    pool: &PgPool,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
    filters: Option<&SearchFilters>,
    config: &HybridSearchConfig,
) -> sqlx::Result<Vec<HybridSearchResult>> {
    let fetch_limit = (limit * 3).max(24);
    let rewritten = crate::embeddings::rewrite_query(query);
    let extra = crate::embeddings::serving::has_extra_filters(
        filters.and_then(|f| f.source_ids.as_ref()).is_some(),
        filters.and_then(|f| f.categories.as_ref()).is_some(),
        filters.and_then(|f| f.min_quality).is_some(),
        filters.and_then(|f| f.since).is_some(),
    );
    let keyword_fetch = crate::embeddings::serving::keyword_candidate_limit(fetch_limit, extra);
    let ann_fetch = crate::embeddings::serving::ann_candidate_limit(fetch_limit, extra);

    let keyword_results = keyword_search(
        pool,
        &rewritten.keyword,
        keyword_fetch,
        fetch_limit,
        filters,
        config.min_keyword_score,
    )
    .await?;

    let query_embedding = crate::embeddings::align_vector(query_embedding)
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

    let semantic_results = semantic_search(
        pool,
        &query_embedding,
        ann_fetch,
        fetch_limit,
        filters,
        config.min_semantic_score,
    )
    .await?;

    let combined = reciprocal_rank_fusion(keyword_results, semantic_results, config);
    Ok(finalize_ranking(combined, query, limit))
}

/// Keyword search using PostgreSQL full-text search
///
/// Uses `websearch_to_tsquery` which supports Google-like syntax:
/// - "exact phrase" for phrase matching
/// - -word for exclusion
/// - word1 OR word2 for alternatives
async fn keyword_search(
    pool: &PgPool,
    query: &str,
    candidate_limit: usize,
    limit: usize,
    filters: Option<&SearchFilters>,
    min_score: f32,
) -> sqlx::Result<Vec<KeywordResult>> {
    let sanitized_query = crate::embeddings::sanitize_search_query(query);
    let workspace_id = filters.and_then(|f| f.workspace_id);
    let source_ids = filters.and_then(|f| f.source_ids.as_deref());
    let categories = filters.and_then(|f| {
        f.categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.to_lowercase()).collect::<Vec<_>>())
    });
    let min_quality = filters.and_then(|f| f.min_quality);
    let since = filters.and_then(|f| f.since.map(|dt| dt.naive_utc()));

    sqlx::query_as(
        r#"
        WITH q AS (
            SELECT websearch_to_tsquery('english', $1) AS tsq
        ),
        hits AS (
            SELECT
                cc.id AS chunk_id,
                cc.content_item_id,
                cc.text AS chunk_text,
                ts_rank_cd(cc.search_vector, q.tsq) AS score
            FROM content_chunks cc
            JOIN content_items ci ON ci.id = cc.content_item_id
            CROSS JOIN q
            WHERE cc.search_vector @@ q.tsq
                AND ($3::uuid IS NULL OR ci.workspace_id = $3)
                AND ($4::uuid[] IS NULL OR ci.source_id = ANY($4))
            ORDER BY score DESC
            LIMIT $5
        )
        SELECT
            hits.chunk_id,
            hits.content_item_id,
            ci.source_id,
            hits.chunk_text,
            ci.uri AS item_uri,
            ci.title AS item_title,
            hits.score
        FROM hits
        JOIN content_items ci ON ci.id = hits.content_item_id
        LEFT JOIN heuristic_analysis ha ON ha.content_item_id = ci.id
        WHERE hits.score >= $2
            AND ($6::text[] IS NULL OR ci.category = ANY($6))
            AND ($7::float IS NULL OR ha.quality->>'score' IS NULL OR (ha.quality->>'score')::float >= $7)
            AND ($8::timestamp IS NULL OR ci.fetched_at >= $8)
        ORDER BY hits.score DESC
        LIMIT $9
        "#,
    )
    .bind(&sanitized_query)
    .bind(min_score)
    .bind(workspace_id)
    .bind(source_ids)
    .bind(candidate_limit as i64)
    .bind(categories.as_deref())
    .bind(min_quality.map(|q| q as f64))
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

/// Semantic search using pgvector cosine similarity
async fn semantic_search(
    pool: &PgPool,
    query_embedding: &[f32],
    candidate_limit: usize,
    limit: usize,
    filters: Option<&SearchFilters>,
    min_similarity: f32,
) -> sqlx::Result<Vec<SemanticResult>> {
    let vector_str = vector_to_pg_string(query_embedding)
        .map_err(|e| sqlx::Error::Protocol(format!("Invalid embedding vector: {}", e)))?;
    let workspace_id = filters.and_then(|f| f.workspace_id);
    let source_ids = filters.and_then(|f| f.source_ids.as_deref());
    let categories = filters.and_then(|f| {
        f.categories
            .as_ref()
            .map(|cats| cats.iter().map(|c| c.to_lowercase()).collect::<Vec<_>>())
    });
    let min_quality = filters.and_then(|f| f.min_quality);
    let since = filters.and_then(|f| f.since.map(|dt| dt.naive_utc()));

    sqlx::query_as(
        r#"
        WITH ann AS (
            SELECT e.chunk_id, e.content_item_id, e.source_id, e.vector
            FROM embeddings e
            WHERE ($3::uuid IS NULL OR e.workspace_id = $3)
                AND ($4::uuid[] IS NULL OR e.source_id = ANY($4))
            ORDER BY e.vector_bit <~> binary_quantize($1::vector)::bit(1024)
            LIMIT $5
        )
        SELECT
            ann.chunk_id,
            ann.content_item_id,
            ann.source_id,
            cc.text as chunk_text,
            ci.uri as item_uri,
            ci.title as item_title,
            (1 - (ann.vector <=> $1::vector))::REAL as similarity
        FROM ann
        JOIN content_chunks cc ON cc.id = ann.chunk_id
        JOIN content_items ci ON ci.id = ann.content_item_id
        LEFT JOIN heuristic_analysis ha ON ha.content_item_id = ci.id
        WHERE (1 - (ann.vector <=> $1::vector)) >= $2
            AND ($6::text[] IS NULL OR ci.category = ANY($6))
            AND ($7::float IS NULL OR ha.quality->>'score' IS NULL OR (ha.quality->>'score')::float >= $7)
            AND ($8::timestamp IS NULL OR ci.fetched_at >= $8)
        ORDER BY ann.vector <=> $1::vector
        LIMIT $9
        "#,
    )
    .bind(&vector_str)
    .bind(min_similarity)
    .bind(workspace_id)
    .bind(source_ids)
    .bind(candidate_limit as i64)
    .bind(categories.as_deref())
    .bind(min_quality.map(|q| q as f64))
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

/// Reciprocal Rank Fusion (RRF) algorithm
///
/// Combines multiple ranked lists into a single ranking.
/// Formula: RRF_score(d) = sum_over_rankings( 1 / (k + rank(d)) )
///
/// This is position-based (not score-based), which handles different
/// score scales gracefully without normalization.
///
/// # Arguments
/// * `keyword_results` - Results from keyword search (ordered by relevance)
/// * `semantic_results` - Results from semantic search (ordered by similarity)
/// * `config` - Contains RRF constant k and semantic weight
///
/// # Returns
/// Combined results sorted by weighted RRF score
fn reciprocal_rank_fusion(
    keyword_results: Vec<KeywordResult>,
    semantic_results: Vec<SemanticResult>,
    config: &HybridSearchConfig,
) -> Vec<HybridSearchResult> {
    let mut scores: HashMap<Uuid, RRFScore> = HashMap::new();

    // Add keyword results (rank starts at 1)
    for (rank, result) in keyword_results.iter().enumerate() {
        let entry = scores.entry(result.chunk_id).or_insert_with(|| RRFScore {
            chunk_id: result.chunk_id,
            content_item_id: result.content_item_id,
            source_id: result.source_id,
            chunk_text: result.chunk_text.clone(),
            item_uri: result.item_uri.clone(),
            item_title: result.item_title.clone(),
            ..Default::default()
        });
        entry.keyword_rank = Some(rank + 1);
        entry.keyword_score = Some(result.score);
        entry.keyword_rrf = 1.0 / (config.rrf_k + (rank + 1) as f32);
    }

    // Add semantic results (rank starts at 1)
    for (rank, result) in semantic_results.iter().enumerate() {
        let entry = scores.entry(result.chunk_id).or_insert_with(|| RRFScore {
            chunk_id: result.chunk_id,
            content_item_id: result.content_item_id,
            source_id: result.source_id,
            chunk_text: result.chunk_text.clone(),
            item_uri: result.item_uri.clone(),
            item_title: result.item_title.clone(),
            ..Default::default()
        });
        entry.semantic_rank = Some(rank + 1);
        entry.semantic_score = Some(result.similarity);
        entry.semantic_rrf = 1.0 / (config.rrf_k + (rank + 1) as f32);
    }

    // Calculate final scores with weighting and sort
    let mut results: Vec<HybridSearchResult> = scores
        .into_values()
        .map(|s| {
            // Weighted combination: semantic_weight * semantic_rrf + (1 - semantic_weight) * keyword_rrf
            let final_score = config.semantic_weight * s.semantic_rrf
                + (1.0 - config.semantic_weight) * s.keyword_rrf;

            HybridSearchResult {
                chunk_id: s.chunk_id,
                content_item_id: s.content_item_id,
                source_id: s.source_id,
                chunk_text: s.chunk_text,
                item_uri: s.item_uri,
                item_title: s.item_title,
                score: final_score,
                fusion_score: final_score,
                keyword_rank: s.keyword_rank,
                semantic_rank: s.semantic_rank,
                keyword_score: s.keyword_score,
                semantic_score: s.semantic_score,
            }
        })
        .collect();

    // Sort by final RRF score (descending) with proper NaN handling
    results.sort_by(|a, b| {
        match (a.score.is_nan(), b.score.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater, // NaN goes last
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => b
                .score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        }
    });
    results
}

/// Learned ranker + lexical cross-encoder, then at most two chunks per file.
pub fn finalize_ranking(
    results: Vec<HybridSearchResult>,
    query: &str,
    limit: usize,
) -> Vec<HybridSearchResult> {
    cap_per_file(apply_local_rerank(query, results), 2, limit)
}

pub fn apply_local_rerank(
    query: &str,
    mut results: Vec<HybridSearchResult>,
) -> Vec<HybridSearchResult> {
    for result in &mut results {
        if result.fusion_score == 0.0 {
            result.fusion_score = result.score;
        }
        result.score = crate::embeddings::ranker::score_hit(
            query,
            &result.item_uri,
            &result.item_title,
            &result.chunk_text,
            result.semantic_score,
            result.keyword_score,
            result.keyword_rank,
            result.semantic_rank,
            result.fusion_score,
        );
    }
    sort_by_score(&mut results);
    results
}

pub fn cap_per_file(
    results: Vec<HybridSearchResult>,
    max_per_file: usize,
    limit: usize,
) -> Vec<HybridSearchResult> {
    let mut per_item = HashMap::<Uuid, usize>::new();
    results
        .into_iter()
        .filter(|result| {
            let seen = per_item.entry(result.content_item_id).or_insert(0);
            *seen += 1;
            *seen <= max_per_file
        })
        .take(limit)
        .collect()
}

pub fn sort_by_score(results: &mut [HybridSearchResult]) {
    results.sort_by(|a, b| match (a.score.is_nan(), b.score.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => b
            .score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal),
    });
}

/// Convert a vector to PostgreSQL vector string format
///
/// Returns an error if any vector values are non-finite (NaN or Infinity).
fn vector_to_pg_string(v: &[f32]) -> Result<String, &'static str> {
    if v.is_empty() {
        return Ok("[]".to_string());
    }

    // Validate all floats are finite
    for f in v {
        if !f.is_finite() {
            tracing::error!("Vector contains non-finite values (NaN or Infinity)");
            return Err("Vector contains non-finite values (NaN or Infinity)");
        }
    }

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
    Ok(result)
}

/// Keyword-only search (no semantic component)
pub async fn keyword_only_search(
    pool: &PgPool,
    query: &str,
    limit: usize,
    filters: Option<SearchFilters>,
    min_score: f32,
) -> sqlx::Result<Vec<HybridSearchResult>> {
    let rewritten = crate::embeddings::rewrite_query(query);
    let extra = crate::embeddings::serving::has_extra_filters(
        filters
            .as_ref()
            .and_then(|f| f.source_ids.as_ref())
            .is_some(),
        filters
            .as_ref()
            .and_then(|f| f.categories.as_ref())
            .is_some(),
        filters.as_ref().and_then(|f| f.min_quality).is_some(),
        filters.as_ref().and_then(|f| f.since).is_some(),
    );
    let results = keyword_search(
        pool,
        &rewritten.keyword,
        crate::embeddings::serving::keyword_candidate_limit(limit, extra),
        limit,
        filters.as_ref(),
        min_score,
    )
    .await?;
    let mapped = results
        .into_iter()
        .enumerate()
        .map(|(rank, r)| HybridSearchResult {
            chunk_id: r.chunk_id,
            content_item_id: r.content_item_id,
            source_id: r.source_id,
            chunk_text: r.chunk_text,
            item_uri: r.item_uri,
            item_title: r.item_title,
            score: r.score,
            fusion_score: r.score,
            keyword_rank: Some(rank + 1),
            semantic_rank: None,
            keyword_score: Some(r.score),
            semantic_score: None,
        })
        .collect();
    Ok(finalize_ranking(mapped, query, limit))
}

/// Semantic-only search (no keyword component)
pub async fn semantic_only_search(
    pool: &PgPool,
    query_embedding: &[f32],
    limit: usize,
    filters: Option<SearchFilters>,
    min_similarity: f32,
) -> sqlx::Result<Vec<HybridSearchResult>> {
    let query_embedding = crate::embeddings::align_vector(query_embedding)
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

    let extra = crate::embeddings::serving::has_extra_filters(
        filters
            .as_ref()
            .and_then(|f| f.source_ids.as_ref())
            .is_some(),
        filters
            .as_ref()
            .and_then(|f| f.categories.as_ref())
            .is_some(),
        filters.as_ref().and_then(|f| f.min_quality).is_some(),
        filters.as_ref().and_then(|f| f.since).is_some(),
    );
    let results = semantic_search(
        pool,
        &query_embedding,
        crate::embeddings::serving::ann_candidate_limit(limit, extra),
        limit,
        filters.as_ref(),
        min_similarity,
    )
    .await?;

    Ok(results
        .into_iter()
        .enumerate()
        .map(|(rank, r)| HybridSearchResult {
            chunk_id: r.chunk_id,
            content_item_id: r.content_item_id,
            source_id: r.source_id,
            chunk_text: r.chunk_text,
            item_uri: r.item_uri,
            item_title: r.item_title,
            score: r.similarity,
            fusion_score: r.similarity,
            keyword_rank: None,
            semantic_rank: Some(rank + 1),
            keyword_score: None,
            semantic_score: Some(r.similarity),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_config_default() {
        let config = HybridSearchConfig::default();
        assert_eq!(config.semantic_weight, 0.7);
        assert_eq!(config.rrf_k, 60.0);
        assert_eq!(config.min_keyword_score, 0.0);
        assert_eq!(config.min_semantic_score, 0.35);
    }

    #[test]
    fn test_rrf_calculation() {
        let config = HybridSearchConfig::default();

        // Create mock results
        let keyword_results = vec![
            KeywordResult {
                chunk_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
                content_item_id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                chunk_text: "Result 1".to_string(),
                item_uri: "uri1".to_string(),
                item_title: "Title 1".to_string(),
                score: 0.9,
            },
            KeywordResult {
                chunk_id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
                content_item_id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                chunk_text: "Result 2".to_string(),
                item_uri: "uri2".to_string(),
                item_title: "Title 2".to_string(),
                score: 0.8,
            },
        ];

        let semantic_results = vec![
            SemanticResult {
                chunk_id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
                content_item_id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                chunk_text: "Result 2".to_string(),
                item_uri: "uri2".to_string(),
                item_title: "Title 2".to_string(),
                similarity: 0.95,
            },
            SemanticResult {
                chunk_id: Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
                content_item_id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                chunk_text: "Result 3".to_string(),
                item_uri: "uri3".to_string(),
                item_title: "Title 3".to_string(),
                similarity: 0.85,
            },
        ];

        let results = reciprocal_rank_fusion(keyword_results, semantic_results, &config);

        // Result 2 appears in both lists, should have highest combined score
        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0].chunk_id.to_string(),
            "00000000-0000-0000-0000-000000000002"
        );
        assert!(results[0].keyword_rank.is_some());
        assert!(results[0].semantic_rank.is_some());

        // Result 1 only in keyword list
        let result_1 = results
            .iter()
            .find(|r| r.chunk_id.to_string() == "00000000-0000-0000-0000-000000000001")
            .unwrap();
        assert!(result_1.keyword_rank.is_some());
        assert!(result_1.semantic_rank.is_none());

        // Result 3 only in semantic list
        let result_3 = results
            .iter()
            .find(|r| r.chunk_id.to_string() == "00000000-0000-0000-0000-000000000003")
            .unwrap();
        assert!(result_3.keyword_rank.is_none());
        assert!(result_3.semantic_rank.is_some());
    }

    #[test]
    fn identifier_hits_outrank_semantic_neighbors() {
        let config = HybridSearchConfig::for_query("What does should_skip_blob do?");
        assert_eq!(config.semantic_weight, 0.45);

        let keyword_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let semantic_id = Uuid::parse_str("00000000-0000-0000-0000-000000000099").unwrap();
        let keyword_results = vec![KeywordResult {
            chunk_id: keyword_id,
            content_item_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            chunk_text: "fn should_skip_blob(uri: &str, sha: &str) -> bool".to_string(),
            item_uri: "github://zone/content/mod.rs".to_string(),
            item_title: "mod.rs".to_string(),
            score: 0.07,
        }];
        let semantic_results = vec![SemanticResult {
            chunk_id: semantic_id,
            content_item_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            chunk_text: "generic authentication helper".to_string(),
            item_uri: "github://zone/auth.ts".to_string(),
            item_title: "auth.ts".to_string(),
            similarity: 0.51,
        }];
        let fused = reciprocal_rank_fusion(keyword_results, semantic_results, &config);
        let ranked = finalize_ranking(fused, "What does should_skip_blob do?", 5);
        assert_eq!(ranked[0].chunk_id, keyword_id);
    }

    #[test]
    fn finalize_ranking_caps_chunks_per_file() {
        let item = Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap();
        let results = (0..4)
            .map(|i| HybridSearchResult {
                chunk_id: Uuid::from_u128(i as u128 + 1),
                content_item_id: item,
                source_id: Uuid::new_v4(),
                chunk_text: format!("fn should_skip_blob chunk {i}"),
                item_uri: "github://zone/content/mod.rs".to_string(),
                item_title: "mod.rs".to_string(),
                score: 1.0 - i as f32 * 0.01,
                fusion_score: 1.0 - i as f32 * 0.01,
                keyword_rank: Some(i + 1),
                semantic_rank: None,
                keyword_score: Some(0.1),
                semantic_score: None,
            })
            .collect();
        let ranked = finalize_ranking(results, "should_skip_blob", 10);
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn test_vector_to_pg_string() {
        assert_eq!(vector_to_pg_string(&[]).unwrap(), "[]");
        assert_eq!(vector_to_pg_string(&[1.5]).unwrap(), "[1.5]");
        assert_eq!(
            vector_to_pg_string(&[0.1, 0.2, 0.3]).unwrap(),
            "[0.1,0.2,0.3]"
        );
        assert_eq!(
            vector_to_pg_string(&[-0.5, 0.5, -1.0]).unwrap(),
            "[-0.5,0.5,-1]"
        );

        // Test NaN rejection
        assert!(vector_to_pg_string(&[f32::NAN]).is_err());
        assert!(vector_to_pg_string(&[f32::INFINITY]).is_err());
        assert!(vector_to_pg_string(&[f32::NEG_INFINITY]).is_err());
        assert!(vector_to_pg_string(&[1.0, f32::NAN, 2.0]).is_err());
    }
}
