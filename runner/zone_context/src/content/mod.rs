//! Content processing and types
//!
//! This module provides the core types for representing content gathered from sources,
//! along with utilities for tokenization, chunking, and intelligent sizing.

mod code_chunker;
mod sizing;
mod tokenizer;

pub use code_chunker::*;
pub use sizing::*;
pub use tokenizer::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Re-export SourceCategory as ContentCategory for domain clarity
pub use zone_core::SourceCategory as ContentCategory;

/// A single content item fetched from a source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentItem {
    /// Unique identifier for this content item
    pub id: Uuid,
    /// Source this content came from
    pub source_id: Uuid,
    /// Category of content
    pub category: ContentCategory,
    /// URI or path identifier within the source
    pub uri: String,
    /// Display title
    pub title: String,
    /// The actual content (None if metadata_only)
    pub content: Option<String>,
    /// Content MIME type (text/plain, text/markdown, etc.)
    pub content_type: String,
    /// Estimated token count
    pub token_count: usize,
    /// Whether this is metadata-only (content omitted due to size)
    pub metadata_only: bool,
    /// Additional metadata
    pub metadata: ContentMetadata,
    /// When the content was last modified at the source
    pub modified_at: Option<DateTime<Utc>>,
    /// When we fetched this content
    pub fetched_at: DateTime<Utc>,
}

impl ContentItem {
    /// Create a new content item
    pub fn new(
        source_id: Uuid,
        category: ContentCategory,
        uri: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id,
            category,
            uri: uri.into(),
            title: title.into(),
            content: None,
            content_type: "text/plain".to_string(),
            token_count: 0,
            metadata_only: true,
            metadata: ContentMetadata::default(),
            modified_at: None,
            fetched_at: Utc::now(),
        }
    }

    /// Set the content and update token count
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        let content = content.into();
        self.token_count = estimate_tokens(&content);
        self.content = Some(content);
        self.metadata_only = false;
        self
    }

    /// Set content type
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = content_type.into();
        self
    }

    /// Set metadata
    pub fn with_metadata(mut self, metadata: ContentMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set modified timestamp
    pub fn with_modified_at(mut self, modified_at: DateTime<Utc>) -> Self {
        self.modified_at = Some(modified_at);
        self
    }

    /// Get content hash for deduplication
    pub fn content_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        if let Some(content) = &self.content {
            hasher.update(content.as_bytes());
        } else {
            hasher.update(self.uri.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

/// Metadata extracted from content
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentMetadata {
    /// Author or creator
    pub author: Option<String>,
    /// Language (for code files)
    pub language: Option<String>,
    /// File extension
    pub extension: Option<String>,
    /// Size in bytes
    pub size_bytes: Option<usize>,
    /// Line count
    pub line_count: Option<usize>,
    /// Commit hash (for git sources)
    pub commit_hash: Option<String>,
    /// Branch name (for git sources)
    pub branch: Option<String>,
    /// Event start time (for calendar)
    pub event_start: Option<DateTime<Utc>>,
    /// Event end time (for calendar)
    pub event_end: Option<DateTime<Utc>>,
    /// Recipients (for mail)
    pub recipients: Vec<String>,
    /// Labels/tags
    pub labels: Vec<String>,
    /// URL reference
    pub url: Option<String>,
    /// Additional source-specific metadata
    pub extra: serde_json::Value,
}

impl ContentMetadata {
    /// Create metadata with author
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Create metadata with language
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Create metadata with URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// A chunk of content ready for embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentChunk {
    /// Unique identifier for this chunk
    pub id: Uuid,
    /// Parent content item ID
    pub content_item_id: Uuid,
    /// Index of this chunk within the parent (0-based)
    pub chunk_index: usize,
    /// The chunk text
    pub text: String,
    /// Token count of this chunk
    pub token_count: usize,
    /// Start byte offset in original content
    pub start_offset: usize,
    /// End byte offset in original content
    pub end_offset: usize,
}

impl ContentChunk {
    /// Create a new chunk
    pub fn new(
        content_item_id: Uuid,
        chunk_index: usize,
        text: impl Into<String>,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        let text = text.into();
        let token_count = estimate_tokens(&text);
        Self {
            id: Uuid::new_v4(),
            content_item_id,
            chunk_index,
            text,
            token_count,
            start_offset,
            end_offset,
        }
    }
}

/// Configuration for content fetching
#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// Maximum tokens to fetch (full content threshold)
    pub max_tokens: usize,
    /// Token budget for this fetch operation
    pub token_budget: usize,
    /// Since timestamp for incremental fetching
    pub since: Option<DateTime<Utc>>,
    /// File patterns to include (glob syntax)
    pub include_patterns: Vec<String>,
    /// File patterns to exclude (glob syntax)
    pub exclude_patterns: Vec<String>,
    /// Whether to fetch metadata only for large items
    pub allow_metadata_only: bool,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            max_tokens: 100_000,
            token_budget: 100_000,
            since: None,
            include_patterns: vec![],
            exclude_patterns: vec![
                "node_modules/**".to_string(),
                ".git/**".to_string(),
                "target/**".to_string(),
                "dist/**".to_string(),
                "build/**".to_string(),
                "*.lock".to_string(),
                "*.min.js".to_string(),
                "*.min.css".to_string(),
            ],
            allow_metadata_only: true,
        }
    }
}

