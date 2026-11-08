//! GitHub adapter for repository-based content sources
//!
//! Provides intelligent repository traversal with pattern matching,
//! binary file detection, and progressive fetching strategies.

use crate::adapters::ProgressCallback;
use crate::adapters::RateLimitConfig;
use crate::adapters::SourceAdapter;
use crate::adapters::SyncState;
use crate::adapters::credential::Credential;
use crate::adapters::status::Status;
use crate::content::ContentCategory;
use crate::content::ContentItem;
use crate::content::ContentMetadata;
use crate::content::FetchConfig;
use crate::content::FetchResult;
use crate::content::FetchStrategy;
use crate::content::TokenBudget;
use crate::content::estimate_tokens_from_bytes;
use crate::content::prioritize_paths;
use crate::error::ContextError;
use crate::error::Result;
use abnegate_vcs::BranchName;
use abnegate_vcs::Contents;
use abnegate_vcs::ParseError;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestService;
use abnegate_vcs::Repository;
use abnegate_vcs::RepositoryPath;
use abnegate_vcs::Tree;
use abnegate_vcs::TreeEntry;
use abnegate_vcs::TreeEntryKind;
use async_trait::async_trait;
use chrono::Utc;
use glob::Pattern;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;
use zone_core::Source;

const SOURCE_TYPE: &str = "github";

/// Binary file extensions to skip (same as FilesystemAdapter)
const BINARY_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".webp", ".svg", ".pdf", ".zip", ".tar",
    ".gz", ".7z", ".rar", ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib", ".bin", ".wasm",
    ".mp3", ".mp4", ".avi", ".mov", ".wav", ".flac", ".ogg", ".ttf", ".otf", ".woff", ".woff2",
    ".eot", ".icns",
];

/// Maximum file size in bytes (10MB, same as FilesystemAdapter)
const MAX_FILE_SIZE_BYTES: usize = 10 * 1024 * 1024;

/// Configuration for GitHub sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    /// Repository owner
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// Branch name (defaults to repo's default branch)
    #[serde(default)]
    pub branch: Option<String>,
    /// Path within repository (defaults to root "")
    #[serde(default)]
    pub path: Option<String>,
    /// GitHub API token (optional)
    #[serde(default)]
    pub token: Option<String>,
}

/// Cached tree entry with LRU tracking
#[derive(Debug, Clone)]
struct CachedTree {
    tree: Tree,
    cached_at: Instant,
    last_accessed: Instant,
    size_bytes: usize,
}

/// Cache key for GitHub trees (owner, repo, branch)
type TreeCacheKey = (String, String, String);

/// Tree cache with TTL and size limits
#[derive(Debug, Clone)]
struct TreeCache {
    entries: Arc<RwLock<HashMap<TreeCacheKey, CachedTree>>>,
    in_flight: Arc<RwLock<HashSet<TreeCacheKey>>>,
    ttl: Duration,
    max_entries: usize,
    max_total_bytes: usize,
}

impl TreeCache {
    /// Maximum number of cached trees
    const MAX_ENTRIES: usize = 50;

    /// Maximum total cached size (10MB)
    const MAX_TOTAL_BYTES: usize = 10 * 1024 * 1024;

    fn new(ttl_secs: u64) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            in_flight: Arc::new(RwLock::new(HashSet::new())),
            ttl: Duration::from_secs(ttl_secs),
            max_entries: Self::MAX_ENTRIES,
            max_total_bytes: Self::MAX_TOTAL_BYTES,
        }
    }

    const ENTRY_KIND_BYTES: usize = 20;

    const ENTRY_SIZE_BYTES: usize = 8;

    fn estimate_tree_size(tree: &Tree) -> usize {
        let entries: usize = tree
            .entries
            .iter()
            .map(|entry| {
                entry.path.len()
                    + entry.sha.as_str().len()
                    + entry.mode.len()
                    + Self::ENTRY_KIND_BYTES
                    + Self::ENTRY_SIZE_BYTES
            })
            .sum();

        entries + tree.sha.as_str().len()
    }

    async fn get(&self, key: &TreeCacheKey) -> Option<Tree> {
        let mut entries = self.entries.write().await;
        entries.get_mut(key).and_then(|entry| {
            if entry.cached_at.elapsed() < self.ttl {
                // Update last accessed time for LRU
                entry.last_accessed = Instant::now();
                Some(entry.tree.clone())
            } else {
                None
            }
        })
    }

    async fn insert(&self, key: TreeCacheKey, tree: Tree) {
        let mut entries = self.entries.write().await;
        let tree_size = Self::estimate_tree_size(&tree);

        // Calculate current total size
        let mut total_size: usize = entries.values().map(|e| e.size_bytes).sum();

        // Evict entries if needed to make space
        while (entries.len() >= self.max_entries || total_size + tree_size > self.max_total_bytes)
            && !entries.is_empty()
        {
            // Find LRU entry (oldest last_accessed)
            if let Some((lru_key, _)) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(k, v)| (k.clone(), v.size_bytes))
            {
                if let Some(removed) = entries.remove(&lru_key) {
                    total_size = total_size.saturating_sub(removed.size_bytes);
                    tracing::debug!(
                        "Evicted LRU tree cache entry: {}/{}/{} ({} bytes)",
                        lru_key.0,
                        lru_key.1,
                        lru_key.2,
                        removed.size_bytes
                    );
                }
            } else {
                break;
            }
        }

        let now = Instant::now();
        entries.insert(
            key,
            CachedTree {
                tree,
                cached_at: now,
                last_accessed: now,
                size_bytes: tree_size,
            },
        );

        // Cleanup stale entries (optional, as we now have size-based eviction)
        let ttl = self.ttl;
        entries.retain(|_, entry| entry.cached_at.elapsed() < ttl);
    }
}

