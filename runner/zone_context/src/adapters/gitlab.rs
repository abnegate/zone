//! GitLab adapter for repository-based content sources
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
use abnegate_vcs::Blob;
use abnegate_vcs::GitLabError;
use abnegate_vcs::GitLabService;
use abnegate_vcs::ParseError;
use abnegate_vcs::Project;
use abnegate_vcs::RemoteBranchName;
use abnegate_vcs::RepositoryPath;
use abnegate_vcs::TreeEntry;
use abnegate_vcs::TreeEntryKind;
use async_trait::async_trait;
use chrono::Utc;
use glob::Pattern;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;
use zone_core::Source;

/// Binary file extensions to skip (same as GitHub/Filesystem)
const BINARY_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".webp", ".svg", ".pdf", ".zip", ".tar",
    ".gz", ".7z", ".rar", ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib", ".bin", ".wasm",
    ".mp3", ".mp4", ".avi", ".mov", ".wav", ".flac", ".ogg", ".ttf", ".otf", ".woff", ".woff2",
    ".eot",
];

const SOURCE_TYPE: &str = "gitlab";

const FORBIDDEN_MESSAGE: &str = "Access forbidden - check token permissions";

const NOT_FOUND_MESSAGE: &str = "Project not found";

const UNBUILT_MESSAGE: &str = "Failed to build HTTP client";

const REQUEST_FAILED_MESSAGE: &str = "Request failed";

const REFUSED_MESSAGE: &str = "API error";

const MISREAD_PROJECT_MESSAGE: &str = "Failed to parse project info";

const UNSET_DEFAULT_BRANCH_MESSAGE: &str = "default_branch is null";

const UNLISTED_MESSAGE: &str = "Failed to fetch tree";

const MISREAD_TREE_MESSAGE: &str = "Failed to parse tree";

const UNREAD_MESSAGE: &str = "Failed to fetch content";

const MISREAD_CONTENT_MESSAGE: &str = "Failed to parse content";

/// Configuration for GitLab sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLabConfig {
    /// Project path (e.g., "owner/project") or numeric project ID (the
    /// console's wizard stores it as `project_id`)
    #[serde(alias = "project_id")]
    pub project: String,
    /// Branch name (defaults to repo's default branch)
    #[serde(default)]
    pub branch: Option<String>,
    /// Path within repository (defaults to root "")
    #[serde(default)]
    pub path: Option<String>,
    /// GitLab API token (optional)
    #[serde(default)]
    pub token: Option<String>,
    /// Base URL for self-hosted GitLab (defaults to gitlab.com; the console's
    /// wizard stores it as `host`)
    #[serde(default, alias = "host")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedTree {
    tree: Vec<TreeEntry>,
    cached_at: Instant,
    last_accessed: Instant,
    size_bytes: usize,
}

