//! Context service - orchestrates the full context pipeline
//!
//! The ContextService integrates all components to provide a complete
//! content gathering, analysis, embedding, and search solution.

use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::adapters::{AdapterRegistry, ProgressCallback};
use crate::content::{
    CHUNK_OVERLAP_TOKENS, ContentChunk, ContentItem, FetchConfig, MAX_CHUNK_TOKENS,
    embed_char_budget, smart_chunk, split_for_embedding,
};
use crate::context::{AssembledContext, ContextBuilder, ContextConfig};
use crate::embeddings::{
    Embedding, EmbeddingService, HybridSearchConfig, HybridSearchResult, PgVectorStore,
    SearchFilters, VectorStore, embed_query_text, hybrid_search, keyword_only_search,
    semantic_only_search,
};
use crate::error::{ContextError, Result};
use crate::heuristics::{HeuristicAnalysis, HeuristicAnalyzer};
use crate::stream::{AnalysisStage, GatheringCallback, GatheringEvent};
use zone_core::Source;

/// Result of a gathering operation
#[derive(Debug, Clone, Default)]
pub struct GatheringResult {
    /// Number of sources processed
    pub sources_processed: usize,
    /// Number of content items gathered
    pub items_gathered: usize,
    /// Number of items analyzed
    pub items_analyzed: usize,
    /// Number of embeddings created
    pub embeddings_created: usize,
    /// Items skipped because their content hash already matched the index
    pub items_unchanged: usize,
    /// Errors encountered (source_id, error message)
    pub errors: Vec<(Uuid, String)>,
    /// Total duration in milliseconds
    pub duration_ms: u64,
}

/// Search result with heuristic analysis
#[derive(Debug, Clone)]
pub struct SearchResultWithAnalysis {
    /// Chunk ID
    pub chunk_id: Uuid,
    /// Content item ID
    pub content_item_id: Uuid,
    /// Source ID
    pub source_id: Uuid,
    /// Semantic cosine when available, otherwise the ranking score for this mode
    pub similarity: f32,
    /// Reciprocal rank fusion score. This is *not* cosine; rank-1 is ~0.01.
    pub rrf_score: Option<f32>,
    /// Cosine similarity (0.0-1.0) when the semantic leg contributed
    pub semantic_score: Option<f32>,
    /// PostgreSQL `ts_rank` when the keyword leg contributed
    pub keyword_score: Option<f32>,
    /// Chunk text
    pub chunk_text: String,
    /// Content item URI
    pub item_uri: String,
    /// Content item title
    pub item_title: String,
    /// Heuristic analysis (if loaded)
    pub analysis: Option<HeuristicAnalysis>,
}

impl SearchResultWithAnalysis {
    fn from_search_result(r: crate::embeddings::SearchResult) -> Self {
        Self {
            chunk_id: r.chunk_id,
            content_item_id: r.content_item_id,
            source_id: r.source_id,
            similarity: r.similarity,
            rrf_score: None,
            semantic_score: Some(r.similarity),
            keyword_score: None,
            chunk_text: r.chunk_text,
            item_uri: r.item_uri,
            item_title: r.item_title,
            analysis: None,
        }
    }

    fn from_hybrid(r: HybridSearchResult, rrf: bool) -> Self {
        Self {
            chunk_id: r.chunk_id,
            content_item_id: r.content_item_id,
            source_id: r.source_id,
            similarity: r.semantic_score.unwrap_or(r.score),
            rrf_score: rrf.then_some(r.score),
            semantic_score: r.semantic_score,
            keyword_score: r.keyword_score,
            chunk_text: r.chunk_text,
            item_uri: r.item_uri,
            item_title: r.item_title,
            analysis: None,
        }
    }
}

enum StoreOutcome {
    Embedded(usize),
    Unchanged,
}

/// Context service that orchestrates the full pipeline
pub struct ContextService {
    #[allow(dead_code)] // Used for future database operations
    pool: PgPool,
    adapter_registry: Arc<AdapterRegistry>,
    embedding_service: Arc<dyn EmbeddingService>,
    vector_store: PgVectorStore,
}

