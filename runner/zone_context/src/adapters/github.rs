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
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;
use zone_core::Source;

const SOURCE_TYPE: &str = "github";

const FORBIDDEN_MESSAGE: &str = "Access forbidden - check token permissions";

const NOT_FOUND_MESSAGE: &str = "Repository not found";

const UNBUILT_MESSAGE: &str = "Failed to build HTTP client";

const REQUEST_FAILED_MESSAGE: &str = "Request failed";

const REFUSED_MESSAGE: &str = "API error";

const MISREAD_REPOSITORY_MESSAGE: &str = "Failed to parse repo info";

const UNLISTED_MESSAGE: &str = "Failed to fetch tree";

const MISREAD_TREE_MESSAGE: &str = "Failed to parse tree";

const UNREAD_MESSAGE: &str = "Failed to fetch content";

const MISREAD_CONTENT_MESSAGE: &str = "Failed to parse content";

const MALFORMED_CONTENT_MESSAGE: &str = "Failed to decode base64";

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

#[derive(Debug, Clone)]
struct CachedTree {
    tree: Tree,
    cached_at: Instant,
    last_accessed: Instant,
    size_bytes: usize,
}

/// Cache key for GitHub trees (owner, repo, branch)
type TreeCacheKey = (String, String, String);

#[derive(Debug, Clone)]
struct TreeCache {
    entries: Arc<RwLock<HashMap<TreeCacheKey, CachedTree>>>,
    in_flight: Arc<RwLock<HashSet<TreeCacheKey>>>,
    ttl: Duration,
    max_entries: usize,
    max_total_bytes: usize,
}

impl TreeCache {
    const MAX_ENTRIES: usize = 50;

    const MAX_TOTAL_BYTES: usize = 10 * 1024 * 1024;

    const ENTRY_KIND_AND_OVERHEAD_BYTES: usize = 20;

    const ENTRY_SIZE_FIELD_BYTES: usize = 8;

    const WAIT_ATTEMPTS: u64 = 10;

    const WAIT_INTERVAL_MILLISECONDS: u64 = 100;