/// Cache key for GitLab trees (project, branch)
type TreeCacheKey = (String, String);

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

    fn estimate_tree_size(tree: &[TreeEntry]) -> usize {
        tree.iter()
            .map(|entry| {
                entry.path.len()
                    + entry.sha.as_str().len()
                    + entry.mode.len()
                    + Self::ENTRY_KIND_AND_OVERHEAD_BYTES
            })
            .sum()
    }

    async fn get(&self, key: &TreeCacheKey) -> Option<Vec<TreeEntry>> {
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

    async fn insert(&self, key: TreeCacheKey, tree: Vec<TreeEntry>) {
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
                        "Evicted LRU tree cache entry: {}/{} ({} bytes)",
                        lru_key.0,
                        lru_key.1,
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

#[derive(Debug, Clone)]
pub struct GitLabAdapter {
    service: std::result::Result<GitLabService, String>,
    tree_cache: TreeCache,
}

impl Default for GitLabAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitLabAdapter {
    const TREE_CACHE_TTL: Duration = Duration::from_secs(300);

    /// Estimated size of a file, since GitLab's tree lists none
    const ESTIMATED_FILE_BYTES: usize = 2048;

    /// Create a new GitLab adapter reading from gitlab.com
    pub fn new() -> Self {
        Self::serving(
            GitLabService::new()
                .map(GitLabService::with_anonymous_reads)
                .map_err(Self::unbuilt),
        )
    }

    /// Create a GitLab adapter that reads through `service`
    pub fn with_service(service: GitLabService) -> Self {
        Self::serving(Ok(service))
    }

    fn serving(service: std::result::Result<GitLabService, String>) -> Self {
        Self {
            service,
            tree_cache: TreeCache::new(Self::TREE_CACHE_TTL),
        }
    }

    fn unbuilt(error: GitLabError) -> String {
        let reason = match error {
            GitLabError::Http(error) => error.to_string(),
            error => error.to_string(),
        };
        format!("{UNBUILT_MESSAGE}: {reason}")
    }

    fn service(&self) -> Result<&GitLabService> {
        self.service
            .as_ref()
            .map_err(|message| ContextError::Config(message.clone()))
    }

    fn limited(error: &GitLabError) -> bool {
        matches!(error, GitLabError::RateLimited { .. })
    }

    fn status(error: &GitLabError) -> Option<Status> {
        match error {
            GitLabError::NotFound => Some(Status::NOT_FOUND),
            GitLabError::AuthenticationFailed => Some(Status::UNAUTHORIZED),
            GitLabError::Forbidden => Some(Status::FORBIDDEN),
            GitLabError::Refused { status, .. } => Some(Status(*status)),
            _ => None,
        }
    }

    fn failed(message: String) -> ContextError {
        ContextError::adapter(SOURCE_TYPE, message)
    }

    fn not_found(project: &str) -> ContextError {
        ContextError::InvalidSourceConfig(format!("{NOT_FOUND_MESSAGE}: {project}"))
    }

    fn unlisted(status: Status) -> ContextError {
        Self::failed(format!("{UNLISTED_MESSAGE}: {status}"))
    }

    fn unread(status: Status) -> ContextError {
        Self::failed(format!("{UNREAD_MESSAGE}: {status}"))
    }

    fn project_error(project: &str, error: GitLabError) -> ContextError {
        match error {
            GitLabError::NotFound => Self::not_found(project),
            GitLabError::AuthenticationFailed | GitLabError::Forbidden => {
                ContextError::PermissionDenied(FORBIDDEN_MESSAGE.to_string())
            }
            GitLabError::Refused { status, .. } => {
                Self::failed(format!("{REFUSED_MESSAGE}: {}", Status(status)))
            }
            GitLabError::Http(error) => Self::failed(format!("{REQUEST_FAILED_MESSAGE}: {error}")),
            GitLabError::GitLabApi(reason) => {
                Self::failed(format!("{MISREAD_PROJECT_MESSAGE}: {reason}"))
            }
            GitLabError::Parse(error @ ParseError::BranchName(_)) => {
                Self::failed(format!("{MISREAD_PROJECT_MESSAGE}: {error}"))
            }
            error @ GitLabError::InvalidOrigin => ContextError::Config(error.to_string()),
            error => Self::failed(format!("{REFUSED_MESSAGE}: {error}")),
        }
    }

    fn tree_error(error: GitLabError) -> ContextError {
        if let Some(status) = Self::status(&error) {
            return Self::unlisted(status);
        }
        match error {
            GitLabError::Parse(ParseError::BranchName(_)) => Self::unlisted(Status::NOT_FOUND),
            GitLabError::Http(error) => Self::failed(format!("{REQUEST_FAILED_MESSAGE}: {error}")),
            GitLabError::GitLabApi(reason) => {
                Self::failed(format!("{MISREAD_TREE_MESSAGE}: {reason}"))
            }
            GitLabError::Parse(error @ ParseError::ObjectId(_)) => {
                Self::failed(format!("{MISREAD_TREE_MESSAGE}: {error}"))
            }
            error => Self::failed(format!("{UNLISTED_MESSAGE}: {error}")),
        }
    }

    fn content_error(error: GitLabError) -> ContextError {
        if let Some(status) = Self::status(&error) {
            return Self::unread(status);
        }
        match error {
            GitLabError::Http(error) => Self::failed(format!("{REQUEST_FAILED_MESSAGE}: {error}")),
            GitLabError::GitLabApi(reason) => {
                Self::failed(format!("{MISREAD_CONTENT_MESSAGE}: {reason}"))
            }
            error => Self::failed(format!("{UNREAD_MESSAGE}: {error}")),
        }
    }

    fn project(project: &str) -> Result<Project> {
        Project::parse(project).map_err(|_| Self::not_found(project))
    }

    fn validate_gitlab_identifier(identifier: &str, field_name: &str) -> Result<()> {
        if identifier.is_empty() {
            return Err(ContextError::InvalidSourceConfig(format!(
                "{} cannot be empty",
                field_name
            )));
        }

        if identifier.contains('\n')
            || identifier.contains('\r')
            || identifier.contains('\0')
            || identifier.contains("..")
        {
            return Err(ContextError::InvalidSourceConfig(format!(
                "{} contains invalid characters (newlines, null bytes, or path traversal)",
                field_name
            )));
        }

        Ok(())
    }

    fn validate_path(path: &str) -> Result<()> {
        if path.contains("..") {
            return Err(ContextError::InvalidSourceConfig(
                "Path cannot contain '..'".to_string(),
            ));
        }
        if path.starts_with('/') {
            return Err(ContextError::InvalidSourceConfig(
                "Path cannot be absolute".to_string(),
            ));
        }
        Ok(())
    }

    fn parse_config(&self, source: &Source) -> Result<GitLabConfig> {
        serde_json::from_value(source.config.clone()).map_err(|error| {
            ContextError::InvalidSourceConfig(format!("Invalid GitLab config: {}", error))
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

    /// Map file extension to content type (same as GitHub/Filesystem)
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
        project: &str,
        token: Option<&str>,
    ) -> Result<RemoteBranchName> {
        Self::validate_gitlab_identifier(project, "project")?;

        let credential = Credential::private(token)?;
        let service = self.service()?;
        let parsed = Self::project(project)?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.get_default_branch(&parsed, credential.secret())
            })
            .await?
            .map_err(|error| Self::project_error(project, error))?
            .ok_or_else(|| {
                Self::failed(format!(
                    "{MISREAD_PROJECT_MESSAGE}: {UNSET_DEFAULT_BRANCH_MESSAGE}"
                ))
            })
    }

    async fn get_tree(
        &self,
        project: &str,
        branch: &str,
        path: Option<&str>,
        token: Option<&str>,
    ) -> Result<Vec<TreeEntry>> {
        Self::validate_gitlab_identifier(project, "project")?;
        Self::validate_gitlab_identifier(branch, "branch")?;
        let path = path.filter(|path| !path.is_empty());
        if let Some(path) = path {
            Self::validate_path(path)?;
        }

        let credential = Credential::private(token)?;
        let service = self.service()?;
        let project = Self::project(project)?;
        let prefix = path
            .map(RepositoryPath::parse)
            .transpose()
            .map_err(|_| Self::unlisted(Status::NOT_FOUND))?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.fetch_tree_at(&project, credential.secret(), branch, prefix.as_ref())
            })
            .await?
            .map_err(Self::tree_error)
    }

    async fn get_tree_cached(
        &self,
        project: &str,
        branch: &str,
        path: Option<&str>,
        token: Option<&str>,
    ) -> Result<Vec<TreeEntry>> {
        let key = (project.to_string(), branch.to_string());

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

                {
                    let in_flight = self.tree_cache.in_flight.read().await;
                    if in_flight.contains(&key) {
                        return Err(ContextError::Timeout {
                            operation: "tree cache wait".to_string(),
                            timeout_ms: TreeCache::WAIT_ATTEMPTS
                                * TreeCache::WAIT_INTERVAL_MILLISECONDS,
                        });
                    }
                }
            } else {
                in_flight.insert(key.clone());
            }
        }

        let fetch_result = self.get_tree(project, branch, path, token).await;

        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            in_flight.remove(&key);
        }

        let tree = fetch_result?;

        self.tree_cache.insert(key, tree.clone()).await;

        Ok(tree)
    }

    async fn get_blob(
        &self,
        project: &str,
        entry: &TreeEntry,
        token: Option<&str>,
    ) -> Result<Blob> {
        Self::validate_gitlab_identifier(project, "project")?;

        let credential = Credential::private(token)?;
        let service = self.service()?;
        let project = Self::project(project)?;

        self.rate_limit_config()
            .retried(Self::limited, || {
                service.fetch_blob(&project, credential.secret(), &entry.sha, usize::MAX)
            })
            .await?
            .map_err(Self::content_error)
    }

    fn create_content_item(
        source_id: uuid::Uuid,
        project: &str,
        branch: &str,
        path: &str,
        blob_id: &str,
        size: usize,
        content: Option<String>,
        metadata_only: bool,
    ) -> Result<ContentItem> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_string();

        let uri = Self::file_uri(project, path, branch);
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
            commit_hash: Some(blob_id.to_string()),
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

    fn file_uri(project: &str, path: &str, branch: &str) -> String {
        format!("gitlab://{project}/{path}@{branch}")
    }

    fn priority(priority_order: &[String], path: &str) -> Option<usize> {
        priority_order.iter().position(|pattern| {
            Pattern::new(pattern).is_ok_and(|glob_pattern| glob_pattern.matches(path))
        })
    }

    fn tree_version(entries: &[TreeEntry]) -> String {
        let mut hasher = Sha256::new();
        for entry in entries {
            if entry.kind == TreeEntryKind::Blob {
                hasher.update(entry.path.as_bytes());
                hasher.update(entry.sha.as_str().as_bytes());
            }
        }
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[async_trait]
impl SourceAdapter for GitLabAdapter {
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

        if config.project.is_empty() {
            return Err(ContextError::InvalidSourceConfig(
                "project is required".to_string(),
            ));
        }

        Self::validate_gitlab_identifier(&config.project, "project")?;
        if let Some(ref branch) = config.branch {
            Self::validate_gitlab_identifier(branch, "branch")?;
        }

        self.get_default_branch(&config.project, config.token.as_deref())
            .await?;

        Ok(())
    }

    async fn estimate_tokens(&self, source: &Source) -> Result<usize> {
        let config = self.parse_config(source)?;

        let default_branch = self
            .get_default_branch(&config.project, config.token.as_deref())
            .await?;

        let branch = config.branch.as_deref().unwrap_or(default_branch.as_str());

        let tree = self
            .get_tree_cached(
                &config.project,
                branch,
                config.path.as_deref(),
                config.token.as_deref(),
            )
            .await?;

        let mut total_tokens = 0;

        for entry in &tree {
            if entry.kind != TreeEntryKind::Blob {
                continue;
            }

            if Self::is_binary_file(&entry.path) {
                continue;
            }

            if let Some(ref path_filter) = config.path
                && !path_filter.is_empty()
                && !entry.path.starts_with(path_filter)
            {
                continue;
            }

            total_tokens += estimate_tokens_from_bytes(Self::ESTIMATED_FILE_BYTES);
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
            .get_default_branch(&config.project, config.token.as_deref())
            .await?;

        let branch = config
            .branch
            .as_deref()
            .unwrap_or(default_branch.as_str())
            .to_string();

        let tree = self
            .get_tree_cached(
                &config.project,
                &branch,
                config.path.as_deref(),
                config.token.as_deref(),
            )
            .await?;

        let files: Vec<&TreeEntry> = tree
            .iter()
            .filter(|entry| {
                if entry.kind != TreeEntryKind::Blob {
                    return false;
                }

                if Self::is_binary_file(&entry.path) {
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
        result.version = Some(Self::tree_version(&tree));
        result.live_uris = files
            .iter()
            .map(|entry| Self::file_uri(&config.project, &entry.path, &branch))
            .collect();

        match strategy {
            FetchStrategy::Full => {
                let to_fetch: Vec<&&TreeEntry> = files
                    .iter()
                    .filter(|entry| {
                        let uri = Self::file_uri(&config.project, &entry.path, &branch);
                        !fetch_config.should_skip_blob(&uri, entry.sha.as_str())
                    })
                    .collect();
                result.stats.items_skipped = total_files.saturating_sub(to_fetch.len());
                progress.on_message(&format!(
                    "Fetching {} changed files from GitLab ({} unchanged)",
                    to_fetch.len(),
                    result.stats.items_skipped
                ));
                for (index, entry) in to_fetch.iter().enumerate() {
                    let blob = self
                        .get_blob(&config.project, entry, config.token.as_deref())
                        .await?;
                    let size = blob.content.len();
                    let Ok(text) = String::from_utf8(blob.content) else {
                        tracing::debug!(path = %entry.path, "skipping non-UTF-8 GitLab file");
                        progress.on_progress(index + 1, Some(total_files));
                        continue;
                    };

                    let item = Self::create_content_item(
                        source.id,
                        &config.project,
                        &branch,
                        &entry.path,
                        entry.sha.as_str(),
                        size,
                        Some(text),
                        false,
                    )?;

                    progress.on_item(&item);
                    result.add_item(item);
                    progress.on_progress(index + 1, Some(to_fetch.len().max(1)));
                }
            }
            FetchStrategy::MetadataOnly => {
                progress.on_message(&format!(
                    "Fetching metadata for {} files from GitLab",
                    total_files
                ));
                for (index, entry) in files.iter().enumerate() {
                    let item = Self::create_content_item(
                        source.id,
                        &config.project,
                        &branch,
                        &entry.path,
                        entry.sha.as_str(),
                        Self::ESTIMATED_FILE_BYTES,
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
                    "Fetching files from GitLab (budget: {} tokens)",
                    max_tokens
                ));
                let mut budget = TokenBudget::new(max_tokens);

                for (index, entry) in files.iter().enumerate() {
                    let estimated_tokens = estimate_tokens_from_bytes(Self::ESTIMATED_FILE_BYTES);

                    if budget.can_fit(estimated_tokens) {
                        let blob = self
                            .get_blob(&config.project, entry, config.token.as_deref())
                            .await?;
                        let size = blob.content.len();
                        let Ok(text) = String::from_utf8(blob.content) else {
                            tracing::debug!(path = %entry.path, "skipping non-UTF-8 GitLab file");
                            continue;
                        };

                        let item = Self::create_content_item(
                            source.id,
                            &config.project,
                            &branch,
                            &entry.path,
                            entry.sha.as_str(),
                            size,
                            Some(text),
                            false,
                        )?;

                        budget.try_add(&entry.path, item.token_count);
                        progress.on_item(&item);
                        result.add_item(item);
                    } else {
                        break;
                    }

                    progress.on_progress(index + 1, Some(total_files));
                }
            }
            FetchStrategy::Progressive { priority_order } => {
                progress.on_message("Fetching files from GitLab by priority");

                let paths_with_sizes: Vec<(String, Option<usize>)> = files
                    .iter()
                    .map(|entry| (entry.path.clone(), Some(Self::ESTIMATED_FILE_BYTES)))
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
                    if let Some(entry) = files_by_path.get(priority_item.path.as_str()) {
                        let blob = self
                            .get_blob(&config.project, entry, config.token.as_deref())
                            .await?;
                        let size = blob.content.len();
                        let Ok(text) = String::from_utf8(blob.content) else {
                            tracing::debug!(path = %entry.path, "skipping non-UTF-8 GitLab file");
                            continue;
                        };

                        let item = Self::create_content_item(
                            source.id,
                            &config.project,
                            &branch,
                            &entry.path,
                            entry.sha.as_str(),
                            size,
                            Some(text),
                            false,
                        )?;

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
            .get_default_branch(&config.project, config.token.as_deref())
            .await?;

        let branch = config.branch.as_deref().unwrap_or(default_branch.as_str());

        let tree = self
            .get_tree_cached(
                &config.project,
                branch,
                config.path.as_deref(),
                config.token.as_deref(),
            )
            .await?;

        Ok(SyncState {
            source_id: source.id,
            last_sync_at: Some(Utc::now()),
            version: Some(Self::tree_version(&tree)),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::NoOpProgress;
    use abnegate_vcs::ObjectId;
    use serde_json::json;
    use uuid::Uuid;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path_regex;
    use wiremock::matchers::query_param;
    use wiremock::matchers::query_param_is_missing;

    const README_SHA: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const FILE_SHA: &str = "f9e8d7c6b5a40312837f6e5d4c3b2a1098765432";

    fn create_test_source(config: serde_json::Value) -> Source {
        Source {
            id: Uuid::new_v4(),
            name: "Test GitLab Source".to_string(),
            source_type: zone_core::SourceType::GitLab,
            category: zone_core::SourceCategory::File,
            config,
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn served(server: &MockServer) -> GitLabAdapter {
        GitLabAdapter::with_service(
            GitLabService::configured_allowing_http(&server.uri())
                .unwrap()
                .with_anonymous_reads(),
        )
    }

    #[test]
    fn test_gitlab_adapter_source_type() {
        let adapter = GitLabAdapter::new();
        assert_eq!(adapter.source_type(), "gitlab");
    }

    #[test]
    fn test_validate_gitlab_identifier() {
        assert!(GitLabAdapter::validate_gitlab_identifier("myproject", "project").is_ok());
        assert!(GitLabAdapter::validate_gitlab_identifier("owner/project", "project").is_ok());
        assert!(GitLabAdapter::validate_gitlab_identifier("123", "project").is_ok());

        assert!(GitLabAdapter::validate_gitlab_identifier("", "project").is_err());
        assert!(GitLabAdapter::validate_gitlab_identifier("project\n", "project").is_err());
        assert!(GitLabAdapter::validate_gitlab_identifier("project\0", "project").is_err());
        assert!(GitLabAdapter::validate_gitlab_identifier("../etc/passwd", "project").is_err());
    }

    #[tokio::test]
    async fn test_gitlab_adapter_verify_missing_project() {
        let adapter = GitLabAdapter::new();
        let source = create_test_source(json!({}));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_gitlab_adapter_verify_with_mock() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/projects/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = served(&mock_server);
        let source = create_test_source(json!({
            "project": "test-owner/test-project"
        }));

        let result = adapter.verify(&source).await;
        assert!(
            result.is_ok(),
            "a reachable project must verify: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_gitlab_adapter_with_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/projects/.*"))
            .and(header("PRIVATE-TOKEN", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = served(&mock_server);
        let source = create_test_source(json!({
            "project": "test-owner/test-project",
            "token": "test-token"
        }));

        let result = adapter.verify(&source).await;
        assert!(
            result.is_ok(),
            "the token must be sent as PRIVATE-TOKEN: {result:?}"
        );
    }

    #[test]
    fn test_gitlab_adapter_binary_detection() {
        assert!(GitLabAdapter::is_binary_file("image.png"));
        assert!(GitLabAdapter::is_binary_file("file.zip"));
        assert!(!GitLabAdapter::is_binary_file("file.rs"));
        assert!(!GitLabAdapter::is_binary_file("README.md"));
    }

    #[test]
    fn test_gitlab_adapter_content_type() {
        assert_eq!(GitLabAdapter::get_content_type("file.rs"), "text/rust");
        assert_eq!(GitLabAdapter::get_content_type("file.py"), "text/python");
        assert_eq!(GitLabAdapter::get_content_type("file.md"), "text/markdown");
    }

    #[test]
    fn test_gitlab_adapter_pattern_matching() {
        assert!(GitLabAdapter::should_include_file(
            "src/main.rs",
            &["src/**".to_string()],
            &[]
        ));

        assert!(!GitLabAdapter::should_include_file(
            "src/main.rs",
            &[],
            &["src/**".to_string()]
        ));
    }

    #[test]
    fn test_create_content_item() {
        let source_id = Uuid::new_v4();
        let item = GitLabAdapter::create_content_item(
            source_id,
            "owner/project",
            "main",
            "src/lib.rs",
            README_SHA,
            1000,
            Some("fn main() {}".to_string()),
            false,
        )
        .unwrap();

        assert_eq!(item.source_id, source_id);
        assert_eq!(item.category, ContentCategory::File);
        assert_eq!(item.uri, "gitlab://owner/project/src/lib.rs@main");
        assert_eq!(item.title, "lib.rs");
        assert!(item.content.is_some());
    }

    #[tokio::test]
    async fn test_gitlab_adapter_fetch_metadata_only() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/repository/tree.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "path": "README.md",
                    "type": "blob",
                    "mode": "100644",
                    "id": README_SHA
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex("/projects/[^/]+"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = served(&mock_server);
        let source = create_test_source(json!({
            "project": "test/project"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        let fetch_result = result.expect("the metadata is fetched");
        assert_eq!(fetch_result.items.len(), 1);
        assert!(fetch_result.items[0].metadata_only);
    }

    #[test]
    fn test_gitlab_adapter_supports_incremental() {
        let adapter = GitLabAdapter::new();
        assert!(adapter.supports_incremental());
    }

    #[test]
    fn test_gitlab_adapter_rate_limit_config() {
        let adapter = GitLabAdapter::new();
        let config = adapter.rate_limit_config();
        assert_eq!(config.requests_per_second, 1.0);
        assert!(config.retry_after_429);
    }

    #[test]
    fn test_validate_path() {
        assert!(GitLabAdapter::validate_path("src/main.rs").is_ok());
        assert!(GitLabAdapter::validate_path("README.md").is_ok());
        assert!(GitLabAdapter::validate_path("path/to/file.txt").is_ok());

        assert!(GitLabAdapter::validate_path("../etc/passwd").is_err());
        assert!(GitLabAdapter::validate_path("src/../etc/passwd").is_err());
        assert!(GitLabAdapter::validate_path("src/../../file").is_err());

        assert!(GitLabAdapter::validate_path("/etc/passwd").is_err());
        assert!(GitLabAdapter::validate_path("/home/user/file").is_err());
    }

    #[tokio::test]
    async fn test_gitlab_adapter_branch_encoding() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/repository/tree.*"))
            .and(query_param("ref", "feature/my-branch"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "path": "README.md",
                    "type": "blob",
                    "mode": "100644",
                    "id": README_SHA
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex("/projects/[^/]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = served(&mock_server);
        let source = create_test_source(json!({
            "project": "test/project",
            "branch": "feature/my-branch"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        assert!(
            result.is_ok(),
            "the tree must be read at feature/my-branch: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_gitlab_adapter_special_chars_in_branch() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/repository/tree.*"))
            .and(query_param("ref", "fix/bug-123&feature=new"))
            .and(query_param_is_missing("feature"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "path": "file.txt",
                    "type": "blob",
                    "mode": "100644",
                    "id": FILE_SHA
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex("/projects/[^/]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = served(&mock_server);

        let source = create_test_source(json!({
            "project": "test/project",
            "branch": "fix/bug-123&feature=new"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        assert!(
            result.is_ok(),
            "the whole branch must reach GitLab as one ref: {result:?}"
        );
    }

    fn adapter_message(error: ContextError) -> String {
        match error {
            ContextError::Adapter {
                source_type,
                message,
            } if source_type == SOURCE_TYPE => message,
            other => panic!("expected a gitlab adapter error, got {other:?}"),
        }
    }

    #[test]
    fn each_answered_status_is_named_by_its_variant() {
        for (error, status) in [
            (GitLabError::NotFound, Some(Status::NOT_FOUND)),
            (
                GitLabError::AuthenticationFailed,
                Some(Status::UNAUTHORIZED),
            ),
            (GitLabError::Forbidden, Some(Status::FORBIDDEN)),
            (GitLabError::refused(502), Some(Status(502))),
            (GitLabError::TooLarge, None),
            (GitLabError::rate_limited(None), None),
        ] {
            assert_eq!(GitLabAdapter::status(&error), status, "{error:?}");
        }
        assert!(GitLabAdapter::limited(&GitLabError::rate_limited(None)));
        assert!(!GitLabAdapter::limited(&GitLabError::NotFound));
    }

    #[test]
    fn a_tree_error_is_written_as_the_old_adapter_wrote_it() {
        for (error, written) in [
            (
                GitLabError::Forbidden,
                "Failed to fetch tree: 403 Forbidden",
            ),
            (
                GitLabError::refused(599),
                "Failed to fetch tree: 599 <unknown status code>",
            ),
            (
                GitLabError::Parse(ParseError::BranchName("@".to_string())),
                "Failed to fetch tree: 404 Not Found",
            ),
            (
                GitLabError::GitLabApi("REPEATED".to_string()),
                "Failed to parse tree: REPEATED",
            ),
            (
                GitLabError::Parse(ParseError::ObjectId("abc".to_string())),
                "Failed to parse tree: Invalid object name: \"abc\"",
            ),
        ] {
            assert_eq!(adapter_message(GitLabAdapter::tree_error(error)), written);
        }
    }

    #[test]
    fn a_blob_error_is_written_as_the_old_adapter_wrote_it() {
        for (error, written) in [
            (
                GitLabError::AuthenticationFailed,
                "Failed to fetch content: 401 Unauthorized",
            ),
            (
                GitLabError::GitLabApi("UNREADABLE".to_string()),
                "Failed to parse content: UNREADABLE",
            ),
            (
                GitLabError::TooLarge,
                "Failed to fetch content: The content is larger than the limit the caller set",
            ),
        ] {
            assert_eq!(
                adapter_message(GitLabAdapter::content_error(error)),
                written
            );
        }
    }

    #[test]
    fn the_tree_version_hashes_only_blob_paths_and_shas() {
        let sha = |value: &str| ObjectId::parse(value).unwrap();
        let entries = [
            TreeEntry::new("README.md", "100644", TreeEntryKind::Blob, sha(README_SHA)),
            TreeEntry::new("src", "040000", TreeEntryKind::Tree, sha(FILE_SHA)),
            TreeEntry::new("link", "120000", TreeEntryKind::Blob, sha(FILE_SHA)),
        ];

        let mut hasher = Sha256::new();
        for (path, sha) in [("README.md", README_SHA), ("link", FILE_SHA)] {
            hasher.update(path.as_bytes());
            hasher.update(sha.as_bytes());
        }
        let expected: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();

        assert_eq!(GitLabAdapter::tree_version(&entries), expected);
    }

    #[tokio::test]
    async fn a_malformed_token_is_a_configuration_error_at_every_read() {
        let mock_server = MockServer::start().await;
        let adapter = served(&mock_server);
        let entry = TreeEntry::new(
            "README.md",
            "100644",
            TreeEntryKind::Blob,
            ObjectId::parse(README_SHA).unwrap(),
        );
        let malformed = Some("bad\ntoken");

        let errors = [
            adapter
                .get_default_branch("group/project", malformed)
                .await
                .unwrap_err(),
            adapter
                .get_tree("group/project", "main", None, malformed)
                .await
                .unwrap_err(),
            adapter
                .get_blob("group/project", &entry, malformed)
                .await
                .unwrap_err(),
        ];

        for error in errors {
            assert!(
                matches!(&error, ContextError::Config(message) if message == "Invalid authorization token format (token not logged for security)"),
                "{error:?}"
            );
        }
        assert!(mock_server.received_requests().await.unwrap().is_empty());
    }

    #[test]
    fn the_consoles_project_id_and_host_keys_name_the_project() {
        let adapter = GitLabAdapter::new();
        let source = create_test_source(json!({
            "project_id": "group/project",
            "host": "https://gitlab.example.com",
            "branch": "main"
        }));

        let config = adapter
            .parse_config(&source)
            .expect("a source the wizard wrote parses");
        assert_eq!(config.project, "group/project");
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://gitlab.example.com")
        );
    }
}
