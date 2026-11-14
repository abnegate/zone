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

pub use adapters::AdapterRegistry;
pub use adapters::ProgressCallback;
pub use adapters::SourceAdapter;
pub use adapters::TextAdapter;
pub use content::ContentCategory;
pub use content::ContentChunk;
pub use content::ContentItem;
pub use content::ContentMetadata;
pub use content::FetchConfig;
pub use content::FetchResult;
pub use content::FetchStats;
pub use content::FetchStrategy;
pub use context::AssembledContext;
pub use context::ContextBuilder;
pub use context::ContextConfig;
pub use embeddings::CrossEncoder;
pub use embeddings::Embedding;
pub use embeddings::EmbeddingService;
pub use embeddings::VectorStore;
pub use embeddings::ann_candidate_limit;
pub use embeddings::configure_ann_connection;
pub use embeddings::default_ranker;
pub use embeddings::hybrid_search;
pub use embeddings::hybrid_search_filtered;
pub use embeddings::keyword_only_search;
pub use embeddings::probe_cross_encoder;
pub use embeddings::score_hit;
pub use embeddings::semantic_only_search;
pub use error::ContextError;
pub use error::Result;
pub use heuristics::ActionabilityScore;
pub use heuristics::ContentCategorization;
pub use heuristics::ExtractedEntities;
pub use heuristics::HeuristicAnalysis;
pub use heuristics::Priority;
pub use heuristics::QualityScore;
pub use heuristics::RelevanceScore;
pub use heuristics::Sentiment;
pub use heuristics::Topic;
pub use stream::GatheringEvent;
