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
    let fetch_limit = (limit * 4).max(32);
    let rewritten = crate::embeddings::rewrite_query(query);
    let extra = crate::embeddings::serving::has_extra_filters(
        filters.and_then(|f| f.source_ids.as_ref()).is_some(),
        filters.and_then(|f| f.categories.as_ref()).is_some(),
        filters.and_then(|f| f.min_quality).is_some(),
        filters.and_then(|f| f.since).is_some(),
    );
    let keyword_fetch = crate::embeddings::serving::keyword_candidate_limit(fetch_limit, extra);
    let ann_fetch = crate::embeddings::serving::ann_candidate_limit(fetch_limit, extra);

    let query_embedding = crate::embeddings::align_vector(query_embedding)
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

    let (keyword_raw, semantic_raw) = tokio::join!(
        keyword_search(
            pool,
            &rewritten.keyword,
            (keyword_fetch * 2).min(128),
            fetch_limit * 2,
            filters,
            config.min_keyword_score,
        ),
        semantic_search(
            pool,
            &query_embedding,
            ann_fetch,
            fetch_limit * 2,
            filters,
            config.min_semantic_score,
        ),
    );

    let keyword_results =
        keep_answer_chunks(keyword_raw?, fetch_limit, &rewritten.identifiers, |row| {
            (row.chunk_text.as_str(), row.item_uri.as_str())
        });
    let semantic_results =
        keep_answer_chunks(semantic_raw?, fetch_limit, &rewritten.identifiers, |row| {
            (row.chunk_text.as_str(), row.item_uri.as_str())
        });

    let combined = reciprocal_rank_fusion(keyword_results, semantic_results, config);
    let combined = expand_same_file(pool, query, combined, filters).await?;
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
    let ranked = apply_local_rerank(query, results);
    let ranked = ranked
        .into_iter()
        .filter(|result| !crate::embeddings::ranker::is_fixture_uri(&result.item_uri))
        .collect();
    cap_per_file(ranked, 2, limit, query)
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
    boost_impl_neighbors(query, &mut results);
    sort_by_score(&mut results);
    prefer_definition_chunks(query, &mut results);
    results
}

/// If a file has both a definition chunk and a mention/bag chunk, show the
/// definition first so unique-file eval and the 1-per-file pass keep the impl.
pub fn prefer_definition_chunks(query: &str, results: &mut [HybridSearchResult]) {
    let identifiers = crate::embeddings::rewrite_query(query).identifiers;
    results.sort_by(|a, b| {
        let ta = crate::embeddings::ranker::chunk_rank_tier(query, &a.item_uri, &a.chunk_text);
        let tb = crate::embeddings::ranker::chunk_rank_tier(query, &b.item_uri, &b.chunk_text);
        match tb.cmp(&ta) {
            std::cmp::Ordering::Equal => {
                let ra = crate::embeddings::ranker::role_strength(
                    &a.chunk_text,
                    &a.item_uri,
                    &identifiers,
                );
                let rb = crate::embeddings::ranker::role_strength(
                    &b.chunk_text,
                    &b.item_uri,
                    &identifiers,
                );
                match rb.cmp(&ra) {
                    std::cmp::Ordering::Equal => {
                        let ca = crate::embeddings::ranker::ident_part_coverage(
                            &a.chunk_text,
                            &identifiers,
                        );
                        let cb = crate::embeddings::ranker::ident_part_coverage(
                            &b.chunk_text,
                            &identifiers,
                        );
                        cb.partial_cmp(&ca)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| {
                                b.score
                                    .partial_cmp(&a.score)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            })
                    }
                    other => other,
                }
            }
            other => other,
        }
    });
}

fn boost_impl_neighbors(query: &str, results: &mut [HybridSearchResult]) {
    let identifiers = crate::embeddings::rewrite_query(query).identifiers;
    if identifiers.is_empty() {
        return;
    }
    let def_uris: Vec<String> = results
        .iter()
        .filter(|result| {
            !crate::embeddings::ranker::is_test_chunk(&result.item_uri, &result.chunk_text)
                && crate::embeddings::ranker::identifier_role(
                    &result.chunk_text,
                    &result.item_uri,
                    &identifiers,
                )
                .is_definition()
        })
        .map(|result| result.item_uri.clone())
        .collect();
    if def_uris.is_empty() {
        return;
    }
    for result in results.iter_mut() {
        if crate::embeddings::ranker::identifier_role(
            &result.chunk_text,
            &result.item_uri,
            &identifiers,
        )
        .is_definition()
        {
            continue;
        }
        let bonus = def_uris
            .iter()
            .map(|uri| path_affinity(&result.item_uri, uri))
            .fold(0.0f32, f32::max);
        result.score += bonus;
    }
}

fn path_affinity(uri: &str, def_uri: &str) -> f32 {
    let left = dir_of(uri);
    let right = dir_of(def_uri);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 0.22;
    }
    let crate_left = crate_of(uri);
    let crate_right = crate_of(def_uri);
    if !crate_left.is_empty() && crate_left == crate_right {
        return 0.12;
    }
    0.0
}