impl ContextService {
    /// Create a new context service
    pub fn new(
        pool: PgPool,
        adapter_registry: Arc<AdapterRegistry>,
        embedding_service: Arc<dyn EmbeddingService>,
    ) -> Self {
        let vector_store = PgVectorStore::new(pool.clone());
        Self {
            pool,
            adapter_registry,
            embedding_service,
            vector_store,
        }
    }

    /// Gather content from sources, analyze, embed, and store
    ///
    /// This is the main entry point for the gathering pipeline. It:
    /// 1. Fetches content from each source using adapters
    /// 2. Chunks the content for embedding
    /// 3. Generates embeddings and stores them
    /// 4. Runs heuristic analysis
    /// 5. Streams progress events via callback
    pub async fn gather(
        &self,
        sources: &[Source],
        fetch_config: FetchConfig,
        callback: &dyn GatheringCallback,
    ) -> Result<GatheringResult> {
        let start = std::time::Instant::now();
        let gathering_id = Uuid::new_v4();

        // Emit started event
        callback.on_event(GatheringEvent::Started {
            gathering_id,
            source_count: sources.len(),
            timestamp: Utc::now(),
        });

        let mut result = GatheringResult::default();
        let mut total_items = Vec::new();

        // Process each source
        for source in sources {
            result.sources_processed += 1;

            // Get the appropriate adapter
            let adapter = match self.adapter_registry.get_for_source(source) {
                Ok(adapter) => adapter,
                Err(e) => {
                    let error = format!("No adapter for source type: {}", e);
                    result.errors.push((source.id, error.clone()));
                    callback.on_event(GatheringEvent::SourceError {
                        gathering_id,
                        source_id: source.id,
                        error,
                    });
                    continue;
                }
            };

            // Emit source started event
            callback.on_event(GatheringEvent::SourceStarted {
                gathering_id,
                source_id: source.id,
                source_name: source.name.clone(),
                source_type: format!("{:?}", source.source_type),
            });

            // Verify source access
            if let Err(e) = adapter.verify(source).await {
                let error = format!("Source verification failed: {}", e);
                result.errors.push((source.id, error.clone()));
                callback.on_event(GatheringEvent::SourceError {
                    gathering_id,
                    source_id: source.id,
                    error,
                });
                continue;
            }

            let mut source_fetch_config = fetch_config.clone();
            if source_fetch_config.index_mode {
                match self.vector_store.list_indexed_blobs(source.id).await {
                    Ok(blobs) => source_fetch_config.known_blobs = blobs,
                    Err(e) => tracing::warn!(
                        source_id = %source.id,
                        error = %e,
                        "failed to load indexed blobs for incremental fetch"
                    ),
                }
                match self.vector_store.load_sync_version(source.id).await {
                    Ok(version) => source_fetch_config.last_version = version,
                    Err(e) => tracing::warn!(
                        source_id = %source.id,
                        error = %e,
                        "failed to load source sync version"
                    ),
                }
            }

            // Estimate tokens and decide strategy
            let estimated_tokens = match adapter.estimate_tokens(source).await {
                Ok(tokens) => tokens,
                Err(e) => {
                    tracing::warn!("Failed to estimate tokens for source {}: {}", source.id, e);
                    0
                }
            };
            let strategy = source_fetch_config.fetch_strategy(estimated_tokens);
            tracing::debug!(
                source_id = %source.id,
                estimated_tokens,
                index_mode = source_fetch_config.index_mode,
                allow_metadata_only = source_fetch_config.allow_metadata_only,
                known_blobs = source_fetch_config.known_blobs.len(),
                ?strategy,
                "chose fetch strategy"
            );

            // Create progress adapter
            let progress = SourceProgressAdapter {
                gathering_id,
                source_id: source.id,
                callback,
            };

            // Fetch content
            let fetch_result = match adapter
                .fetch(source, &source_fetch_config, strategy, &progress)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let error = format!("Fetch failed: {}", e);
                    result.errors.push((source.id, error.clone()));
                    callback.on_event(GatheringEvent::SourceError {
                        gathering_id,
                        source_id: source.id,
                        error,
                    });
                    continue;
                }
            };