/// GitHub source adapter
#[derive(Debug, Clone)]
pub struct GitHubAdapter {
    service: std::result::Result<PullRequestService, String>,
    tree_cache: TreeCache,
}

impl Default for GitHubAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubAdapter {
    /// Tree cache TTL in seconds (5 minutes)
    const TREE_CACHE_TTL_SECS: u64 = 300;

    const NOT_FOUND_MESSAGE: &str = "Repository not found";

    const FORBIDDEN_MESSAGE: &str = "Access forbidden - check token permissions";

    /// Create a new GitHub adapter reading from api.github.com
    pub fn new() -> Self {
        let service = PullRequestService::new()
            .map(PullRequestService::with_anonymous_reads)
            .map_err(|error| match error {
                PullRequestError::Http(error) => format!("Failed to build HTTP client: {error}"),
                error => format!("Failed to build HTTP client: {error}"),
            });
        Self {
            service,
            tree_cache: TreeCache::new(Self::TREE_CACHE_TTL_SECS),
        }
    }

    /// Create a GitHub adapter that reads through `service`
    pub fn with_service(service: PullRequestService) -> Self {
        Self {
            service: Ok(service),
            tree_cache: TreeCache::new(Self::TREE_CACHE_TTL_SECS),
        }
    }

    fn service(&self) -> Result<&PullRequestService> {
        self.service
            .as_ref()
            .map_err(|message| ContextError::Config(message.clone()))
    }

    fn limited(error: &PullRequestError) -> bool {
        matches!(
            error,
            PullRequestError::RateLimited | PullRequestError::QuotaExhausted { .. }
        )
    }

    fn status(error: &PullRequestError) -> Option<Status> {
        match error {
            PullRequestError::NotFound | PullRequestError::RepositoryNotFound => {
                Some(Status::NOT_FOUND)
            }
            PullRequestError::AuthenticationFailed => Some(Status::UNAUTHORIZED),
            PullRequestError::Forbidden => Some(Status::FORBIDDEN),
            PullRequestError::Refused { status, .. } => Some(Status(*status)),
            _ => None,
        }
    }

    fn failed(message: String) -> ContextError {
        ContextError::adapter(SOURCE_TYPE, message)
    }

    fn repository_failed(owner: &str, repo: &str, error: PullRequestError) -> ContextError {
        match error {
            PullRequestError::NotFound
            | PullRequestError::RepositoryNotFound
            | PullRequestError::InvalidRepositoryName => ContextError::InvalidSourceConfig(
                format!("{}: {owner}/{repo}", Self::NOT_FOUND_MESSAGE),
            ),
            PullRequestError::Forbidden => {
                ContextError::PermissionDenied(Self::FORBIDDEN_MESSAGE.to_string())
            }
            PullRequestError::AuthenticationFailed => {
                Self::failed(format!("API error: {}", Status::UNAUTHORIZED))
            }
            PullRequestError::Refused { status, .. } => {
                Self::failed(format!("API error: {}", Status(status)))
            }
            PullRequestError::Http(error) => Self::failed(format!("Request failed: {error}")),
            PullRequestError::GitHubApi(reason) => {
                Self::failed(format!("Failed to parse repo info: {reason}"))
            }
            PullRequestError::Parse(error) => {
                Self::failed(format!("Failed to parse repo info: {error}"))
            }
            error => Self::failed(format!("API error: {error}")),
        }
    }

    fn tree_failed(error: PullRequestError) -> ContextError {
        if let Some(status) = Self::status(&error) {
            return Self::failed(format!("Failed to fetch tree: {status}"));
        }
        Self::failed(match error {
            PullRequestError::Parse(ParseError::BranchName(_)) => {
                format!("Failed to fetch tree: {}", Status::NOT_FOUND)
            }
            PullRequestError::Http(error) => format!("Request failed: {error}"),
            PullRequestError::GitHubApi(reason) => format!("Failed to parse tree: {reason}"),
            PullRequestError::Parse(error @ ParseError::ObjectId(_)) => {
                format!("Failed to parse tree: {error}")
            }
            error => format!("Failed to fetch tree: {error}"),
        })
    }