/// Strategy for fetching content
#[derive(Debug, Clone)]
pub enum FetchStrategy {
    /// Fetch everything (source fits in budget)
    Full,
    /// Fetch only metadata (source too large)
    MetadataOnly,
    /// Fetch partial content up to max_tokens
    Partial { max_tokens: usize },
    /// Progressive loading by priority
    Progressive { priority_order: Vec<String> },
}

/// Result of a fetch operation
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// Unique identifier for this fetch
    pub fetch_id: Uuid,
    /// Source ID
    pub source_id: Uuid,
    /// Fetched content items
    pub items: Vec<ContentItem>,
    /// Whether this was incremental (since a previous timestamp)
    pub is_incremental: bool,
    /// Timestamp of this fetch
    pub fetched_at: DateTime<Utc>,
    /// Fetch statistics
    pub stats: FetchStats,
}

impl FetchResult {
    /// Create a new fetch result
    pub fn new(source_id: Uuid, is_incremental: bool) -> Self {
        Self {
            fetch_id: Uuid::new_v4(),
            source_id,
            items: Vec::new(),
            is_incremental,
            fetched_at: Utc::now(),
            stats: FetchStats::default(),
        }
    }

    /// Add an item to the result
    pub fn add_item(&mut self, item: ContentItem) {
        self.stats.total_tokens += item.token_count;
        if item.metadata_only {
            self.stats.metadata_only_count += 1;
        }
        self.stats.items_fetched += 1;
        self.items.push(item);
    }

    /// Get total token count
    pub fn total_tokens(&self) -> usize {
        self.stats.total_tokens
    }
}

/// Statistics about a fetch operation
#[derive(Debug, Clone, Default)]
pub struct FetchStats {
    /// Number of items fetched
    pub items_fetched: usize,
    /// Number of items skipped (filtered)
    pub items_skipped: usize,
    /// Total tokens in fetched content
    pub total_tokens: usize,
    /// Number of items with metadata only
    pub metadata_only_count: usize,
    /// Duration of fetch in milliseconds
    pub duration_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_item_new() {
        let source_id = Uuid::new_v4();
        let item = ContentItem::new(
            source_id,
            ContentCategory::File,
            "/path/to/file.rs",
            "file.rs",
        );

        assert_eq!(item.source_id, source_id);
        assert_eq!(item.uri, "/path/to/file.rs");
        assert_eq!(item.title, "file.rs");
        assert!(item.content.is_none());
        assert!(item.metadata_only);
    }

    #[test]
    fn test_content_item_with_content() {
        let source_id = Uuid::new_v4();
        let item = ContentItem::new(
            source_id,
            ContentCategory::File,
            "/path/to/file.rs",
            "file.rs",
        )
        .with_content("fn main() { println!(\"Hello\"); }");

        assert!(item.content.is_some());
        assert!(!item.metadata_only);
        assert!(item.token_count > 0);
    }

    #[test]
    fn test_content_item_hash() {
        let source_id = Uuid::new_v4();
        let item1 = ContentItem::new(
            source_id,
            ContentCategory::File,
            "/path/to/file.rs",
            "file.rs",
        )
        .with_content("hello world");
        let item2 = ContentItem::new(
            source_id,
            ContentCategory::File,
            "/path/to/other.rs",
            "other.rs",
        )
        .with_content("hello world");

        // Same content = same hash
        assert_eq!(item1.content_hash(), item2.content_hash());

        let item3 = ContentItem::new(
            source_id,
            ContentCategory::File,
            "/path/to/file.rs",
            "file.rs",
        )
        .with_content("different content");

        // Different content = different hash
        assert_ne!(item1.content_hash(), item3.content_hash());
    }

    #[test]
    fn test_content_chunk_new() {
        let item_id = Uuid::new_v4();
        let chunk = ContentChunk::new(item_id, 0, "This is chunk text", 0, 18);

        assert_eq!(chunk.content_item_id, item_id);
        assert_eq!(chunk.chunk_index, 0);
        assert_eq!(chunk.text, "This is chunk text");
        assert!(chunk.token_count > 0);
    }

    #[test]
    fn test_fetch_config_default() {
        let config = FetchConfig::default();
        assert_eq!(config.max_tokens, 100_000);
        assert!(
            config
                .exclude_patterns
                .contains(&"node_modules/**".to_string())
        );
    }

    #[test]
    fn test_fetch_result_add_item() {
        let source_id = Uuid::new_v4();
        let mut result = FetchResult::new(source_id, false);

        let item = ContentItem::new(source_id, ContentCategory::File, "/test.rs", "test.rs")
            .with_content("fn test() {}");

        result.add_item(item);

        assert_eq!(result.stats.items_fetched, 1);
        assert!(result.stats.total_tokens > 0);
    }

    #[test]
    fn test_content_metadata_builder() {
        let metadata = ContentMetadata::default()
            .with_author("test@example.com")
            .with_language("rust")
            .with_url("https://github.com/test");

        assert_eq!(metadata.author, Some("test@example.com".to_string()));
        assert_eq!(metadata.language, Some("rust".to_string()));
        assert_eq!(metadata.url, Some("https://github.com/test".to_string()));
    }
}