            result.items_gathered += fetch_result.items.len();
            result.items_unchanged += fetch_result.stats.items_skipped;
            if source_fetch_config.index_mode {
                let uris = if fetch_result.live_uris.is_empty() {
                    fetch_result
                        .items
                        .iter()
                        .map(|item| item.uri.clone())
                        .collect()
                } else {
                    fetch_result.live_uris.clone()
                };
                if let Err(e) = self
                    .vector_store
                    .retain_content_uris(source.id, &uris)
                    .await
                {
                    tracing::warn!(
                        "Failed to drop stale indexed files for source {}: {}",
                        source.id,
                        e
                    );
                }
            }
            if let Some(version) = fetch_result.version.clone()
                && let Err(e) = self
                    .vector_store
                    .save_sync_version(source.id, &version)
                    .await
            {
                tracing::warn!(
                    source_id = %source.id,
                    error = %e,
                    "failed to persist source sync version"
                );
            }
            total_items.extend(fetch_result.items);

            // Emit source completed
            callback.on_event(GatheringEvent::SourceCompleted {
                gathering_id,
                source_id: source.id,
                items_count: fetch_result.stats.items_fetched,
                token_count: fetch_result.stats.total_tokens,
                duration_ms: fetch_result.stats.duration_ms,
            });
        }

        // Emit analysis started
        callback.on_event(GatheringEvent::AnalysisStarted {
            gathering_id,
            total_items: total_items.len(),
        });

        // Process items: chunk, embed, analyze, store
        for (idx, item) in total_items.iter().enumerate() {
            // Skip metadata-only items or items without content
            let content = match item.content.as_ref() {
                Some(c) if !item.metadata_only => c,
                _ => continue,
            };

            let text_chunks = smart_chunk(
                content,
                &item.content_type,
                item.metadata.extension.as_deref(),
                Some(item.uri.as_str()),
                MAX_CHUNK_TOKENS,
                CHUNK_OVERLAP_TOKENS,
            );
            let chunks: Vec<ContentChunk> = text_chunks
                .into_iter()
                .map(|tc| {
                    ContentChunk::new(item.id, tc.index, tc.text, tc.start_offset, tc.end_offset)
                })
                .collect();

            if chunks.is_empty() {
                continue;
            }

            // Emit embedding progress
            callback.on_event(GatheringEvent::AnalysisProgress {
                gathering_id,
                analyzed_count: idx,
                total_count: total_items.len(),
                current_stage: AnalysisStage::Embedding,
            });

            match self.store_and_embed(item, &chunks).await {
                Ok(StoreOutcome::Unchanged) => {
                    result.items_unchanged += 1;
                }
                Ok(StoreOutcome::Embedded(count)) => {
                    result.embeddings_created += count;
                }
                Err(e) => {
                    tracing::warn!(
                        source_id = %item.source_id,
                        uri = %item.uri,
                        error = %e,
                        "Store/embed failed"
                    );
                    result
                        .errors
                        .push((item.source_id, format!("Store/embed failed: {}", e)));
                    continue;
                }
            }

            // Run heuristic analysis
            callback.on_event(GatheringEvent::AnalysisProgress {
                gathering_id,
                analyzed_count: idx,
                total_count: total_items.len(),
                current_stage: AnalysisStage::EntityExtraction,
            });

            let _analysis = HeuristicAnalyzer::analyze(
                item.id,
                content,
                &item.content_type,
                &format!("{:?}", item.category),
                item.modified_at,
            );

            result.items_analyzed += 1;

            // Emit embedding progress
            callback.on_event(GatheringEvent::EmbeddingProgress {
                gathering_id,
                embedded_count: result.embeddings_created,
                total_count: total_items.len(),
            });
        }

        result.duration_ms = start.elapsed().as_millis() as u64;

        // Emit completed event
        callback.on_event(GatheringEvent::Completed {
            gathering_id,
            total_items: result.items_gathered,
            total_tokens: total_items.iter().map(|i| i.token_count).sum(),
            duration_ms: result.duration_ms,
            timestamp: Utc::now(),
        });

        Ok(result)
    }

    /// Search for relevant content given a query (semantic only - legacy method)
    ///
    /// Embeds the query and searches the vector store for similar chunks.
    /// For better results, consider using `search_hybrid` instead.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<SearchResultWithAnalysis>> {
        // Generate query embedding
        let query_embedding = self
            .embedding_service
            .embed(&embed_query_text(self.embedding_service.model(), query))
            .await?;

        // Search vector store
        let results = self
            .vector_store
            .search(&query_embedding, limit, None, filters)
            .await?;

        let enriched_results = results
            .into_iter()
            .map(SearchResultWithAnalysis::from_search_result)
            .collect();

        Ok(enriched_results)
    }

    /// Hybrid search combining keyword and semantic retrieval (RECOMMENDED)
    ///
    /// Uses Reciprocal Rank Fusion to combine:
    /// - PostgreSQL full-text search (keyword matching)
    /// - pgvector similarity search (semantic matching)
    ///
    /// This provides better results than semantic-only search, especially for:
    /// - Exact term matching (variable names, function names, error codes)
    /// - Technical queries with specific terminology
    /// - Queries where both conceptual and literal matching matter
    pub async fn search_hybrid(
        &self,
        query: &str,
        limit: usize,
        filters: Option<SearchFilters>,
        config: Option<HybridSearchConfig>,
    ) -> Result<Vec<SearchResultWithAnalysis>> {
        // Generate query embedding for semantic search
        let query_embedding = self
            .embedding_service
            .embed(&embed_query_text(self.embedding_service.model(), query))
            .await?;

        // Use default config if not provided
        let hybrid_config = config.unwrap_or_default();

        // Extract filter parameters
        let workspace_id = filters.as_ref().and_then(|f| f.workspace_id);
        let source_ids = filters.as_ref().and_then(|f| f.source_ids.as_deref());

        // Perform hybrid search
        let results = hybrid_search(
            &self.pool,
            query,
            &query_embedding,
            limit,
            workspace_id,
            source_ids,
            &hybrid_config,
        )
        .await?;

        // Convert to SearchResultWithAnalysis
        let enriched_results = results
            .into_iter()
            .map(|r| SearchResultWithAnalysis::from_hybrid(r, true))
            .collect();

        Ok(enriched_results)
    }

    /// Keyword-only search (no semantic component)
    ///
    /// Uses PostgreSQL full-text search only. Useful for:
    /// - Debugging keyword search performance
    /// - Cases where embeddings are not available
    /// - Finding exact technical terms
    pub async fn search_keyword_only(
        &self,
        query: &str,
        limit: usize,
        filters: Option<SearchFilters>,
        min_score: f32,
    ) -> Result<Vec<SearchResultWithAnalysis>> {
        let results = keyword_only_search(&self.pool, query, limit, filters, min_score).await?;

        let enriched_results = results
            .into_iter()
            .map(|r| SearchResultWithAnalysis::from_hybrid(r, false))
            .collect();

        Ok(enriched_results)
    }

    /// Semantic-only search (no keyword component)
    ///
    /// Uses pgvector similarity search only. This is the same as the `search` method
    /// but provides consistency with the hybrid/keyword variants.
    pub async fn search_semantic_only(
        &self,
        query: &str,
        limit: usize,
        filters: Option<SearchFilters>,
        min_similarity: f32,
    ) -> Result<Vec<SearchResultWithAnalysis>> {
        // Generate query embedding
        let query_embedding = self
            .embedding_service
            .embed(&embed_query_text(self.embedding_service.model(), query))
            .await?;

        let results =
            semantic_only_search(&self.pool, &query_embedding, limit, filters, min_similarity)
                .await?;

        let enriched_results = results
            .into_iter()
            .map(|r| SearchResultWithAnalysis::from_hybrid(r, false))
            .collect();

        Ok(enriched_results)
    }

    /// Build context for a prompt using search results
    ///
    /// Combines search and context assembly into a single operation.
    pub async fn build_context(
        &self,
        query: &str,
        config: &ContextConfig,
    ) -> Result<AssembledContext> {
        // Search for relevant content
        let filters = SearchFilters {
            source_ids: config.source_ids.clone(),
            workspace_id: None,
            categories: None,
            min_quality: None,
            since: None,
        };

        let results = self
            .search(query, config.max_items * 2, Some(filters))
            .await?;

        // Convert to SearchResult format for ContextBuilder
        let search_results: Vec<crate::embeddings::SearchResult> = results
            .into_iter()
            .map(|r| crate::embeddings::SearchResult {
                chunk_id: r.chunk_id,
                content_item_id: r.content_item_id,
                source_id: r.source_id,
                similarity: r.similarity,
                chunk_text: r.chunk_text,
                item_uri: r.item_uri,
                item_title: r.item_title,
            })
            .collect();

        // Build context using ContextBuilder
        let builder = ContextBuilder::new(config.clone());
        let dummy_lookup = |_: Uuid| None;
        let context = builder.build_from_results(&search_results, &dummy_lookup);

        Ok(context)
    }

    /// Analyze a specific content item
    ///
    /// Retrieves the item from storage and runs heuristic analysis.
    ///
    /// Note: This is a simplified implementation. In practice, heuristic analysis
    /// is performed during content indexing and stored in the database. This method
    /// re-analyzes the content on-demand if needed for debugging or verification.
    pub async fn analyze_content(&self, content_item_id: Uuid) -> Result<HeuristicAnalysis> {
        use crate::heuristics::HeuristicAnalyzer;

        // Fetch the content item from vector store
        let item = self
            .vector_store
            .get_content_item(content_item_id)
            .await?
            .ok_or_else(|| {
                ContextError::VectorStore(format!("Content item {} not found", content_item_id))
            })?;

        // Get content text for analysis
        let text = item.content.as_deref().unwrap_or("");

        // Perform comprehensive heuristic analysis
        // Note: We use "unknown" for source_type since we don't have direct access
        // to the Source record from the context service. In production, the source
        // type would be stored in content metadata or fetched separately.
        let analysis = HeuristicAnalyzer::analyze(
            content_item_id,
            text,
            &item.content_type,
            "unknown", // Source type not available without additional query
            item.modified_at,
        );

        Ok(analysis)
    }

    /// Persist a content item, its chunks, and embeddings in FK order
    async fn store_and_embed(
        &self,
        item: &ContentItem,
        chunks: &[ContentChunk],
    ) -> Result<StoreOutcome> {
        if let Some((existing_id, existing_hash)) = self
            .vector_store
            .content_item_hash(item.source_id, &item.uri)
            .await?
            && existing_hash == item.content_hash()
            && self
                .vector_store
                .content_item_has_embeddings(existing_id)
                .await?
        {
            return Ok(StoreOutcome::Unchanged);
        }

        let item_id = self.vector_store.store_content_item(item).await?;
        let max_chars = embed_char_budget(self.embedding_service.max_tokens());
        let persisted_chunks = expand_chunks_for_embedding(item_id, chunks, max_chars);

        self.vector_store
            .replace_content_chunks(item_id, &persisted_chunks)
            .await?;

        let pairs = self.embed_chunks_resilient(item, &persisted_chunks).await?;
        if pairs.is_empty() {
            return Err(ContextError::Embedding(format!(
                "no chunks could be embedded for {}",
                item.uri
            )));
        }

        let embeddings: Vec<Embedding> = pairs
            .into_iter()
            .map(|(chunk, vector)| {
                Embedding::new(
                    chunk.id,
                    item_id,
                    item.source_id,
                    vector,
                    self.embedding_service.model(),
                )
            })
            .collect();

        self.vector_store.store_batch(&embeddings).await?;
        Ok(StoreOutcome::Embedded(embeddings.len()))
    }

    async fn embed_chunks_resilient(
        &self,
        item: &ContentItem,
        chunks: &[ContentChunk],
    ) -> Result<Vec<(ContentChunk, Vec<f32>)>> {
        let texts: Vec<&str> = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        match self.embedding_service.embed_batch(&texts).await {
            Ok(vectors) if vectors.len() == chunks.len() => {
                Ok(chunks.iter().cloned().zip(vectors).collect())
            }
            Ok(_) => Err(ContextError::Embedding(
                "embedding batch size did not match chunk count".to_string(),
            )),
            Err(err) if err.is_fatal_embedding() => Err(err),
            Err(err) => {
                tracing::warn!(
                    uri = %item.uri,
                    error = %err,
                    "batch embed failed; retrying chunks individually"
                );
                let mut ok = Vec::new();
                for chunk in chunks {
                    match self.embedding_service.embed(&chunk.text).await {
                        Ok(vector) => ok.push((chunk.clone(), vector)),
                        Err(chunk_err) if chunk_err.is_context_length() => {
                            tracing::warn!(
                                uri = %item.uri,
                                chunk_index = chunk.chunk_index,
                                chars = chunk.text.chars().count(),
                                "skipping chunk that exceeds embedding context"
                            );
                        }
                        Err(chunk_err) if chunk_err.is_fatal_embedding() => return Err(chunk_err),
                        Err(chunk_err) => {
                            tracing::warn!(
                                uri = %item.uri,
                                chunk_index = chunk.chunk_index,
                                error = %chunk_err,
                                "skipping chunk after embed failure"
                            );
                        }
                    }
                }
                Ok(ok)
            }
        }
    }
}