    fn content_failed(error: PullRequestError) -> ContextError {
        if let Some(status) = Self::status(&error) {
            return Self::failed(format!("Failed to fetch content: {status}"));
        }
        Self::failed(match error {
            PullRequestError::Parse(ParseError::BranchName(_)) => Self::content_not_found(),
            PullRequestError::Http(error) => format!("Request failed: {error}"),
            PullRequestError::GitHubApi(reason) => format!("Failed to parse content: {reason}"),
            error => format!("Failed to fetch content: {error}"),
        })
    }

    fn content_not_found() -> String {
        format!("Failed to fetch content: {}", Status::NOT_FOUND)
    }

    fn size(entry: &TreeEntry) -> Option<usize> {
        entry
            .size
            .map(|size| usize::try_from(size).unwrap_or(usize::MAX))
    }

    /// Validate GitHub identifier (owner, repo, branch) to prevent URL injection
    fn validate_github_identifier(identifier: &str, field_name: &str) -> Result<()> {
        if identifier.is_empty() {
            return Err(ContextError::InvalidSourceConfig(format!(
                "{} cannot be empty",
                field_name
            )));
        }

        // Check for path traversal and injection characters
        if identifier.contains('/')
            || identifier.contains('\\')
            || identifier.contains('\n')
            || identifier.contains('\r')
            || identifier.contains('\0')
        {
            return Err(ContextError::InvalidSourceConfig(format!(
                "{} contains invalid characters (/, \\, newlines, or null bytes)",
                field_name
            )));
        }

        Ok(())
    }

    /// Parse GitHub config from source
    fn parse_config(&self, source: &Source) -> Result<GitHubConfig> {
        serde_json::from_value(source.config.clone())
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid GitHub config: {}", e)))
    }

    /// Check if a file is binary based on extension
    fn is_binary_file(path: &str) -> bool {
        if let Some(ext_start) = path.rfind('.') {
            let ext = &path[ext_start..];
            BINARY_EXTENSIONS.contains(&ext)
        } else {
            false
        }
    }

    /// Map file extension to content type (same as FilesystemAdapter)
    fn get_content_type(path: &str) -> String {
        if let Some(ext_start) = path.rfind('.') {
            let ext = &path[ext_start + 1..].to_lowercase();
            match ext.as_str() {
                "rs" => "text/rust".to_string(),
                "py" => "text/python".to_string(),
                "ts" | "tsx" => "text/typescript".to_string(),
                "json" => "application/json".to_string(),
                "js" | "jsx" => "text/javascript".to_string(),
                "md" => "text/markdown".to_string(),
                "toml" => "application/toml".to_string(),
                "yaml" | "yml" => "application/yaml".to_string(),
                "html" => "text/html".to_string(),
                "css" => "text/css".to_string(),
                _ => "text/plain".to_string(),
            }
        } else {
            "text/plain".to_string()
        }
    }

    /// Check if a path matches any of the patterns
    fn matches_patterns(path: &str, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }

        patterns.iter().any(|pattern| {
            if let Ok(glob_pattern) = Pattern::new(pattern) {
                glob_pattern.matches(path)
            } else {
                false
            }
        })
    }

    /// Check if a file should be included based on patterns
    fn should_include_file(
        path: &str,
        include_patterns: &[String],
        exclude_patterns: &[String],
    ) -> bool {
        // Check exclude patterns first
        if Self::matches_patterns(path, exclude_patterns) {
            return false;
        }

        // If include patterns specified, file must match at least one
        if !include_patterns.is_empty() {
            return Self::matches_patterns(path, include_patterns);
        }

        // No include patterns, include by default (unless excluded above)
        true
    }

    /// Read the repository's default branch, checking it is accessible
    async fn get_default_branch(
        &self,
        owner: &str,
        repo: &str,
        token: Option<&str>,
    ) -> Result<BranchName> {
        Self::validate_github_identifier(owner, "owner")?;
        Self::validate_github_identifier(repo, "repo")?;

        let credential = Credential::bearer(token)?;
        let service = self.service()?;
        let repository = Repository::new(owner, repo)
            .map_err(|error| Self::repository_failed(owner, repo, error))?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.get_default_branch(&repository, credential.secret())
            })
            .await?
            .map_err(|error| Self::repository_failed(owner, repo, error))
    }

    /// Fetch Git tree (recursive)
    async fn get_tree(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        token: Option<&str>,
    ) -> Result<Tree> {
        Self::validate_github_identifier(owner, "owner")?;
        Self::validate_github_identifier(repo, "repo")?;
        Self::validate_github_identifier(branch, "branch")?;

        let credential = Credential::bearer(token)?;
        let service = self.service()?;
        let repository = Repository::new(owner, repo).map_err(Self::tree_failed)?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.fetch_tree_at(&repository, credential.secret(), branch, true)
            })
            .await?
            .map_err(Self::tree_failed)
    }

    /// Get tree with caching and in-flight tracking
    async fn get_tree_cached(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        token: Option<&str>,
    ) -> Result<Tree> {
        let key = (owner.to_string(), repo.to_string(), branch.to_string());

        // Check cache first
        if let Some(tree) = self.tree_cache.get(&key).await {
            return Ok(tree);
        }

        // Check if already being fetched by another task
        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            if in_flight.contains(&key) {
                // Drop the lock and wait a bit before retrying
                drop(in_flight);

                // Wait for the in-flight fetch to complete (with timeout)
                for _ in 0..10 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    if let Some(tree) = self.tree_cache.get(&key).await {
                        return Ok(tree);
                    }
                }

                // If still not cached after waiting, fall through to fetch
            } else {
                // Mark as in-flight
                in_flight.insert(key.clone());
            }
        }

        // Fetch from API (outside the lock)
        let fetch_result = self.get_tree(owner, repo, branch, token).await;

        // Remove from in-flight and handle result
        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            in_flight.remove(&key);
        }

        let tree = fetch_result?;

        // Store in cache
        self.tree_cache.insert(key, tree.clone()).await;

        Ok(tree)
    }

    /// Fetch file content: the outer error aborts the fetch, the inner one skips the file
    async fn get_file_content(
        &self,
        config: &GitHubConfig,
        branch: &str,
        path: &str,
    ) -> Result<Result<Contents>> {
        let (service, repository, path, credential) =
            match self.content_request(config, branch, path) {
                Ok(request) => request,
                Err(error) => return Ok(Err(error)),
            };

        let answer = self
            .rate_limit_config()
            .retried(Self::limited, || {
                service.fetch_contents(&repository, credential.secret(), &path, branch)
            })
            .await?;

        match answer {
            Ok(contents) => Ok(Ok(contents)),
            Err(error @ PullRequestError::MalformedContent) => Err(ContextError::Parse(format!(
                "Failed to decode base64: {error}"
            ))),
            Err(error) => Ok(Err(Self::content_failed(error))),
        }
    }

    fn content_request(
        &self,
        config: &GitHubConfig,
        branch: &str,
        path: &str,
    ) -> Result<(&PullRequestService, Repository, RepositoryPath, Credential)> {
        Self::validate_github_identifier(&config.owner, "owner")?;
        Self::validate_github_identifier(&config.repo, "repo")?;
        Self::validate_github_identifier(branch, "branch")?;

        let credential = Credential::bearer(config.token.as_deref())?;
        let service = self.service()?;
        let repository =
            Repository::new(&config.owner, &config.repo).map_err(Self::content_failed)?;
        let path =
            RepositoryPath::parse(path).map_err(|_| Self::failed(Self::content_not_found()))?;

        Ok((service, repository, path, credential))
    }

    /// Read file bytes as UTF-8 text, or `None` for a binary blob
    fn decode_text_content(content: Vec<u8>) -> Option<String> {
        String::from_utf8(content).ok()
    }

    async fn fetch_file_item(
        &self,
        source_id: uuid::Uuid,
        config: &GitHubConfig,
        branch: &str,
        entry: &TreeEntry,
    ) -> Result<Option<ContentItem>> {
        let contents = match self.get_file_content(config, branch, &entry.path).await? {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(path = %entry.path, error = %error, "skipping GitHub file");
                return Ok(None);
            }
        };

        let content = match contents.content.map(Self::decode_text_content) {
            Some(Some(text)) => Some(text),
            Some(None) => {
                tracing::debug!(path = %entry.path, "skipping non-UTF-8 GitHub file");
                return Ok(None);
            }
            None => None,
        };

        Ok(Some(Self::create_content_item(
            source_id,
            &config.owner,
            &config.repo,
            branch,
            &entry.path,
            entry.sha.as_str(),
            Self::size(entry).unwrap_or(0),
            content,
            false,
        )?))
    }

    /// Create a ContentItem from a GitHub file
    fn create_content_item(
        source_id: uuid::Uuid,
        owner: &str,
        repo: &str,
        branch: &str,
        path: &str,
        sha: &str,
        size: usize,
        content: Option<String>,
        metadata_only: bool,
    ) -> Result<ContentItem> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();

        let uri = Self::file_uri(owner, repo, path, branch);
        let content_type = Self::get_content_type(path);

        let mut item = ContentItem::new(source_id, ContentCategory::File, uri, file_name)
            .with_content_type(content_type);

        // Build metadata
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string());
        let mut content_metadata = ContentMetadata {
            size_bytes: Some(size),
            extension,
            commit_hash: Some(sha.to_string()),
            branch: Some(branch.to_string()),
            ..Default::default()
        };

        // Detect language from extension
        if let Some(ext_start) = path.rfind('.') {
            let ext = &path[ext_start + 1..].to_lowercase();
            let lang = match ext.as_str() {
                "rs" => Some("rust"),
                "py" => Some("python"),
                "ts" | "tsx" => Some("typescript"),
                "js" | "jsx" => Some("javascript"),
                "go" => Some("go"),
                "java" => Some("java"),
                "c" => Some("c"),
                "cpp" | "cc" | "cxx" => Some("cpp"),
                "h" | "hpp" => Some("c++"),
                _ => None,
            };
            if let Some(l) = lang {
                content_metadata.language = Some(l.to_string());
            }
        }

        item = item.with_metadata(content_metadata);

        // Set content if provided
        if !metadata_only && let Some(content) = content {
            item = item.with_content(content);
        }

        Ok(item)
    }

    fn file_uri(owner: &str, repo: &str, path: &str, branch: &str) -> String {
        format!("github://{owner}/{repo}/{path}@{branch}")
    }
}

