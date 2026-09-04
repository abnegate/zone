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
use std::collections::HashMap;
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
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
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

/// Previously indexed blob used to skip unchanged downloads
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexedBlob {
    /// Provider blob SHA, filesystem fingerprint, or equivalent
    pub blob_sha: Option<String>,
    /// Whether searchable embeddings already exist for this URI
    pub has_embeddings: bool,
}

/// Configuration for content fetching
///
/// Fetch/index writes the same content store as chat gathering, so the
/// default is an uncapped full fetch. Prompt assembly uses
/// `ContextConfig.token_budget` (50k), not this struct.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// Optional cap for an explicitly budgeted `Partial` fetch
    pub max_tokens: usize,
    /// Budget used only when `allow_metadata_only` is on
    pub token_budget: usize,
    /// Since timestamp for incremental fetching
    pub since: Option<DateTime<Utc>>,
    /// File patterns to include (glob syntax)
    pub include_patterns: Vec<String>,
    /// File patterns to exclude (glob syntax)
    pub exclude_patterns: Vec<String>,
    /// Allow `MetadataOnly` when a source is larger than 10× `token_budget`.
    /// Off by default so large repos are never stored as empty shells.
    pub allow_metadata_only: bool,
    /// Index the whole source instead of gathering prompt context
    pub index_mode: bool,
    /// Last persisted source version (tree SHA / fingerprint)
    pub last_version: Option<String>,
    /// Indexed files keyed by content URI so adapters can skip unchanged blobs
    pub known_blobs: HashMap<String, IndexedBlob>,
}

impl FetchConfig {
    /// Skip downloading a blob that is already indexed at this SHA
    pub fn should_skip_blob(&self, uri: &str, blob_sha: &str) -> bool {
        self.known_blobs.get(uri).is_some_and(|known| {
            known.has_embeddings && known.blob_sha.as_deref() == Some(blob_sha)
        })
    }

    /// Default exclude globs shared by context gathering and source indexing
    pub fn default_exclude_patterns() -> Vec<String> {
        vec![
            "node_modules/**".to_string(),
            "vendor/**".to_string(),
            ".git/**".to_string(),
            "target/**".to_string(),
            "dist/**".to_string(),
            "build/**".to_string(),
            "__pycache__/**".to_string(),
            "*.lock".to_string(),
            "*.min.js".to_string(),
            "*.min.css".to_string(),
            "*.pb.go".to_string(),
            "*.generated.*".to_string(),
        ]
    }

    /// Full fetch for chat/context assembly (prompt budget is applied later)
    pub fn for_context_gathering(force_refresh: bool) -> Self {
        Self {
            since: if force_refresh {
                None
            } else {
                Some(Utc::now() - chrono::Duration::days(30))
            },
            ..Self::default()
        }
    }

    /// Full-source fetch for search indexing
    pub fn for_source_indexing() -> Self {
        Self {
            index_mode: true,
            since: None,
            ..Self::default()
        }
    }

    /// Choose how much of a source to pull.
    ///
    /// Indexing and default fetch always take `Full`. A 100k-style
    /// metadata-only fallback is only used when a caller opts in.
    pub fn fetch_strategy(&self, estimated_tokens: usize) -> FetchStrategy {
        if self.index_mode || !self.allow_metadata_only {
            FetchStrategy::Full
        } else {
            decide_fetch_strategy(estimated_tokens, self.token_budget)
        }
    }
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            max_tokens: usize::MAX,
            token_budget: usize::MAX,
            since: None,
            include_patterns: vec![],
            exclude_patterns: Self::default_exclude_patterns(),
            allow_metadata_only: false,
            index_mode: false,
            last_version: None,
            known_blobs: HashMap::new(),
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
    /// Every live file URI in the source (used to retain the index)
    pub live_uris: Vec<String>,
    /// Provider version after this fetch (tree SHA / fingerprint)
    pub version: Option<String>,
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
            live_uris: Vec::new(),
            version: None,
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
        assert_eq!(config.max_tokens, usize::MAX);
        assert_eq!(config.token_budget, usize::MAX);
        assert!(!config.allow_metadata_only);
        assert!(!config.index_mode);
        assert!(
            config
                .exclude_patterns
                .contains(&"node_modules/**".to_string())
        );
        assert!(FetchConfig::for_source_indexing().index_mode);
        assert!(!FetchConfig::for_source_indexing().allow_metadata_only);
    }

    #[test]
    fn test_should_skip_blob_requires_matching_sha_and_embeddings() {
        let uri = "github://owner/repo/src/lib.rs@main";
        let mut config = FetchConfig::default();
        config.known_blobs.insert(
            uri.to_string(),
            IndexedBlob {
                blob_sha: Some("abc".to_string()),
                has_embeddings: true,
            },
        );

        assert!(config.should_skip_blob(uri, "abc"));
        assert!(!config.should_skip_blob(uri, "def"));
        config.known_blobs.get_mut(uri).unwrap().has_embeddings = false;
        assert!(!config.should_skip_blob(uri, "abc"));
    }

    #[test]
    fn test_fetch_config_never_metadata_only_for_persist_paths() {
        let huge = 5_000_000;
        assert!(matches!(
            FetchConfig::default().fetch_strategy(huge),
            FetchStrategy::Full
        ));
        assert!(matches!(
            FetchConfig::for_context_gathering(true).fetch_strategy(huge),
            FetchStrategy::Full
        ));
        assert!(matches!(
            FetchConfig::for_source_indexing().fetch_strategy(huge),
            FetchStrategy::Full
        ));
    }

    #[test]
    fn test_fetch_config_metadata_only_only_when_opted_in() {
        let config = FetchConfig {
            max_tokens: 100_000,
            token_budget: 100_000,
            allow_metadata_only: true,
            ..FetchConfig::default()
        };
        assert!(matches!(
            config.fetch_strategy(5_000_000),
            FetchStrategy::MetadataOnly
        ));
        assert!(matches!(config.fetch_strategy(50_000), FetchStrategy::Full));
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