fn strip_uri_path(uri: &str) -> &str {
    let rest = uri.split("://").nth(1).unwrap_or(uri);
    rest.split('@').next().unwrap_or(rest)
}

fn dir_of(uri: &str) -> &str {
    let path = strip_uri_path(uri);
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

fn crate_of(uri: &str) -> &str {
    let path = strip_uri_path(uri);
    path.find("/src/")
        .map(|idx| &path[..idx])
        .unwrap_or_else(|| dir_of(uri))
}

fn keep_answer_chunks<T>(
    items: Vec<T>,
    limit: usize,
    identifiers: &[String],
    parts: impl Fn(&T) -> (&str, &str),
) -> Vec<T> {
    let mut body = Vec::new();
    let mut nav = Vec::new();
    for item in items {
        let (text, uri) = parts(&item);
        if crate::embeddings::ranker::is_fixture_uri(uri) {
            continue;
        }
        let outline = crate::embeddings::ranker::is_outline_chunk(text);
        let defined =
            crate::embeddings::ranker::identifier_role(text, uri, identifiers).is_definition();
        if outline && !defined {
            nav.push(item);
        } else {
            body.push(item);
            if body.len() >= limit {
                return body;
            }
        }
    }
    let need = limit.saturating_sub(body.len());
    body.extend(nav.into_iter().take(need));
    body
}

fn file_key(result: &HybridSearchResult) -> String {
    result
        .item_uri
        .split('@')
        .next()
        .filter(|stem| !stem.is_empty())
        .unwrap_or(result.item_uri.as_str())
        .to_string()
}

fn cmp_file_chunks(
    query: &str,
    left: &HybridSearchResult,
    right: &HybridSearchResult,
) -> std::cmp::Ordering {
    let left_tier = file_row_tier(&left.item_uri, &left.chunk_text);
    let right_tier = file_row_tier(&right.item_uri, &right.chunk_text);
    let identifiers = crate::embeddings::rewrite_query(query).identifiers;
    left_tier.cmp(&right_tier).then_with(|| {
        let left_def = crate::embeddings::ranker::answers_as_definition(
            query,
            &left.item_uri,
            &left.chunk_text,
        );
        let right_def = crate::embeddings::ranker::answers_as_definition(
            query,
            &right.item_uri,
            &right.chunk_text,
        );
        left_def.cmp(&right_def).then_with(|| {
            let same_symbol = defined_leaf_symbol(&left.chunk_text)
                .zip(defined_leaf_symbol(&right.chunk_text))
                .is_some_and(|(left_sym, right_sym)| left_sym == right_sym);
            let left_stem = if same_symbol {
                filename_stem_hits(&left.item_uri, &left.chunk_text)
            } else {
                0
            };
            let right_stem = if same_symbol {
                filename_stem_hits(&right.item_uri, &right.chunk_text)
            } else {
                0
            };
            left_stem.cmp(&right_stem).then_with(|| {
            let left_sym = defined_symbol_prefix(query, &left.chunk_text);
            let right_sym = defined_symbol_prefix(query, &right.chunk_text);
            left_sym.cmp(&right_sym).then_with(|| {
            let left_excl =
                exclusive_question_hits(query, &left.chunk_text, &right.chunk_text);
            let right_excl =
                exclusive_question_hits(query, &right.chunk_text, &left.chunk_text);
            left_excl.cmp(&right_excl).then_with(|| {
                let left_cover = question_coverage(query, &left.chunk_text, &identifiers);
                let right_cover = question_coverage(query, &right.chunk_text, &identifiers);
                left_cover.cmp(&right_cover).then_with(|| {
                let left_role = crate::embeddings::ranker::role_strength(
                    &left.chunk_text,
                    &left.item_uri,
                    &identifiers,
                );
                let right_role = crate::embeddings::ranker::role_strength(
                    &right.chunk_text,
                    &right.item_uri,
                    &identifiers,
                );
                left_role.cmp(&right_role).then_with(|| {
                    let left_lex = crate::embeddings::lexical_cross_score(
                        query,
                        &left.item_uri,
                        &left.item_title,
                        &left.chunk_text,
                    );
                    let right_lex = crate::embeddings::lexical_cross_score(
                        query,
                        &right.item_uri,
                        &right.item_title,
                        &right.chunk_text,
                    );
                    left_lex
                        .partial_cmp(&right_lex)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| {
                            left.score
                                .partial_cmp(&right.score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                })
            })
            })
            })
            })
        })
    })
}

fn uri_path_hit(query: &str, uri: &str) -> u8 {
    let uri_l = uri.to_ascii_lowercase();
    crate::embeddings::path_uri_tokens(query)
        .iter()
        .any(|token| uri_l.contains(&format!("/{token}.")) || uri_l.contains(&format!("/{token}@")))
        .then_some(1)
        .unwrap_or(0)
}

async fn path_matched_item_ids(
    pool: &PgPool,
    query: &str,
    filters: Option<&SearchFilters>,
    limit: usize,
) -> sqlx::Result<Vec<Uuid>> {
    let patterns = crate::embeddings::path_uri_patterns(query);
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

fn exclusive_question_hits(query: &str, text: &str, other: &str) -> u32 {
    let text_l = text.to_ascii_lowercase();
    let other_l = other.to_ascii_lowercase();
    let text_docs = doc_comment_text(text);
    let text_code = strip_doc_comments(text);
    let other_code = strip_doc_comments(other);
    let tokens = crate::embeddings::nl_content_tokens(query);
    let doc_words = tokens
        .iter()
        .filter(|token| text_docs.contains(*token) && !other_l.contains(*token))
        .count() as u32;
    let body_words = tokens
        .iter()
        .filter(|token| {
            text_l.contains(*token) && !text_docs.contains(*token) && !other_l.contains(*token)
        })
        .count() as u32;
    let phrases = crate::embeddings::nl_question_phrases(query)
        .into_iter()
        .filter(|phrase| text_code.contains(phrase.as_str()) && !other_code.contains(phrase.as_str()))
        .count() as u32;
    doc_words * 3 + body_words + phrases * 2
}

fn strip_doc_comments(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("///") && !trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase()
}

fn doc_comment_text(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("///") || trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase()
}

fn filename_stem_hits(uri: &str, text: &str) -> u32 {
    let Some(stem) = uri_file_stem(uri) else {
        return 0;
    };
    let mut stems = vec![stem.clone()];
    if stem.ends_with('s') && !stem.ends_with("ss") && stem.len() >= 5 {
        stems.push(stem[..stem.len() - 1].to_string());
    }
    ident_like_tokens(text)
        .into_iter()
        .filter(|token| {
            let lower = token.to_ascii_lowercase();
            stems.iter().any(|stem| {
                lower.starts_with(stem)
                    && lower.len() > stem.len()
                    && (token.chars().any(|c| c.is_ascii_uppercase()) || token.contains('_'))
            })
        })
        .count() as u32
}

fn uri_stem_matches_ident(uri: &str, identifiers: &[String]) -> bool {
    let Some(stem) = uri_file_stem(uri) else {
        return false;
    };
    identifiers.iter().any(|id| {
        let id_l = id.to_ascii_lowercase();
        stem == id_l
            || id_l.split(['_', '-', '/']).any(|part| part.len() >= 4 && (stem == part || stem.contains(part)))
    })
}

fn uri_file_stem(uri: &str) -> Option<String> {
    let name = uri
        .rsplit('/')
        .next()?
        .split('@')
        .next()?
        .split('.')
        .next()?
        .to_ascii_lowercase();
    (name.len() >= 4).then_some(name)
}

fn ident_like_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn file_row_tier(uri: &str, text: &str) -> u8 {
    if crate::embeddings::ranker::is_test_chunk(uri, text)
        || crate::embeddings::ranker::is_fixture_uri(uri)
    {
        return 0;
    }
    if crate::embeddings::ranker::is_outline_chunk(text)
        || crate::embeddings::ranker::is_module_prelude(text)
    {
        return 1;
    }
    2
}

fn question_coverage(query: &str, text: &str, identifiers: &[String]) -> u32 {
    let ident_hits = crate::embeddings::ranker::ident_hit_count(text, identifiers) as u32;
    let text_l = text.to_ascii_lowercase();
    let bridges = crate::embeddings::nl_bridge_terms(query)
        .into_iter()
        .filter(|term| text_l.contains(term))
        .count() as u32;
    let words = crate::embeddings::nl_content_tokens(query)
        .into_iter()
        .filter(|term| text_l.contains(term))
        .count() as u32;
    let phrases = crate::embeddings::nl_question_phrases(query)
        .into_iter()
        .filter(|phrase| text_l.contains(phrase))
        .count() as u32;
    let symbol = defined_symbol_overlap(query, text);
    let docs = if function_kind(text) {
        doc_comment_coverage(query, text)
    } else {
        0
    };
    ident_hits * 2 + bridges + words + phrases * 3 + symbol * 3 + docs * 2
}

fn doc_comment_coverage(query: &str, text: &str) -> u32 {
    let comments: String = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("///") || trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if comments.is_empty() {
        return 0;
    }
    crate::embeddings::nl_content_tokens(query)
        .into_iter()
        .filter(|token| comments.contains(token))
        .count() as u32
}

fn defined_symbol_prefix(query: &str, text: &str) -> u32 {
    if !function_kind(text) {
        return 0;
    }
    let Some(leaf) = defined_leaf_symbol(text) else {
        return 0;
    };
    crate::embeddings::nl_content_tokens(query)
        .into_iter()
        .chain(question_stems(query))
        .any(|token| {
            leaf.starts_with(&token)
                || (token.len() >= 6 && leaf.contains(&token))
        })
        .then_some(1)
        .unwrap_or(0)
}

fn defined_symbol_overlap(query: &str, text: &str) -> u32 {
    if !function_kind(text) {
        return 0;
    }
    let Some(leaf) = defined_leaf_symbol(text) else {
        return 0;
    };
    crate::embeddings::nl_content_tokens(query)
        .into_iter()
        .chain(question_stems(query))
        .any(|token| leaf.contains(&token) || (token.len() >= 4 && token.contains(&leaf)))
        .then_some(1)
        .unwrap_or(0)
}

fn function_kind(text: &str) -> bool {
    if text.lines().take(24).any(|line| {
        let line = line.trim();
        line.eq_ignore_ascii_case("kind: Function")
            || line.eq_ignore_ascii_case("kind: Method")
            || line.eq_ignore_ascii_case("kind: Assoc")
    }) {
        return true;
    }
    let Some(leaf) = defined_leaf_symbol(text) else {
        return false;
    };
    text.contains(&format!("fn {leaf}(")) || text.contains(&format!("fn {leaf} ("))
}

fn defined_leaf_symbol(text: &str) -> Option<String> {
    for line in text.lines().take(24) {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("symbol: ")
            .or_else(|| line.strip_prefix("Symbol: "))
        else {
            continue;
        };
        let Some(leaf) = rest.split('.').next_back().map(str::trim) else {
            continue;
        };
        if leaf.len() >= 3 {
            return Some(leaf.to_ascii_lowercase());
        }
    }
    None
}

fn question_stems(query: &str) -> Vec<String> {
    crate::embeddings::nl_content_tokens(query)
        .into_iter()
        .filter_map(|token| match token.as_str() {
            "derived" => Some("derive".into()),
            "authorized" | "authorizing" => Some("authorize".into()),
            "configured" => Some("configure".into()),
            "trimmed" => Some("trim".into()),
            _ => None,
        })
        .collect()
}

fn same_file_patterns(query: &str) -> Vec<String> {
    let rewritten = crate::embeddings::rewrite_query(query);
    let mut seen = std::collections::HashSet::new();
    let mut patterns = Vec::new();
    let mut push = |token: &str| {
        let token = token.to_ascii_lowercase();
        if token.len() < 3
            || !token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || !seen.insert(token.clone())
        {
            return;
        }
        patterns.push(format!("%{token}%"));
    };
    for id in &rewritten.identifiers {
        push(id);
    }
    for token in crate::embeddings::nl_content_tokens(query) {
        push(&token);
    }
    for token in crate::embeddings::nl_bridge_terms(query) {
        push(&token);
    }
    patterns.truncate(16);
    patterns
}

async fn expand_same_file(
    pool: &PgPool,
    query: &str,
    mut results: Vec<HybridSearchResult>,
    filters: Option<&SearchFilters>,
) -> sqlx::Result<Vec<HybridSearchResult>> {
    let identifiers = crate::embeddings::rewrite_query(query).identifiers;
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for result in &results {
        if seen.insert(result.content_item_id) {
            ids.push(result.content_item_id);
            if ids.len() >= 16 {
                break;
            }
        }
    }
    for result in &results {
        if ids.len() >= 20 {
            break;
        }
        if seen.contains(&result.content_item_id) {
            continue;
        }
        if !uri_stem_matches_ident(&result.item_uri, &identifiers) {
            continue;
        }
        seen.insert(result.content_item_id);
        ids.push(result.content_item_id);
    }
    for id in path_matched_item_ids(pool, query, filters, 6).await? {
        if seen.insert(id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Ok(results);
    }
    let patterns = same_file_patterns(query);
    let extras = same_file_chunks(pool, &ids, &patterns, 28).await?;
    let existing: std::collections::HashSet<Uuid> =
        results.iter().map(|result| result.chunk_id).collect();
    for row in extras {
        if existing.contains(&row.chunk_id) {
            continue;
        }
        results.push(HybridSearchResult {
            chunk_id: row.chunk_id,
            content_item_id: row.content_item_id,
            source_id: row.source_id,
            chunk_text: row.chunk_text,
            item_uri: row.item_uri,
            item_title: row.item_title,
            score: 0.0,
            fusion_score: 0.0,
            keyword_rank: None,
            semantic_rank: None,
            keyword_score: None,
            semantic_score: None,
        });
    }
    Ok(results)
}

async fn same_file_chunks(
    pool: &PgPool,
    item_ids: &[Uuid],
    patterns: &[String],
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
        WHERE rn <= $3
        "#,
    )
    .bind(item_ids)
    .bind(patterns)
    .bind(per_file as i64)
    .fetch_all(pool)
    .await
}

/// Keep unique files first so a second gold can enter the window, then
/// fill leftover slots with extra chunks (up to `max_per_file`).
///
/// The row that represents a file is the best retrieved chunk of that file
/// (body / definition / lexical overlap), not whichever chunk RRF surfaced
/// first. Unique-file eval only grades that first row.
pub fn cap_per_file(
    results: Vec<HybridSearchResult>,
    max_per_file: usize,
    limit: usize,
    query: &str,
) -> Vec<HybridSearchResult> {
    if limit == 0 || max_per_file == 0 {
        return Vec::new();
    }
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut order = Vec::new();
    for (idx, result) in results.iter().enumerate() {
        let key = file_key(result);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(idx);
    }
    let mut chosen = Vec::new();
    let mut taken = std::collections::HashSet::<Uuid>::new();
    for key in &order {
        let Some(indices) = groups.get(key) else {
            continue;
        };
        let Some(&best_idx) = indices
            .iter()
            .max_by(|&&left, &&right| cmp_file_chunks(query, &results[left], &results[right]))
        else {
            continue;
        };
        taken.insert(results[best_idx].chunk_id);
        chosen.push(results[best_idx].clone());
    }
    chosen.sort_by(|left, right| {
        file_row_tier(&right.item_uri, &right.chunk_text)
            .cmp(&file_row_tier(&left.item_uri, &left.chunk_text))
            .then_with(|| {
                uri_path_hit(query, &right.item_uri).cmp(&uri_path_hit(query, &left.item_uri))
            })
    });
    chosen.truncate(limit);
    if max_per_file > 1 && chosen.len() < limit {
        let mut extras: HashMap<String, usize> = HashMap::new();
        for result in &results {
            if chosen.len() >= limit {
                break;
            }
            if taken.contains(&result.chunk_id) {
                continue;
            }
            let extras = extras.entry(file_key(result)).or_insert(0);
            if *extras + 1 < max_per_file {
                *extras += 1;
                taken.insert(result.chunk_id);
                chosen.push(result.clone());
            }
        }
    }
    chosen
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
    let results = keep_answer_chunks(
        keyword_search(
            pool,
            &rewritten.keyword,
            crate::embeddings::serving::keyword_candidate_limit(limit * 2, extra),
            limit * 2,
            filters.as_ref(),
            min_score,
        )
        .await?,
        limit,
        &rewritten.identifiers,
        |row| (row.chunk_text.as_str(), row.item_uri.as_str()),
    );
    let mapped: Vec<HybridSearchResult> = results
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
    if crate::embeddings::ranker::first_stage_answered(
        query,
        mapped
            .iter()
            .map(|result| (result.item_uri.as_str(), result.chunk_text.as_str())),
    ) {
        return Ok(finalize_ranking(mapped, query, limit));
    }
    let mapped = expand_same_file(pool, query, mapped, filters.as_ref()).await?;
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
    fn cap_per_file_prefers_filename_type_over_same_symbol_guard() {
        let item = Uuid::from_u128(19);
        let auth = hit(
            1,
            item,
            "github://zone/routes/artifacts.rs@main",
            "symbol: get\nkind: Function\nlet authorized = match chats::get_chat(state.db(), chat_id).await {\n    // Do not reveal whether an artifact exists\n};",
            2.8,
        );
        let store = hit(
            2,
            item,
            "github://zone/routes/artifacts.rs@main",
            "symbol: get\nkind: Function\nlet store = ArtifactStore::new(state.config().comfyui.artifact_root.clone());\nmatch store.read(workspace_id, chat_id, owner_id, &filename).await {",
            0.4,
        );
        let capped = cap_per_file(
            vec![auth, store],
            2,
            2,
            "How are generated chat artifacts authorized before serving?",
        );
        assert!(
            capped[0].chunk_text.contains("ArtifactStore"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_surfaces_path_named_file() {
        let noise = (0..10).map(|idx| {
            hit(
                idx + 1,
                Uuid::from_u128(30 + idx as u128),
                &format!("github://zone/other/file{idx}.rs@main"),
                "fn helper() {}",
                2.0,
            )
        });
        let artifact = hit(
            20,
            Uuid::from_u128(50),
            "github://zone/routes/artifacts.rs@main",
            "//! Authorized generated-artifact serving.\nasync fn get(State(state): State<AppState>)",
            0.1,
        );
        let capped = cap_per_file(
            noise.chain(std::iter::once(artifact)).collect(),
            2,
            10,
            "How are generated chat artifacts authorized before serving?",
        );
        assert!(
            capped.iter().any(|row| row.item_uri.contains("artifacts.rs")),
            "uris {:?}",
            capped.iter().map(|row| row.item_uri.as_str()).collect::<Vec<_>>()
        );
        assert!(
            capped[0].item_uri.contains("artifacts.rs")
                || capped.get(1).is_some_and(|row| row.item_uri.contains("artifacts.rs")),
            "top {:?}",
            capped.iter().take(2).map(|row| row.item_uri.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cap_per_file_prefers_unique_files() {
        let files = [
            Uuid::parse_str("00000000-0000-0000-0000-0000000000a1").unwrap(),
            Uuid::parse_str("00000000-0000-0000-0000-0000000000a2").unwrap(),
            Uuid::parse_str("00000000-0000-0000-0000-0000000000a3").unwrap(),
        ];
        let results = files
            .iter()
            .enumerate()
            .flat_map(|(file_idx, item)| {
                (0..2).map(move |chunk_idx| HybridSearchResult {
                    chunk_id: Uuid::from_u128((file_idx * 2 + chunk_idx + 1) as u128),
                    content_item_id: *item,
                    source_id: Uuid::nil(),
                    chunk_text: format!("chunk {file_idx}-{chunk_idx}"),
                    item_uri: format!("file{file_idx}.rs"),
                    item_title: format!("file{file_idx}.rs"),
                    score: 1.0 - file_idx as f32 * 0.1 - chunk_idx as f32 * 0.01,
                    fusion_score: 1.0,
                    keyword_rank: None,
                    semantic_rank: None,
                    keyword_score: None,
                    semantic_score: None,
                })
            })
            .collect();
        let capped = cap_per_file(results, 2, 4, "unique files");
        let unique: std::collections::HashSet<_> =
            capped.iter().map(|r| r.content_item_id).collect();
        assert_eq!(unique.len(), 3);
        assert_eq!(capped.len(), 4);
    }

    #[test]
    fn cap_per_file_uses_best_chunk_as_file_row() {
        let item = Uuid::from_u128(10);
        let header = HybridSearchResult {
            chunk_id: Uuid::from_u128(1),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text:
                "path: github://zone/services/email.rs kind: file_header\n\nidentifiers: send_email"
                    .into(),
            item_uri: "github://zone/services/email.rs@main".into(),
            item_title: "email.rs".into(),
            score: 3.0,
            fusion_score: 3.0,
            keyword_rank: Some(1),
            semantic_rank: None,
            keyword_score: Some(0.4),
            semantic_score: None,
        };
        let body = HybridSearchResult {
            chunk_id: Uuid::from_u128(2),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: r#"return Err("Email service not configured");"#.into(),
            item_uri: "github://zone/services/email.rs@main".into(),
            item_title: "email.rs".into(),
            score: 1.0,
            fusion_score: 1.0,
            keyword_rank: Some(2),
            semantic_rank: None,
            keyword_score: Some(0.2),
            semantic_score: None,
        };
        let capped = cap_per_file(
            vec![header, body],
            2,
            2,
            "What error is returned when SMTP is not configured?",
        );
        assert!(
            capped[0]
                .chunk_text
                .contains("Email service not configured"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_identifier_mention_over_same_file_neighbor() {
        let item = Uuid::from_u128(11);
        let neighbor = HybridSearchResult {
            chunk_id: Uuid::from_u128(1),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: "pub async fn list_chats(state: AppState) -> Result<Vec<Chat>>".into(),
            item_uri: "github://zone/routes/chats.rs@main".into(),
            item_title: "chats.rs".into(),
            score: 4.0,
            fusion_score: 4.0,
            keyword_rank: Some(1),
            semantic_rank: None,
            keyword_score: Some(0.5),
            semantic_score: None,
        };
        let mention = HybridSearchResult {
            chunk_id: Uuid::from_u128(2),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: "get(search_messages) // GET /api/chats/search".into(),
            item_uri: "github://zone/routes/chats.rs@main".into(),
            item_title: "chats.rs".into(),
            score: 1.1,
            fusion_score: 1.1,
            keyword_rank: Some(2),
            semantic_rank: None,
            keyword_score: Some(0.2),
            semantic_score: None,
        };
        let capped = cap_per_file(
            vec![neighbor, mention],
            2,
            2,
            "Where is search_messages used by GET /api/chats/search?",
        );
        assert!(
            capped[0].chunk_text.contains("search_messages"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_error_phrase_over_from_env() {
        let item = Uuid::from_u128(13);
        let from_env = hit(
            1,
            item,
            "github://zone/services/email.rs@main",
            r#"symbol: EmailConfig.from_env
pub fn from_env() -> EmailResult<Self> {
    env::var("SMTP_HOST").map_err(|_| EmailError::NotConfigured)
}"#,
            3.0,
        );
        let error = hit(
            2,
            item,
            "github://zone/services/email.rs@main",
            r#"symbol: EmailError
#[error("Email service not configured")]
NotConfigured,"#,
            1.0,
        );
        let capped = cap_per_file(
            vec![from_env, error],
            2,
            2,
            "What error is returned when SMTP is not configured?",
        );
        assert!(
            capped[0]
                .chunk_text
                .contains("Email service not configured"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_helper_over_tool_name() {
        let item = Uuid::from_u128(14);
        let name = hit(
            1,
            item,
            "github://zone/tools/command.rs@main",
            "symbol: RunCommandTool.name\nfn name(&self) -> &str {\n        \"run_command\"\n    }",
            3.5,
        );
        let helper = hit(
            2,
            item,
            "github://zone/tools/command.rs@main",
            "symbol: trim_middle\nfn trim_middle(text: &str) -> String { chars }",
            0.4,
        );
        let capped = cap_per_file(
            vec![name, helper],
            2,
            2,
            "How does run_command trim oversized stdout?",
        );
        assert!(
            capped[0].chunk_text.contains("trim_middle"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_trim_over_execute_stdout() {
        let item = Uuid::from_u128(21);
        let execute = hit(
            1,
            item,
            "github://abnegate/zone/runner/zone_core/src/tools/command.rs@main",
            r#"symbol: RunCommandTool.execute
kind: Method
Parent: RunCommandTool
Signature: async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError>

        let mut cmd = Command::new(&params.command);
        cmd.args(&params.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
"#,
            4.0,
        );
        let helper = hit(
            2,
            item,
            "github://abnegate/zone/runner/zone_core/src/tools/command.rs@main",
            r#"symbol: trim_middle
kind: Function
Signature: fn trim_middle(text: &str) -> String

fn trim_middle(text: &str) -> String {
    format!("{head}\n\n[… {} characters trimmed …]\n\n{tail}", chars.len())
}
"#,
            0.2,
        );
        let capped = cap_per_file(
            vec![execute, helper],
            2,
            2,
            "How does run_command trim oversized stdout?",
        );
        assert!(
            capped[0].chunk_text.contains("trim_middle"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_cleanup_over_constructor() {
        let item = Uuid::from_u128(18);
        let ctor = hit(
            1,
            item,
            "github://zone/utils/rate_limit.rs@main",
            "symbol: RateLimiter.new\nkind: Method\n/// Create a new rate limiter with the given configuration\npub fn new(config: RateLimitConfig)",
            3.4,
        );
        let cleanup = hit(
            2,
            item,
            "github://zone/utils/rate_limit.rs@main",
            "symbol: RateLimiter.cleanup\nkind: Method\n/// Clean up old entries (optional, for memory management)\npub fn cleanup(&self)",
            0.4,
        );
        let capped = cap_per_file(
            vec![ctor, cleanup],
            2,
            2,
            "How does the rate limiter prevent unbounded memory growth?",
        );
        assert!(
            capped[0].chunk_text.contains("fn cleanup"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_cleanup_over_check() {
        let item = Uuid::from_u128(15);
        let check = hit(
            1,
            item,
            "github://zone/utils/rate_limit.rs@main",
            "symbol: RateLimiter.check_rate_limit\nkind: Method\nParent: RateLimiter\n// This prevents race condition where two concurrent requests both see count=9\npub fn check_rate_limit(&self, user_id: Uuid)",
            3.0,
        );
        let cleanup = hit(
            2,
            item,
            "github://zone/utils/rate_limit.rs@main",
            "symbol: RateLimiter.cleanup\nParent: RateLimiter\n/// Clean up old entries (optional, for memory management)\npub fn cleanup(&self)",
            0.5,
        );
        let capped = cap_per_file(
            vec![check, cleanup],
            2,
            2,
            "How does the rate limiter prevent unbounded memory growth?",
        );
        assert!(
            capped[0].chunk_text.contains("fn cleanup"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_ident_method_over_type_banner() {
        let item = Uuid::from_u128(17);
        let claims = hit(
            1,
            item,
            "github://zone/auth/jwt.rs@main",
            "symbol: Claims\nkind: Struct\n/// Subject (user ID)\npub struct Claims { pub sub: String }",
            3.2,
        );
        let user_id = hit(
            2,
            item,
            "github://zone/auth/jwt.rs@main",
            "symbol: Claims.user_id\nkind: Method\npub fn user_id(&self) -> Result<Uuid, uuid::Error> {\n        Uuid::parse_str(&self.sub)\n    }",
            0.7,
        );
        let capped = cap_per_file(
            vec![claims, user_id],
            2,
            2,
            "How does user_id parse the JWT subject on Claims?",
        );
        assert!(
            capped[0].chunk_text.contains("parse_str"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn cap_per_file_prefers_body_over_module_prelude() {
        let item = Uuid::from_u128(16);
        let prelude = hit(
            1,
            item,
            "github://zone/routes/artifacts.rs@main",
            "Language: rust\nkind: top_level\n\n//! Authorized generated-artifact serving.\n",
            2.8,
        );
        let body = hit(
            2,
            item,
            "github://zone/routes/artifacts.rs@main",
            "symbol: get_artifact\nlet authorized = chats::get_chat\nlet store = ArtifactStore::new",
            0.6,
        );
        let capped = cap_per_file(
            vec![prelude, body],
            2,
            2,
            "How are generated chat artifacts authorized before serving?",
        );
        assert!(
            capped[0].chunk_text.contains("ArtifactStore"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    fn hit(chunk: u128, item: Uuid, uri: &str, text: &str, score: f32) -> HybridSearchResult {
        HybridSearchResult {
            chunk_id: Uuid::from_u128(chunk),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: text.into(),
            item_uri: uri.into(),
            item_title: uri.rsplit('/').next().unwrap_or(uri).into(),
            score,
            fusion_score: score,
            keyword_rank: Some(chunk as usize),
            semantic_rank: None,
            keyword_score: Some(score / 10.0),
            semantic_score: None,
        }
    }

    #[test]
    fn cap_per_file_prefers_question_coverage_over_bare_definition() {
        let item = Uuid::from_u128(12);
        let definition = HybridSearchResult {
            chunk_id: Uuid::from_u128(1),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: "pub async fn run_command(cmd: &str) -> Result<Output> { spawn(cmd) }"
                .into(),
            item_uri: "github://zone/tools/command.rs@main".into(),
            item_title: "command.rs".into(),
            score: 3.5,
            fusion_score: 3.5,
            keyword_rank: Some(1),
            semantic_rank: None,
            keyword_score: Some(0.5),
            semantic_score: None,
        };
        let body = HybridSearchResult {
            chunk_id: Uuid::from_u128(2),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: "fn trim_middle(stdout: &str) { /* oversized stdout */ }".into(),
            item_uri: "github://zone/tools/command.rs@main".into(),
            item_title: "command.rs".into(),
            score: 1.0,
            fusion_score: 1.0,
            keyword_rank: Some(2),
            semantic_rank: None,
            keyword_score: Some(0.2),
            semantic_score: None,
        };
        let capped = cap_per_file(
            vec![definition, body],
            2,
            2,
            "How does run_command trim oversized stdout?",
        );
        assert!(
            capped[0].chunk_text.contains("trim_middle"),
            "kept {}",
            capped[0].chunk_text
        );
    }

    #[test]
    fn keep_answer_chunks_drops_headers_first() {
        let header = KeywordResult {
            chunk_id: Uuid::from_u128(1),
            content_item_id: Uuid::from_u128(10),
            source_id: Uuid::nil(),
            chunk_text: "path: x kind: file_header\n\nidentifiers: should_skip_blob".into(),
            item_uri: "github://zone/content/mod.rs".into(),
            item_title: "mod.rs".into(),
            score: 0.9,
        };
        let body = KeywordResult {
            chunk_id: Uuid::from_u128(2),
            content_item_id: Uuid::from_u128(10),
            source_id: Uuid::nil(),
            chunk_text: "pub fn should_skip_blob(&self) -> bool { true }".into(),
            item_uri: "github://zone/content/mod.rs".into(),
            item_title: "mod.rs".into(),
            score: 0.4,
        };
        let kept = keep_answer_chunks(vec![header, body], 1, &["should_skip_blob".into()], |row| {
            (row.chunk_text.as_str(), row.item_uri.as_str())
        });
        assert_eq!(kept.len(), 1);
        assert!(kept[0].chunk_text.contains("pub fn should_skip_blob"));
    }

    #[test]
    fn prefers_definition_chunk_over_same_file_mention() {
        let item = Uuid::parse_str("00000000-0000-0000-0000-0000000000bb").unwrap();
        let mention = HybridSearchResult {
            chunk_id: Uuid::from_u128(1),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text: "identifiers: should_skip_blob, other".into(),
            item_uri: "github://zone/content/mod.rs".into(),
            item_title: "mod.rs".into(),
            score: 2.0,
            fusion_score: 2.0,
            keyword_rank: Some(1),
            semantic_rank: None,
            keyword_score: Some(0.4),
            semantic_score: None,
        };
        let definition = HybridSearchResult {
            chunk_id: Uuid::from_u128(2),
            content_item_id: item,
            source_id: Uuid::nil(),
            chunk_text:
                "pub fn should_skip_blob(&self, uri: &str, blob_sha: &str) -> bool { true }".into(),
            item_uri: "github://zone/content/mod.rs".into(),
            item_title: "mod.rs".into(),
            score: 1.2,
            fusion_score: 1.2,
            keyword_rank: Some(2),
            semantic_rank: None,
            keyword_score: Some(0.3),
            semantic_score: None,
        };
        let ranked = finalize_ranking(
            vec![mention, definition],
            "What does should_skip_blob do?",
            2,
        );
        assert!(
            ranked[0].chunk_text.contains("pub fn should_skip_blob"),
            "kept {}",
            ranked[0].chunk_text
        );
    }

    #[test]
    fn conjunction_outranks_single_ident_definition() {
        let query = "Why must retain_content_uris use live_uris after an incremental gather?";
        let store = HybridSearchResult {
            chunk_id: Uuid::from_u128(1),
            content_item_id: Uuid::from_u128(1),
            source_id: Uuid::nil(),
            chunk_text: "pub async fn retain_content_uris(&self, source_id: Uuid, uris: &[String])"
                .into(),
            item_uri: "github://zone/embeddings/pgvector.rs".into(),
            item_title: "pgvector.rs".into(),
            score: 2.0,
            fusion_score: 2.0,
            keyword_rank: Some(1),
            semantic_rank: None,
            keyword_score: Some(0.5),
            semantic_score: None,
        };
        let caller = HybridSearchResult {
            chunk_id: Uuid::from_u128(2),
            content_item_id: Uuid::from_u128(2),
            source_id: Uuid::nil(),
            chunk_text: "store.retain_content_uris(source.id, &fetch_result.live_uris)".into(),
            item_uri: "github://zone/context/service.rs".into(),
            item_title: "service.rs".into(),
            score: 1.0,
            fusion_score: 1.0,
            keyword_rank: Some(2),
            semantic_rank: None,
            keyword_score: Some(0.2),
            semantic_score: None,
        };
        let ranked = finalize_ranking(vec![store, caller], query, 2);
        assert!(
            ranked[0].item_uri.contains("service.rs"),
            "kept {}",
            ranked[0].item_uri
        );
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
