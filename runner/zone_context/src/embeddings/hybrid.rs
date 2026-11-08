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

mod id;
mod keyword_result;
mod semantic_result;

use std::collections::HashSet;

use abnegate_index::HybridSearchConfig;
use abnegate_index::HybridSearchResult;
use abnegate_index::finalize_ranking;
use abnegate_index::first_stage_answered;
use abnegate_index::identifier_file_patterns;
use abnegate_index::keep_answer_chunks;
use abnegate_index::path_uri_patterns;
use abnegate_index::reciprocal_rank_fusion;
use abnegate_index::rewrite_query;
use abnegate_index::same_file_patterns;
use abnegate_index::sanitize_search_query;
use abnegate_index::uri_stem_matches_identifier;
use sqlx::PgPool;
use uuid::Uuid;

use crate::embeddings::SearchFilters;
use crate::embeddings::align_vector;
use crate::embeddings::ranker::default_ranker;
use crate::embeddings::serving::ann_candidate_limit;
use crate::embeddings::serving::has_extra_filters;
use crate::embeddings::serving::keyword_candidate_limit;

pub use id::Id;
use keyword_result::KeywordResult;
use semantic_result::SemanticResult;

pub(crate) const CANDIDATE_POOL_FACTOR: usize = 4;

pub(crate) const MINIMUM_CANDIDATE_POOL: usize = 32;

const LEG_OVERFETCH_FACTOR: usize = 2;

const MAXIMUM_KEYWORD_CANDIDATES: usize = 128;

const RETRIEVED_FILES_TO_EXPAND: usize = 16;

const MAXIMUM_FILES_TO_EXPAND: usize = 20;

const PATH_MATCHED_FILES: usize = 6;

const SAME_FILE_CHUNKS_PER_FILE: usize = 28;

const VECTOR_BRACKETS_LENGTH: usize = 2;

const ESTIMATED_VECTOR_VALUE_LENGTH: usize = 9;

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
) -> sqlx::Result<Vec<HybridSearchResult<Id>>> {
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
) -> sqlx::Result<Vec<HybridSearchResult<Id>>> {
    let fetch_limit = (limit * CANDIDATE_POOL_FACTOR).max(MINIMUM_CANDIDATE_POOL);
    let rewritten = rewrite_query(query);
    let extra = has_extra_filters(
        filters
            .and_then(|filter| filter.source_ids.as_ref())
            .is_some(),
        filters
            .and_then(|filter| filter.categories.as_ref())
            .is_some(),
        filters.and_then(|filter| filter.min_quality).is_some(),
        filters.and_then(|filter| filter.since).is_some(),
    );
    let keyword_fetch = keyword_candidate_limit(fetch_limit, extra);
    let ann_fetch = ann_candidate_limit(fetch_limit, extra);

    let query_embedding =
        align_vector(query_embedding).map_err(|error| sqlx::Error::Protocol(error.to_string()))?;

    let (keyword_raw, semantic_raw) = tokio::join!(
        keyword_search(
            pool,
            &rewritten.keyword,
            (keyword_fetch * LEG_OVERFETCH_FACTOR).min(MAXIMUM_KEYWORD_CANDIDATES),
            fetch_limit * LEG_OVERFETCH_FACTOR,
            filters,
            config.minimum_keyword_score,
        ),
        semantic_search(
            pool,
            &query_embedding,
            ann_fetch,
            fetch_limit * LEG_OVERFETCH_FACTOR,
            filters,
            config.minimum_semantic_score,
        ),
    );

    let combined = fuse(
        keyword_raw?,
        semantic_raw?,
        fetch_limit,
        &rewritten.identifiers,
        config,
    );
    let combined = expand_same_file(pool, query, combined, filters).await?;
    Ok(finalize_ranking(default_ranker(), combined, query, limit))
}