fn expand_chunks_for_embedding(
    item_id: Uuid,
    chunks: &[ContentChunk],
    max_chars: usize,
) -> Vec<ContentChunk> {
    let mut out = Vec::new();
    for chunk in chunks {
        if chunk.text.chars().count() <= max_chars {
            out.push(chunk.clone());
            continue;
        }
        for piece in split_for_embedding(&chunk.text, max_chars) {
            out.push(ContentChunk::new(
                item_id,
                0,
                piece,
                chunk.start_offset,
                chunk.end_offset,
            ));
        }
    }
    out.into_iter()
        .enumerate()
        .map(|(index, mut chunk)| {
            chunk.content_item_id = item_id;
            chunk.chunk_index = index;
            chunk
        })
        .collect()
}

/// Adapter to convert ProgressCallback to GatheringCallback
struct SourceProgressAdapter<'a> {
    gathering_id: Uuid,
    source_id: Uuid,
    callback: &'a dyn GatheringCallback,
}

impl<'a> ProgressCallback for SourceProgressAdapter<'a> {
    fn on_item(&self, _item: &ContentItem) {
        // Could emit item-level events if needed
    }

    fn on_progress(&self, current: usize, total: Option<usize>) {
        self.callback.on_event(GatheringEvent::SourceProgress {
            gathering_id: self.gathering_id,
            source_id: self.source_id,
            items_fetched: current,
            estimated_total: total,
            tokens_fetched: 0, // Not tracked at this level
        });
    }

    fn on_message(&self, _message: &str) {
        // Could emit message events if needed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gathering_result_default() {
        let result = GatheringResult::default();
        assert_eq!(result.sources_processed, 0);
        assert_eq!(result.items_gathered, 0);
        assert_eq!(result.items_analyzed, 0);
        assert_eq!(result.embeddings_created, 0);
        assert_eq!(result.items_unchanged, 0);
        assert!(result.errors.is_empty());
        assert_eq!(result.duration_ms, 0);
    }

    #[test]
    fn test_search_result_with_analysis_creation() {
        let result = SearchResultWithAnalysis {
            chunk_id: Uuid::new_v4(),
            content_item_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            similarity: 0.85,
            rrf_score: None,
            semantic_score: Some(0.85),
            keyword_score: None,
            chunk_text: "Test chunk".to_string(),
            item_uri: "/test.txt".to_string(),
            item_title: "Test".to_string(),
            analysis: None,
        };

        assert_eq!(result.similarity, 0.85);
        assert!(result.analysis.is_none());
    }

    // Note: Integration tests requiring database connections should be in
    // the zone_context/tests directory with proper test fixtures.
    // These unit tests verify the data structures and basic functionality.
}
