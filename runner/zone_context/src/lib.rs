//! Zone Context - Context gathering and heuristic analysis for Zone AI agents
//!
//! This crate provides intelligent context gathering from multiple source types
//! with deep heuristic analysis for AI agent workflows.
//!
//! # Features
//!
//! - **Source Adapters**: Unified interface for fetching content from various sources
//!   (GitHub, GitLab, filesystem, calendar, email, Slack, Discord, web, text)
//!
//! - **Intelligent Sizing**: Automatic content sizing decisions based on token budgets
//!   - Full content for small sources (<100k tokens)
//!   - Metadata-only for large sources
//!   - Incremental fetching on continuation
//!
//! - **Heuristic Analysis**: Deep analysis of gathered content
//!   - Relevance scoring via embeddings
//!   - Entity extraction (people, dates, code refs, URLs)
//!   - Content categorization (topic, sentiment, priority)
//!   - Quality metrics (freshness, reliability, density)
//!
//! - **Vector Storage**: PostgreSQL with pgvector for semantic search
//!
//! - **Context Assembly**: Build optimized context windows for LLM prompts
//!
//! # Architecture
//!
//! ```text
//! Sources → Adapters → Content → Analysis → Embeddings → Context
//!     ↓         ↓          ↓          ↓           ↓          ↓
//! GitHub   SourceAdapter  ContentItem  Heuristics  pgvector  ContextBuilder
//! GitLab   FetchConfig    ContentChunk Entities              AssembledContext
//! Files    FetchResult    Tokenizer    Categories
//! etc.                    Chunker      Quality
//! ```

pub mod adapters;
pub mod content;
pub mod context;
pub mod db;
pub mod embeddings;
pub mod error;
pub mod heuristics;
pub mod stream;

// Re-export commonly used types
pub use error::{ContextError, Result};

// Content types
pub use content::{
    ContentCategory, ContentChunk, ContentItem, ContentMetadata, FetchConfig, FetchResult,
    FetchStats, FetchStrategy,
};

// Adapter types
pub use adapters::{AdapterRegistry, ProgressCallback, SourceAdapter, TextAdapter};

// Embedding types
pub use embeddings::{
    CrossEncoder, Embedding, EmbeddingService, HybridSearchConfig, HybridSearchResult,
    OllamaCrossEncoder, RewrittenQuery, VectorStore, default_ranker, embed_query_text,
    hybrid_search, hybrid_search_filtered, identifier_match_boost, keyword_only_search,
    probe_cross_encoder, rewrite_query, score_hit, semantic_only_search,
    configure_ann_connection, ann_candidate_limit,
};

// Heuristics types
pub use heuristics::{
    ActionabilityScore, ContentCategorization, ExtractedEntities, HeuristicAnalysis, Priority,
    QualityScore, RelevanceScore, Sentiment, Topic,
};

// Context types
pub use context::{AssembledContext, ContextBuilder, ContextConfig};

// Streaming types
pub use stream::GatheringEvent;