    fn new(ttl: Duration) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            in_flight: Arc::new(RwLock::new(HashSet::new())),
            ttl,
            max_entries: Self::MAX_ENTRIES,
            max_total_bytes: Self::MAX_TOTAL_BYTES,
        }
    }

    fn estimate_tree_size(tree: &Tree) -> usize {
        let entries: usize = tree
            .entries
            .iter()
            .map(|entry| {
                entry.path.len()
                    + entry.sha.as_str().len()
                    + entry.mode.len()
                    + Self::ENTRY_KIND_AND_OVERHEAD_BYTES
                    + Self::ENTRY_SIZE_FIELD_BYTES
            })
            .sum();

        entries + tree.sha.as_str().len()
    }

    async fn get(&self, key: &TreeCacheKey) -> Option<Tree> {
        let mut entries = self.entries.write().await;
        entries.get_mut(key).and_then(|entry| {
            if entry.cached_at.elapsed() < self.ttl {
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

        let mut total_size: usize = entries.values().map(|entry| entry.size_bytes).sum();

        while (entries.len() >= self.max_entries || total_size + tree_size > self.max_total_bytes)
            && !entries.is_empty()
        {
            if let Some((lru_key, _)) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(key, entry)| (key.clone(), entry.size_bytes))
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

        let ttl = self.ttl;
        entries.retain(|_, entry| entry.cached_at.elapsed() < ttl);
    }
}

enum ContentRequest<'service> {
    Sendable {
        service: &'service PullRequestService,
        repository: Repository,
        path: RepositoryPath,
        credential: Credential,
    },
    Skipped(ContextError),
}

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
    const TREE_CACHE_TTL: Duration = Duration::from_secs(300);

    /// Create a new GitHub adapter reading from api.github.com
    pub fn new() -> Self {
        Self::serving(
            PullRequestService::new()
                .map(PullRequestService::with_anonymous_reads)
                .map_err(Self::unbuilt),
        )
    }

    /// Create a GitHub adapter that reads through `service`
    pub fn with_service(service: PullRequestService) -> Self {
        Self::serving(Ok(service))
    }

    fn serving(service: std::result::Result<PullRequestService, String>) -> Self {
        Self {
            service,
            tree_cache: TreeCache::new(Self::TREE_CACHE_TTL),
        }
    }

    fn unbuilt(error: PullRequestError) -> String {
        let reason = match error {
            PullRequestError::Http(error) => error.to_string(),
            error => error.to_string(),
        };
        format!("{UNBUILT_MESSAGE}: {reason}")
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

    fn not_found(owner: &str, repo: &str) -> ContextError {
        ContextError::InvalidSourceConfig(format!("{NOT_FOUND_MESSAGE}: {owner}/{repo}"))
    }

    fn unlisted(status: Status) -> ContextError {
        Self::failed(format!("{UNLISTED_MESSAGE}: {status}"))
    }

    fn unread(status: Status) -> ContextError {
        Self::failed(format!("{UNREAD_MESSAGE}: {status}"))
    }

    fn repository_error(owner: &str, repo: &str, error: PullRequestError) -> ContextError {
        match error {
            PullRequestError::NotFound
            | PullRequestError::RepositoryNotFound
            | PullRequestError::InvalidRepositoryName => Self::not_found(owner, repo),
            PullRequestError::Forbidden => {
                ContextError::PermissionDenied(FORBIDDEN_MESSAGE.to_string())
            }
            PullRequestError::AuthenticationFailed => {
                Self::failed(format!("{REFUSED_MESSAGE}: {}", Status::UNAUTHORIZED))
            }
            PullRequestError::Refused { status, .. } => {
                Self::failed(format!("{REFUSED_MESSAGE}: {}", Status(status)))
            }
            PullRequestError::Http(error) => {
                Self::failed(format!("{REQUEST_FAILED_MESSAGE}: {error}"))
            }
            PullRequestError::GitHubApi(reason) => {
                Self::failed(format!("{MISREAD_REPOSITORY_MESSAGE}: {reason}"))
            }
            PullRequestError::Parse(error) => {
                Self::failed(format!("{MISREAD_REPOSITORY_MESSAGE}: {error}"))
            }
            error => Self::failed(format!("{REFUSED_MESSAGE}: {error}")),
        }
    }

    fn tree_error(error: PullRequestError) -> ContextError {
        if let Some(status) = Self::status(&error) {
            return Self::unlisted(status);
        }
        match error {
            PullRequestError::Parse(ParseError::BranchName(_)) => Self::unlisted(Status::NOT_FOUND),
            PullRequestError::Http(error) => {
                Self::failed(format!("{REQUEST_FAILED_MESSAGE}: {error}"))
            }
            PullRequestError::GitHubApi(reason) => {
                Self::failed(format!("{MISREAD_TREE_MESSAGE}: {reason}"))
            }
            PullRequestError::Parse(error @ ParseError::ObjectId(_)) => {
                Self::failed(format!("{MISREAD_TREE_MESSAGE}: {error}"))
            }
            error => Self::failed(format!("{UNLISTED_MESSAGE}: {error}")),
        }
    }

    fn content_error(error: PullRequestError) -> ContextError {
        if let Some(status) = Self::status(&error) {
            return Self::unread(status);
        }
        match error {
            PullRequestError::Parse(ParseError::BranchName(_)) => Self::unread(Status::NOT_FOUND),
            PullRequestError::Http(error) => {
                Self::failed(format!("{REQUEST_FAILED_MESSAGE}: {error}"))
            }
            PullRequestError::GitHubApi(reason) => {
                Self::failed(format!("{MISREAD_CONTENT_MESSAGE}: {reason}"))
            }
            error => Self::failed(format!("{UNREAD_MESSAGE}: {error}")),
        }
    }

    fn size(entry: &TreeEntry) -> Option<usize> {
        entry
            .size
            .map(|size| usize::try_from(size).unwrap_or(usize::MAX))
    }

    fn validate_github_identifier(identifier: &str, field_name: &str) -> Result<()> {
        if identifier.is_empty() {
            return Err(ContextError::InvalidSourceConfig(format!(
                "{} cannot be empty",
                field_name
            )));
        }

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

    fn parse_config(&self, source: &Source) -> Result<GitHubConfig> {
        serde_json::from_value(source.config.clone()).map_err(|error| {
            ContextError::InvalidSourceConfig(format!("Invalid GitHub config: {}", error))
        })
    }

    fn is_binary_file(path: &str) -> bool {
        if let Some(extension_start) = path.rfind('.') {
            let extension = &path[extension_start..];
            BINARY_EXTENSIONS.contains(&extension)
        } else {
            false
        }
    }

    fn get_content_type(path: &str) -> String {
        if let Some(extension_start) = path.rfind('.') {
            let extension = &path[extension_start + 1..].to_lowercase();
            match extension.as_str() {
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

    fn should_include_file(
        path: &str,
        include_patterns: &[String],
        exclude_patterns: &[String],
    ) -> bool {
        if Self::matches_patterns(path, exclude_patterns) {
            return false;
        }

        if !include_patterns.is_empty() {
            return Self::matches_patterns(path, include_patterns);
        }

        true
    }

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
            .map_err(|error| Self::repository_error(owner, repo, error))?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.get_default_branch(&repository, credential.secret())
            })
            .await?
            .map_err(|error| Self::repository_error(owner, repo, error))
    }

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
        let repository = Repository::new(owner, repo).map_err(Self::tree_error)?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.fetch_tree_at(&repository, credential.secret(), branch, true)
            })
            .await?
            .map_err(Self::tree_error)
    }

    async fn get_tree_cached(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        token: Option<&str>,
    ) -> Result<Tree> {
        let key = (owner.to_string(), repo.to_string(), branch.to_string());

        if let Some(tree) = self.tree_cache.get(&key).await {
            return Ok(tree);
        }

        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            if in_flight.contains(&key) {
                drop(in_flight);

                for _ in 0..TreeCache::WAIT_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(
                        TreeCache::WAIT_INTERVAL_MILLISECONDS,
                    ))
                    .await;
                    if let Some(tree) = self.tree_cache.get(&key).await {
                        return Ok(tree);
                    }
                }
            } else {
                in_flight.insert(key.clone());
            }
        }

        let fetch_result = self.get_tree(owner, repo, branch, token).await;

        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            in_flight.remove(&key);
        }

        let tree = fetch_result?;

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
        let answer = match self.content_request(config, branch, path) {
            ContentRequest::Skipped(error) => return Ok(Err(error)),
            ContentRequest::Sendable {
                service,
                repository,
                path,
                credential,
            } => {
                self.rate_limit_config()
                    .retried(Self::limited, || {
                        service.fetch_contents(&repository, credential.secret(), &path, branch)
                    })
                    .await?
            }
        };

        match answer {
            Ok(contents) => Ok(Ok(contents)),
            Err(error @ PullRequestError::MalformedContent) => Err(ContextError::Parse(format!(
                "{MALFORMED_CONTENT_MESSAGE}: {error}"
            ))),
            Err(error) => Ok(Err(Self::content_error(error))),
        }
    }

    fn content_request(
        &self,
        config: &GitHubConfig,
        branch: &str,
        path: &str,
    ) -> ContentRequest<'_> {
        self.sendable_content(config, branch, path)
            .unwrap_or_else(ContentRequest::Skipped)
    }

    fn sendable_content(
        &self,
        config: &GitHubConfig,
        branch: &str,
        path: &str,
    ) -> Result<ContentRequest<'_>> {
        Self::validate_github_identifier(&config.owner, "owner")?;
        Self::validate_github_identifier(&config.repo, "repo")?;
        Self::validate_github_identifier(branch, "branch")?;

        let credential = Credential::bearer(config.token.as_deref())?;
        let service = self.service()?;
        let repository =
            Repository::new(&config.owner, &config.repo).map_err(Self::content_error)?;
        let path = RepositoryPath::parse(path).map_err(|_| Self::unread(Status::NOT_FOUND))?;

        Ok(ContentRequest::Sendable {
            service,
            repository,
            path,
            credential,
        })
    }

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
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_string();

        let uri = Self::file_uri(owner, repo, path, branch);
        let content_type = Self::get_content_type(path);

        let mut item = ContentItem::new(source_id, ContentCategory::File, uri, file_name)
            .with_content_type(content_type);

        let extension = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_string());
        let mut content_metadata = ContentMetadata {
            size_bytes: Some(size),
            extension,
            commit_hash: Some(sha.to_string()),
            branch: Some(branch.to_string()),
            ..Default::default()
        };

        if let Some(extension_start) = path.rfind('.') {
            let extension = &path[extension_start + 1..].to_lowercase();
            let language = match extension.as_str() {
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
            if let Some(language) = language {
                content_metadata.language = Some(language.to_string());
            }
        }

        item = item.with_metadata(content_metadata);

        if !metadata_only && let Some(content) = content {
            item = item.with_content(content);
        }

        Ok(item)
    }

    fn file_uri(owner: &str, repo: &str, path: &str, branch: &str) -> String {
        format!("github://{owner}/{repo}/{path}@{branch}")
    }

    fn priority(priority_order: &[String], path: &str) -> Option<usize> {
        priority_order.iter().position(|pattern| {
            Pattern::new(pattern).is_ok_and(|glob_pattern| glob_pattern.matches(path))
        })
    }
}