fn fuse(
    keyword: Vec<KeywordResult>,
    semantic: Vec<SemanticResult>,
    limit: usize,
    identifiers: &[String],
    config: &HybridSearchConfig,
) -> Vec<HybridSearchResult<Id>> {
    let keyword = keep_answer_chunks(keyword, limit, identifiers, |row| {
        (row.chunk_text.as_str(), row.item_uri.as_str())
    });
    let semantic = keep_answer_chunks(semantic, limit, identifiers, |row| {
        (row.chunk_text.as_str(), row.item_uri.as_str())
    });
    reciprocal_rank_fusion(
        keyword
            .into_iter()
            .map(KeywordResult::into_candidate)
            .collect(),
        semantic
            .into_iter()
            .map(SemanticResult::into_candidate)
            .collect(),
        config,
    )
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
    minimum_score: f32,
) -> sqlx::Result<Vec<KeywordResult>> {
    let sanitized_query = sanitize_search_query(query);
    let workspace_id = filters.and_then(|filter| filter.workspace_id);
    let source_ids = filters.and_then(|filter| filter.source_ids.as_deref());
    let categories = filters.and_then(|filter| {
        filter.categories.as_ref().map(|categories| {
            categories
                .iter()
                .map(|category| category.to_lowercase())
                .collect::<Vec<_>>()
        })
    });
    let min_quality = filters.and_then(|filter| filter.min_quality);
    let since = filters.and_then(|filter| filter.since.map(|time| time.naive_utc()));

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
    .bind(minimum_score)
    .bind(workspace_id)
    .bind(source_ids)
    .bind(candidate_limit as i64)
    .bind(categories.as_deref())
    .bind(min_quality.map(|quality| quality as f64))
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
    minimum_similarity: f32,
) -> sqlx::Result<Vec<SemanticResult>> {
    let vector = vector_to_pg_string(query_embedding)
        .map_err(|error| sqlx::Error::Protocol(format!("Invalid embedding vector: {}", error)))?;
    let workspace_id = filters.and_then(|filter| filter.workspace_id);
    let source_ids = filters.and_then(|filter| filter.source_ids.as_deref());
    let categories = filters.and_then(|filter| {
        filter.categories.as_ref().map(|categories| {
            categories
                .iter()
                .map(|category| category.to_lowercase())
                .collect::<Vec<_>>()
        })
    });
    let min_quality = filters.and_then(|filter| filter.min_quality);
    let since = filters.and_then(|filter| filter.since.map(|time| time.naive_utc()));

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
    .bind(&vector)
    .bind(minimum_similarity)
    .bind(workspace_id)
    .bind(source_ids)
    .bind(candidate_limit as i64)
    .bind(categories.as_deref())
    .bind(min_quality.map(|quality| quality as f64))
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

async fn path_matched_item_ids(
    pool: &PgPool,
    query: &str,
    filters: Option<&SearchFilters>,
    limit: usize,
) -> sqlx::Result<Vec<Uuid>> {
    let patterns = path_uri_patterns(query);
    if patterns.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let workspace_id = filters.and_then(|filters| filters.workspace_id);
    sqlx::query_scalar(
        r#"
        SELECT ci.id
        FROM content_items ci
        WHERE ($2::uuid IS NULL OR ci.workspace_id = $2)
          AND ci.uri ILIKE ANY($1::text[])
        ORDER BY length(ci.uri), ci.uri
        LIMIT $3
        "#,
    )
    .bind(&patterns)
    .bind(workspace_id)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

async fn expand_same_file(
    pool: &PgPool,
    query: &str,
    mut results: Vec<HybridSearchResult<Id>>,
    filters: Option<&SearchFilters>,
) -> sqlx::Result<Vec<HybridSearchResult<Id>>> {
    let identifiers = rewrite_query(query).identifiers;
    let mut item_ids = Vec::new();
    let mut seen = HashSet::new();
    for result in &results {
        if seen.insert(result.id.content_item_id) {
            item_ids.push(result.id.content_item_id);
            if item_ids.len() >= RETRIEVED_FILES_TO_EXPAND {
                break;
            }
        }
    }
    for result in &results {
        if item_ids.len() >= MAXIMUM_FILES_TO_EXPAND {
            break;
        }
        if seen.contains(&result.id.content_item_id) {
            continue;
        }
        if !uri_stem_matches_identifier(&result.item_uri, &identifiers) {
            continue;
        }
        seen.insert(result.id.content_item_id);
        item_ids.push(result.id.content_item_id);
    }
    for item_id in path_matched_item_ids(pool, query, filters, PATH_MATCHED_FILES).await? {
        if seen.insert(item_id) {
            item_ids.push(item_id);
        }
    }
    if item_ids.is_empty() {
        return Ok(results);
    }
    let patterns = same_file_patterns(query);
    let identifier_patterns = identifier_file_patterns(query);
    let extras = same_file_chunks(
        pool,
        &item_ids,
        &patterns,
        &identifier_patterns,
        SAME_FILE_CHUNKS_PER_FILE,
    )
    .await?;
    let existing: HashSet<Uuid> = results.iter().map(|result| result.id.chunk_id).collect();
    for row in extras {
        if existing.contains(&row.chunk_id) {
            continue;
        }
        results.push(HybridSearchResult::new(
            row.id(),
            row.chunk_text,
            row.item_uri,
            row.item_title,
            0.0,
        ));
    }
    Ok(results)
}

async fn same_file_chunks(
    pool: &PgPool,
    item_ids: &[Uuid],
    patterns: &[String],
    identifier_patterns: &[String],
    per_file: usize,
) -> sqlx::Result<Vec<KeywordResult>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    let patterns = if patterns.is_empty() {
        None
    } else {
        Some(patterns)
    };
    let identifier_patterns = if identifier_patterns.is_empty() {
        None
    } else {
        Some(identifier_patterns)
    };
    sqlx::query_as(
        r#"
        SELECT
            chunk_id,
            content_item_id,
            source_id,
            chunk_text,
            item_uri,
            item_title,
            score
        FROM (
            SELECT
                cc.id AS chunk_id,
                cc.content_item_id,
                ci.source_id,
                cc.text AS chunk_text,
                ci.uri AS item_uri,
                ci.title AS item_title,
                0::float4 AS score,
                row_number() OVER (
                    PARTITION BY cc.content_item_id
                    ORDER BY
                        CASE
                            WHEN $3::text[] IS NULL THEN 0
                            ELSE (
                                SELECT count(*)::int
                                FROM unnest($3::text[]) p
                                WHERE cc.text ILIKE p
                            )
                        END DESC,
                        CASE
                            WHEN $2::text[] IS NULL THEN 1
                            WHEN cc.text ILIKE ANY($2) THEN 0
                            ELSE 1
                        END,
                        cc.chunk_index
                ) AS rn
            FROM content_chunks cc
            JOIN content_items ci ON ci.id = cc.content_item_id
            WHERE cc.content_item_id = ANY($1)
              AND cc.text NOT ILIKE '%symbol: tests%'
              AND cc.text NOT ILIKE '%symbol: tests.%'
              AND cc.text NOT ILIKE '%assert!(%'
        ) ranked
        WHERE rn <= $4
        "#,
    )
    .bind(item_ids)
    .bind(patterns)
    .bind(identifier_patterns)
    .bind(per_file as i64)
    .fetch_all(pool)
    .await
}

/// Convert a vector to PostgreSQL vector string format
///
/// Returns an error if any vector values are non-finite (NaN or Infinity).
fn vector_to_pg_string(vector: &[f32]) -> Result<String, &'static str> {
    if vector.is_empty() {
        return Ok("[]".to_string());
    }

    if vector.iter().any(|value| !value.is_finite()) {
        tracing::error!("Vector contains non-finite values (NaN or Infinity)");
        return Err("Vector contains non-finite values (NaN or Infinity)");
    }

    let estimated_capacity = VECTOR_BRACKETS_LENGTH + vector.len() * ESTIMATED_VECTOR_VALUE_LENGTH;
    let mut result = String::with_capacity(estimated_capacity);
    result.push('[');

    for (index, value) in vector.iter().enumerate() {
        if index > 0 {
            result.push(',');
        }
        result.push_str(&value.to_string());
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
    minimum_score: f32,
) -> sqlx::Result<Vec<HybridSearchResult<Id>>> {
    let rewritten = rewrite_query(query);
    let extra = has_extra_filters(
        filters
            .as_ref()
            .and_then(|filter| filter.source_ids.as_ref())
            .is_some(),
        filters
            .as_ref()
            .and_then(|filter| filter.categories.as_ref())
            .is_some(),
        filters
            .as_ref()
            .and_then(|filter| filter.min_quality)
            .is_some(),
        filters.as_ref().and_then(|filter| filter.since).is_some(),
    );
    let results = keep_answer_chunks(
        keyword_search(
            pool,
            &rewritten.keyword,
            keyword_candidate_limit(limit * LEG_OVERFETCH_FACTOR, extra),
            limit * LEG_OVERFETCH_FACTOR,
            filters.as_ref(),
            minimum_score,
        )
        .await?,
        limit,
        &rewritten.identifiers,
        |row| (row.chunk_text.as_str(), row.item_uri.as_str()),
    );
    let mapped: Vec<HybridSearchResult<Id>> = results
        .into_iter()
        .enumerate()
        .map(|(rank, row)| {
            HybridSearchResult::new(
                row.id(),
                row.chunk_text,
                row.item_uri,
                row.item_title,
                row.score,
            )
            .with_keyword_rank(rank + 1)
            .with_keyword_score(row.score)
        })
        .collect();
    if first_stage_answered(
        query,
        mapped
            .iter()
            .map(|result| (result.item_uri.as_str(), result.chunk_text.as_str())),
    ) {
        return Ok(finalize_ranking(default_ranker(), mapped, query, limit));
    }
    let mapped = expand_same_file(pool, query, mapped, filters.as_ref()).await?;
    Ok(finalize_ranking(default_ranker(), mapped, query, limit))
}

/// Semantic-only search (no keyword component)
pub async fn semantic_only_search(
    pool: &PgPool,
    query_embedding: &[f32],
    limit: usize,
    filters: Option<SearchFilters>,
    minimum_similarity: f32,
) -> sqlx::Result<Vec<HybridSearchResult<Id>>> {
    let query_embedding =
        align_vector(query_embedding).map_err(|error| sqlx::Error::Protocol(error.to_string()))?;

    let extra = has_extra_filters(
        filters
            .as_ref()
            .and_then(|filter| filter.source_ids.as_ref())
            .is_some(),
        filters
            .as_ref()
            .and_then(|filter| filter.categories.as_ref())
            .is_some(),
        filters
            .as_ref()
            .and_then(|filter| filter.min_quality)
            .is_some(),
        filters.as_ref().and_then(|filter| filter.since).is_some(),
    );
    let results = semantic_search(
        pool,
        &query_embedding,
        ann_candidate_limit(limit, extra),
        limit,
        filters.as_ref(),
        minimum_similarity,
    )
    .await?;

    Ok(results
        .into_iter()
        .enumerate()
        .map(|(rank, row)| {
            HybridSearchResult::new(
                row.id(),
                row.chunk_text,
                row.item_uri,
                row.item_title,
                row.similarity,
            )
            .with_semantic_rank(rank + 1)
            .with_semantic_score(row.similarity)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEG_SIZE: u128 = 10;

    fn id(chunk: u128) -> Id {
        Id::new(
            Uuid::from_u128(chunk),
            Uuid::from_u128(chunk + 1_000),
            Uuid::from_u128(1),
        )
    }

    fn keyword_row(chunk: u128, text: &str, score: f32) -> KeywordResult {
        KeywordResult {
            chunk_id: Uuid::from_u128(chunk),
            content_item_id: Uuid::from_u128(chunk + 1_000),
            source_id: Uuid::from_u128(1),
            chunk_text: text.to_string(),
            item_uri: format!("github://acme/widgets/src/keyword_{chunk}.rs@main"),
            item_title: format!("keyword_{chunk}.rs"),
            score,
        }
    }

    fn semantic_row(chunk: u128, text: &str, similarity: f32) -> SemanticResult {
        SemanticResult {
            chunk_id: Uuid::from_u128(chunk),
            content_item_id: Uuid::from_u128(chunk + 1_000),
            source_id: Uuid::from_u128(1),
            chunk_text: text.to_string(),
            item_uri: format!("github://acme/widgets/src/semantic_{chunk}.rs@main"),
            item_title: format!("semantic_{chunk}.rs"),
            similarity,
        }
    }

    #[test]
    fn fusion_ties_keep_the_keyword_leg_first() {
        let keyword: Vec<KeywordResult> = (1..=LEG_SIZE)
            .map(|chunk| keyword_row(chunk, &format!("fn restock_{chunk}() {{}}"), 0.5))
            .collect();
        let semantic: Vec<SemanticResult> = (1..=LEG_SIZE)
            .map(|chunk| {
                let chunk = chunk + 100;
                semantic_row(chunk, &format!("fn reorder_{chunk}() {{}}"), 0.8)
            })
            .collect();
        let config = HybridSearchConfig::default().with_semantic_weight(0.5);

        let results = fuse(keyword, semantic, LEG_SIZE as usize, &[], &config);

        let expected: Vec<Id> = (1..=LEG_SIZE)
            .flat_map(|rank| [id(rank), id(rank + 100)])
            .collect();
        let actual: Vec<Id> = results.iter().map(|result| result.id).collect();
        assert_eq!(actual, expected);
        for pair in results.chunks(2) {
            assert_eq!(pair[0].score.to_bits(), pair[1].score.to_bits());
        }
    }

    #[test]
    fn a_chunk_in_both_legs_keeps_the_keyword_text() {
        let keyword = vec![keyword_row(7, "fn restock() { keyword }", 0.4)];
        let semantic = vec![semantic_row(7, "fn restock() { semantic }", 0.6)];

        let results = fuse(keyword, semantic, 1, &[], &HybridSearchConfig::default());

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(result.id, id(7));
        assert_eq!(result.chunk_text, "fn restock() { keyword }");
        assert_eq!(
            result.item_uri,
            "github://acme/widgets/src/keyword_7.rs@main"
        );
        assert_eq!(result.keyword_rank, Some(1));
        assert_eq!(result.semantic_rank, Some(1));
        assert_eq!(result.keyword_score, Some(0.4));
        assert_eq!(result.semantic_score, Some(0.6));
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