#[async_trait]
impl SourceAdapter for GitHubAdapter {
    fn source_type(&self) -> &str {
        SOURCE_TYPE
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        // GitHub rate limits: 5000/hr authenticated, 60/hr unauthenticated
        // Convert to requests per second
        RateLimitConfig {
            requests_per_second: 1.0, // Conservative: ~3600/hr
            burst_size: 10,
            retry_after_429: true,
            max_retries: 3,
            backoff_base_ms: 1000,
        }
    }

    async fn verify(&self, source: &Source) -> Result<()> {
        let config = self.parse_config(source)?;

        // Validate required fields
        if config.owner.is_empty() {
            return Err(ContextError::InvalidSourceConfig(
                "owner is required".to_string(),
            ));
        }

        if config.repo.is_empty() {
            return Err(ContextError::InvalidSourceConfig(
                "repo is required".to_string(),
            ));
        }

        // Validate identifiers before making any API calls
        Self::validate_github_identifier(&config.owner, "owner")?;
        Self::validate_github_identifier(&config.repo, "repo")?;
        if let Some(ref branch) = config.branch {
            Self::validate_github_identifier(branch, "branch")?;
        }

        self.get_default_branch(&config.owner, &config.repo, config.token.as_deref())
            .await?;

        Ok(())
    }

    async fn estimate_tokens(&self, source: &Source) -> Result<usize> {
        let config = self.parse_config(source)?;

        let default_branch = self
            .get_default_branch(&config.owner, &config.repo, config.token.as_deref())
            .await?;
        let branch = config.branch.as_deref().unwrap_or(default_branch.as_str());

        // Fetch tree (with caching)
        let tree = self
            .get_tree_cached(&config.owner, &config.repo, branch, config.token.as_deref())
            .await?;

        let mut total_tokens = 0;

        for entry in &tree.entries {
            if entry.kind != TreeEntryKind::Blob {
                continue;
            }

            if Self::is_binary_file(&entry.path) {
                continue;
            }

            if let Some(size) = Self::size(entry)
                && size > MAX_FILE_SIZE_BYTES
            {
                continue;
            }

            if let Some(ref path_filter) = config.path
                && !path_filter.is_empty()
                && !entry.path.starts_with(path_filter)
            {
                continue;
            }

            if let Some(size) = Self::size(entry) {
                total_tokens += estimate_tokens_from_bytes(size);
            }
        }

        Ok(total_tokens)
    }