#[async_trait]
impl SourceAdapter for GitHubAdapter {
    fn source_type(&self) -> &str {
        SOURCE_TYPE
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        RateLimitConfig {
            requests_per_second: 1.0,
            burst_size: 10,
            retry_after_429: true,
            max_retries: 3,
            backoff_base_ms: 1000,
        }
    }

    async fn verify(&self, source: &Source) -> Result<()> {
        let config = self.parse_config(source)?;

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
                for (index, entry) in to_fetch.iter().enumerate() {
                    if let Some(item) = self
                        .fetch_file_item(source.id, &config, &branch, entry)
                        .await?
                    {
                        progress.on_item(&item);
                        result.add_item(item);
                    }
                    progress.on_progress(index + 1, Some(to_fetch.len().max(1)));
                }
            }
            FetchStrategy::MetadataOnly => {
                progress.on_message(&format!(
                    "Fetching metadata for {} files from GitHub",
                    total_files
                ));
                for (index, entry) in files.iter().enumerate() {
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
                    progress.on_progress(index + 1, Some(total_files));
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

                for (index, score) in prioritized.iter().enumerate() {
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
                    progress.on_progress(index + 1, Some(total_files));
                }
            }
            FetchStrategy::Progressive { priority_order } => {
                progress.on_message("Fetching files from GitHub by priority");

                let paths_with_sizes: Vec<(String, Option<usize>)> = files
                    .iter()
                    .map(|entry| (entry.path.clone(), Self::size(entry)))
                    .collect();

                let mut prioritized = prioritize_paths(&paths_with_sizes);

                if !priority_order.is_empty() {
                    prioritized.sort_by(|left, right| {
                        match (
                            Self::priority(&priority_order, &left.path),
                            Self::priority(&priority_order, &right.path),
                        ) {
                            (Some(left_position), Some(right_position)) => {
                                left_position.cmp(&right_position)
                            }
                            (Some(_), None) => Ordering::Less,
                            (None, Some(_)) => Ordering::Greater,
                            (None, None) => right
                                .score
                                .partial_cmp(&left.score)
                                .unwrap_or(Ordering::Equal),
                        }
                    });
                }

                let files_by_path: HashMap<&str, &&TreeEntry> = files
                    .iter()
                    .map(|entry| (entry.path.as_str(), entry))
                    .collect();

                for (index, priority_item) in prioritized.iter().enumerate() {
                    if let Some(entry) = files_by_path.get(priority_item.path.as_str())
                        && let Some(item) = self
                            .fetch_file_item(source.id, &config, &branch, entry)
                            .await?
                    {
                        progress.on_item(&item);
                        result.add_item(item);
                    }
                    progress.on_progress(index + 1, Some(prioritized.len()));
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
        if let Err(ContextError::InvalidSourceConfig(message)) = result {
            assert!(message.contains("owner") || message.contains("missing field"));
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
        if let Err(ContextError::InvalidSourceConfig(message)) = result {
            assert!(message.contains("repo") || message.contains("missing field"));
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
        assert!(GitHubAdapter::is_binary_file("image.png"));
        assert!(GitHubAdapter::is_binary_file("file.zip"));
        assert!(!GitHubAdapter::is_binary_file("file.rs"));
        assert!(!GitHubAdapter::is_binary_file("README.md"));
    }

    #[tokio::test]
    async fn a_missing_repository_fails_every_strategy() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let source = create_test_source(test_repository());

        for strategy in [
            FetchStrategy::Full,
            FetchStrategy::MetadataOnly,
            FetchStrategy::Partial { max_tokens: 1000 },
            FetchStrategy::Progressive {
                priority_order: vec!["*.md".to_string()],
            },
        ] {
            let error = adapter
                .fetch(
                    &source,
                    &FetchConfig::default(),
                    strategy.clone(),
                    &NoOpProgress,
                )
                .await
                .unwrap_err();

            assert_eq!(
                error.to_string(),
                "Invalid source configuration: Repository not found: test-owner/test-repo",
                "{strategy:?}"
            );
        }
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