    async fn fetch(
        &self,
        source: &Source,
        fetch_config: &FetchConfig,
        strategy: FetchStrategy,
        progress: &dyn ProgressCallback,
    ) -> Result<FetchResult> {
        let config = self.parse_config(source)?;

        let default_branch = self
            .get_default_branch(&config.owner, &config.repo, config.token.as_deref())
            .await?;
        let branch = config
            .branch
            .as_deref()
            .unwrap_or(default_branch.as_str())
            .to_string();

        // Fetch tree (with caching)
        let tree = self
            .get_tree_cached(
                &config.owner,
                &config.repo,
                &branch,
                config.token.as_deref(),
            )
            .await?;

        let files: Vec<&TreeEntry> = tree
            .entries
            .iter()
            .filter(|entry| {
                if entry.kind != TreeEntryKind::Blob {
                    return false;
                }

                if Self::is_binary_file(&entry.path) {
                    return false;
                }

                if let Some(size) = Self::size(entry)
                    && size > MAX_FILE_SIZE_BYTES
                {
                    return false;
                }

                if let Some(ref path_filter) = config.path
                    && !path_filter.is_empty()
                    && !entry.path.starts_with(path_filter)
                {
                    return false;
                }

                Self::should_include_file(
                    &entry.path,
                    &fetch_config.include_patterns,
                    &fetch_config.exclude_patterns,
                )
            })
            .collect();

        let total_files = files.len();
        let mut result = FetchResult::new(source.id, fetch_config.last_version.is_some());
        result.version = Some(tree.sha.as_str().to_string());
        result.live_uris = files
            .iter()
            .map(|entry| Self::file_uri(&config.owner, &config.repo, &entry.path, &branch))
            .collect();

        match strategy {
            FetchStrategy::Full => {
                let to_fetch: Vec<&&TreeEntry> = files
                    .iter()
                    .filter(|entry| {
                        let uri = Self::file_uri(&config.owner, &config.repo, &entry.path, &branch);
                        !fetch_config.should_skip_blob(&uri, entry.sha.as_str())
                    })
                    .collect();
                result.stats.items_skipped = total_files.saturating_sub(to_fetch.len());
                progress.on_message(&format!(
                    "Fetching {} changed files from GitHub ({} unchanged)",
                    to_fetch.len(),
                    result.stats.items_skipped
                ));
                for (idx, entry) in to_fetch.iter().enumerate() {
                    if let Some(item) = self
                        .fetch_file_item(source.id, &config, &branch, entry)
                        .await?
                    {
                        progress.on_item(&item);
                        result.add_item(item);
                    }
                    progress.on_progress(idx + 1, Some(to_fetch.len().max(1)));
                }
            }
            FetchStrategy::MetadataOnly => {
                progress.on_message(&format!(
                    "Fetching metadata for {} files from GitHub",
                    total_files
                ));
                for (idx, entry) in files.iter().enumerate() {
                    let item = Self::create_content_item(
                        source.id,
                        &config.owner,
                        &config.repo,
                        &branch,
                        &entry.path,
                        entry.sha.as_str(),
                        Self::size(entry).unwrap_or(0),
                        None,
                        true,
                    )?;

                    progress.on_item(&item);
                    result.add_item(item);
                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::Partial { max_tokens } => {
                progress.on_message(&format!(
                    "Fetching files from GitHub (budget: {} tokens)",
                    max_tokens
                ));
                let mut budget = TokenBudget::new(max_tokens);
                let paths_with_sizes: Vec<(String, Option<usize>)> = files
                    .iter()
                    .map(|entry| (entry.path.clone(), Self::size(entry)))
                    .collect();
                let prioritized = prioritize_paths(&paths_with_sizes);
                let files_by_path: HashMap<&str, &&TreeEntry> = files
                    .iter()
                    .map(|entry| (entry.path.as_str(), entry))
                    .collect();

                for (idx, score) in prioritized.iter().enumerate() {
                    let Some(entry) = files_by_path.get(score.path.as_str()) else {
                        continue;
                    };
                    let estimated_tokens =
                        estimate_tokens_from_bytes(Self::size(entry).unwrap_or(0));

                    if !budget.can_fit(estimated_tokens) {
                        break;
                    }

                    if let Some(item) = self
                        .fetch_file_item(source.id, &config, &branch, entry)
                        .await?
                    {
                        budget.try_add(&entry.path, item.token_count);
                        progress.on_item(&item);
                        result.add_item(item);
                    }
                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::Progressive { priority_order } => {
                progress.on_message("Fetching files from GitHub by priority");

                // Build priority scores
                let paths_with_sizes: Vec<(String, Option<usize>)> = files
                    .iter()
                    .map(|entry| (entry.path.clone(), Self::size(entry)))
                    .collect();

                let mut prioritized = prioritize_paths(&paths_with_sizes);

                // If user provided priority_order, reorder to match those patterns first
                if !priority_order.is_empty() {
                    prioritized.sort_by(|a, b| {
                        let a_priority = priority_order.iter().position(|pattern| {
                            if let Ok(glob_pattern) = Pattern::new(pattern) {
                                glob_pattern.matches(&a.path)
                            } else {
                                false
                            }
                        });

                        let b_priority = priority_order.iter().position(|pattern| {
                            if let Ok(glob_pattern) = Pattern::new(pattern) {
                                glob_pattern.matches(&b.path)
                            } else {
                                false
                            }
                        });

                        match (a_priority, b_priority) {
                            (Some(ap), Some(bp)) => ap.cmp(&bp),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => b
                                .score
                                .partial_cmp(&a.score)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        }
                    });
                }

                // Fetch in priority order
                for (idx, priority_item) in prioritized.iter().enumerate() {
                    if let Some(entry) = files.iter().find(|e| e.path == priority_item.path)
                        && let Some(item) = self
                            .fetch_file_item(source.id, &config, &branch, entry)
                            .await?
                    {
                        progress.on_item(&item);
                        result.add_item(item);
                    }
                    progress.on_progress(idx + 1, Some(prioritized.len()));
                }
            }
        }

        Ok(result)
    }

    fn supports_incremental(&self) -> bool {
        true
    }

    async fn get_sync_state(&self, source: &Source) -> Result<SyncState> {
        let config = self.parse_config(source)?;

        let default_branch = self
            .get_default_branch(&config.owner, &config.repo, config.token.as_deref())
            .await?;
        let branch = config.branch.as_deref().unwrap_or(default_branch.as_str());

        let tree = self
            .get_tree(&config.owner, &config.repo, branch, config.token.as_deref())
            .await?;

        Ok(SyncState {
            source_id: source.id,
            last_sync_at: Some(Utc::now()),
            version: Some(tree.sha.as_str().to_string()),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::NoOpProgress;
    use abnegate_vcs::ObjectId;
    use chrono::Utc;
    use serde_json::Value;
    use serde_json::json;
    use uuid::Uuid;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    const GITHUB_HOST: &str = "github.com";

    const LIB_SHA: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    const README_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    const TREE_SHA: &str = "6367c48dd193d56ea7b0baad25b19455e529f5ee";

    const MALFORMED_TOKEN: &str = "bad\ntoken";

    const MALFORMED_TOKEN_MESSAGE: &str =
        "Invalid authorization token format (token not logged for security)";

    fn served(server: &MockServer) -> GitHubAdapter {
        GitHubAdapter::with_service(
            PullRequestService::standing_in_for(GITHUB_HOST, &server.uri())
                .unwrap()
                .with_anonymous_reads(),
        )
    }

    fn assert_repository_not_found<T: std::fmt::Debug>(result: Result<T>, repository: &str) {
        let expected = format!("Repository not found: {repository}");
        assert!(
            matches!(&result, Err(ContextError::InvalidSourceConfig(message)) if *message == expected),
            "{result:?}"
        );
    }

    fn test_repository() -> Value {
        json!({
            "owner": "test-owner",
            "repo": "test-repo"
        })
    }

    fn create_test_source(config: serde_json::Value) -> Source {
        Source {
            id: Uuid::new_v4(),
            name: "Test GitHub Source".to_string(),
            source_type: zone_core::SourceType::GitHub,
            category: zone_core::SourceCategory::File,
            config,
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_github_adapter_source_type() {
        let adapter = GitHubAdapter::new();
        assert_eq!(adapter.source_type(), "github");
    }

    #[tokio::test]
    async fn test_github_adapter_verify_missing_owner() {
        let adapter = GitHubAdapter::new();
        let source = create_test_source(json!({
            "repo": "test-repo"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            // Serde error will contain "missing field" when field is required
            assert!(msg.contains("owner") || msg.contains("missing field"));
        } else {
            panic!("Expected InvalidSourceConfig error for missing owner");
        }
    }

    #[tokio::test]
    async fn test_github_adapter_verify_missing_repo() {
        let adapter = GitHubAdapter::new();
        let source = create_test_source(json!({
            "owner": "test-owner"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            // Serde error will contain "missing field" when field is required
            assert!(msg.contains("repo") || msg.contains("missing field"));
        } else {
            panic!("Expected InvalidSourceConfig error for missing repo");
        }
    }

    #[tokio::test]
    async fn test_github_adapter_verify_valid_config() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "test-repo",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let result = adapter.verify(&source).await;
        assert!(result.is_ok(), "{result:?}");
    }

    async fn mount_listing(server: &MockServer, entries: Value) {
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "test-repo",
                "default_branch": "main"
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/git/trees/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sha": TREE_SHA,
                "tree": entries,
                "truncated": false
            })))
            .mount(server)
            .await;
    }

    fn listed_blob(file: &str, size: usize) -> Value {
        json!({
            "path": file,
            "mode": "100644",
            "type": "blob",
            "sha": LIB_SHA,
            "size": size
        })
    }

    #[tokio::test]
    async fn test_github_adapter_estimate_tokens_empty_repo() {
        let mock_server = MockServer::start().await;
        mount_listing(&mock_server, json!([])).await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let result = adapter.estimate_tokens(&source).await;
        assert!(matches!(result, Ok(0)), "{result:?}");
    }

    #[tokio::test]
    async fn test_github_adapter_estimate_tokens_with_files() {
        let mock_server = MockServer::start().await;
        mount_listing(
            &mock_server,
            json!([
                listed_blob("README.md", 400),
                listed_blob("src/lib.rs", 1200),
                listed_blob("logo.png", 4000),
                listed_blob("data.txt", MAX_FILE_SIZE_BYTES + 1),
            ]),
        )
        .await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let estimated = adapter.estimate_tokens(&source).await.unwrap();
        assert_eq!(
            estimated,
            estimate_tokens_from_bytes(400) + estimate_tokens_from_bytes(1200)
        );
    }

    #[test]
    fn test_github_adapter_estimate_tokens_excludes_binary() {
        // Test binary detection logic
        assert!(GitHubAdapter::is_binary_file("image.png"));
        assert!(GitHubAdapter::is_binary_file("file.zip"));
        assert!(!GitHubAdapter::is_binary_file("file.rs"));
        assert!(!GitHubAdapter::is_binary_file("README.md"));
    }

    #[tokio::test]
    async fn test_github_adapter_fetch_full() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert_repository_not_found(result, "test-owner/test-repo");
    }

    #[tokio::test]
    async fn test_github_adapter_fetch_metadata_only() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        assert_repository_not_found(result, "test-owner/test-repo");
    }

    #[tokio::test]
    async fn test_github_adapter_fetch_partial() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Partial { max_tokens: 1000 },
                &progress,
            )
            .await;

        assert_repository_not_found(result, "test-owner/test-repo");
    }

    #[tokio::test]
    async fn test_github_adapter_fetch_progressive() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Progressive {
                    priority_order: vec!["*.md".to_string()],
                },
                &progress,
            )
            .await;

        assert_repository_not_found(result, "test-owner/test-repo");
    }

    #[test]
    fn test_github_adapter_content_type_detection() {
        assert_eq!(GitHubAdapter::get_content_type("file.rs"), "text/rust");
        assert_eq!(GitHubAdapter::get_content_type("file.py"), "text/python");
        assert_eq!(GitHubAdapter::get_content_type("file.md"), "text/markdown");
        assert_eq!(
            GitHubAdapter::get_content_type("file.json"),
            "application/json"
        );
    }

    #[test]
    fn test_github_adapter_supports_incremental() {
        let adapter = GitHubAdapter::new();
        assert!(adapter.supports_incremental());
    }

    #[test]
    fn test_github_adapter_rate_limit_config() {
        let adapter = GitHubAdapter::new();
        let config = adapter.rate_limit_config();
        assert_eq!(config.requests_per_second, 1.0);
        assert_eq!(config.burst_size, 10);
        assert!(config.retry_after_429);
    }

    #[test]
    fn test_github_adapter_binary_exclusion() {
        assert!(GitHubAdapter::is_binary_file("image.png"));
        assert!(GitHubAdapter::is_binary_file("archive.zip"));
        assert!(GitHubAdapter::is_binary_file("binary.exe"));
        assert!(!GitHubAdapter::is_binary_file("code.rs"));
        assert!(!GitHubAdapter::is_binary_file("doc.md"));
    }

    #[test]
    fn test_github_adapter_pattern_matching() {
        assert!(GitHubAdapter::should_include_file(
            "src/main.rs",
            &["src/**".to_string()],
            &[]
        ));

        assert!(!GitHubAdapter::should_include_file(
            "src/main.rs",
            &[],
            &["src/**".to_string()]
        ));

        assert!(GitHubAdapter::should_include_file("README.md", &[], &[]));
    }

    #[tokio::test]
    async fn test_github_adapter_handles_api_error() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let source = create_test_source(json!({
            "owner": "nonexistent-owner-12345",
            "repo": "nonexistent-repo-67890"
        }));

        let result = adapter.verify(&source).await;
        assert_repository_not_found(result, "nonexistent-owner-12345/nonexistent-repo-67890");
    }

    #[test]
    fn test_decode_content() {
        assert_eq!(
            GitHubAdapter::decode_text_content(b"Hello, World!".to_vec()),
            Some("Hello, World!".to_string())
        );
    }

    #[test]
    fn test_decode_text_content_skips_invalid_utf8() {
        assert_eq!(
            GitHubAdapter::decode_text_content(vec![b'h', b'e', 0xff, b'l']),
            None
        );
    }

    #[test]
    fn test_create_content_item() {
        let source_id = Uuid::new_v4();
        let item = GitHubAdapter::create_content_item(
            source_id,
            "owner",
            "repo",
            "main",
            "src/lib.rs",
            LIB_SHA,
            1000,
            Some("fn main() {}".to_string()),
            false,
        )
        .unwrap();

        assert_eq!(item.source_id, source_id);
        assert_eq!(item.category, ContentCategory::File);
        assert_eq!(item.uri, "github://owner/repo/src/lib.rs@main");
        assert_eq!(item.title, "lib.rs");
        assert_eq!(item.content_type, "text/rust");
        assert!(item.content.is_some());
        assert!(!item.metadata_only);
        assert_eq!(item.metadata.commit_hash, Some(LIB_SHA.to_string()));
        assert_eq!(item.metadata.branch, Some("main".to_string()));
        assert_eq!(
            GitHubAdapter::file_uri("owner", "repo", "src/lib.rs", "main"),
            item.uri
        );
    }

    #[test]
    fn test_create_content_item_metadata_only() {
        let source_id = Uuid::new_v4();
        let item = GitHubAdapter::create_content_item(
            source_id,
            "owner",
            "repo",
            "main",
            "README.md",
            README_SHA,
            500,
            None,
            true,
        )
        .unwrap();

        assert!(item.content.is_none());
        assert!(item.metadata_only);
        assert_eq!(item.token_count, 0);
    }

    #[tokio::test]
    async fn a_malformed_token_fails_the_tree_and_skips_the_contents_unsent() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let config = GitHubConfig {
            owner: "test-owner".to_string(),
            repo: "test-repo".to_string(),
            branch: None,
            path: None,
            token: Some(MALFORMED_TOKEN.to_string()),
        };
        let entry = TreeEntry::new(
            "README.md",
            "100644",
            TreeEntryKind::Blob,
            ObjectId::parse(README_SHA).unwrap(),
        );

        let tree = adapter
            .get_tree("test-owner", "test-repo", "main", Some(MALFORMED_TOKEN))
            .await;
        let contents = adapter.get_file_content(&config, "main", "README.md").await;
        let item = adapter
            .fetch_file_item(Uuid::new_v4(), &config, "main", &entry)
            .await;

        assert!(
            matches!(&tree, Err(ContextError::Config(message)) if message == MALFORMED_TOKEN_MESSAGE),
            "{tree:?}"
        );
        assert!(
            matches!(&contents, Ok(Err(ContextError::Config(message))) if message == MALFORMED_TOKEN_MESSAGE),
            "{contents:?}"
        );
        assert!(matches!(item, Ok(None)), "{item:?}");
        assert!(mock_server.received_requests().await.unwrap().is_empty());
    }
}
