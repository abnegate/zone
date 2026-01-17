//! GitLab adapter for repository-based content sources
//!
//! Provides intelligent repository traversal with pattern matching,
//! binary file detection, and progressive fetching strategies.

use async_trait::async_trait;
use base64::Engine;
use chrono::Utc;
use glob::Pattern;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::adapters::{ProgressCallback, RateLimitConfig, SourceAdapter, SyncState};
use crate::content::{
    ContentCategory, ContentItem, ContentMetadata, FetchConfig, FetchResult, FetchStrategy,
    TokenBudget, estimate_tokens_from_bytes, prioritize_paths,
};
use crate::error::{ContextError, Result};
use zone_core::Source;

/// Binary file extensions to skip (same as GitHub/Filesystem)
const BINARY_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".webp", ".svg", ".pdf", ".zip", ".tar",
    ".gz", ".7z", ".rar", ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib", ".bin", ".wasm",
    ".mp3", ".mp4", ".avi", ".mov", ".wav", ".flac", ".ogg", ".ttf", ".otf", ".woff", ".woff2",
    ".eot",
];

/// Maximum file size in bytes (10MB)
#[allow(dead_code)]
const MAX_FILE_SIZE_BYTES: usize = 10 * 1024 * 1024;

/// Configuration for GitLab sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLabConfig {
    /// Project path (e.g., "owner/project") or numeric project ID
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
    /// Base URL for self-hosted GitLab (defaults to gitlab.com)
    #[serde(default)]
    pub base_url: Option<String>,
}

/// GitLab tree entry from API
#[derive(Debug, Clone, Deserialize)]
struct GitLabTreeEntry {
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    #[allow(dead_code)]
    mode: String,
    id: String,
}

/// GitLab file response
#[derive(Debug, Clone, Deserialize)]
struct GitLabFile {
    #[allow(dead_code)]
    file_name: String,
    #[allow(dead_code)]
    file_path: String,
    size: usize,
    #[allow(dead_code)]
    encoding: String,
    content: Option<String>,
    #[allow(dead_code)]
    content_sha256: String,
    #[allow(dead_code)]
    ref_: Option<String>,
    #[allow(dead_code)]
    blob_id: String,
}

/// GitLab project response
#[derive(Debug, Clone, Deserialize)]
struct GitLabProject {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    name: String,
    default_branch: String,
}

/// Cached tree entry with LRU tracking
#[derive(Debug, Clone)]
struct CachedTree {
    tree: Vec<GitLabTreeEntry>,
    cached_at: Instant,
    last_accessed: Instant,
    size_bytes: usize,
}

/// Cache key for GitLab trees (project_id, branch)
type TreeCacheKey = (String, String);

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

    /// Estimate size of a tree in bytes
    fn estimate_tree_size(tree: &[GitLabTreeEntry]) -> usize {
        tree.iter()
            .map(|entry| {
                40 + // id
                entry.path.len() +
                20 + // type string + overhead
                10 // mode
            })
            .sum()
    }

    async fn get(&self, key: &TreeCacheKey) -> Option<Vec<GitLabTreeEntry>> {
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

    async fn insert(&self, key: TreeCacheKey, tree: Vec<GitLabTreeEntry>) {
        let mut entries = self.entries.write().await;
        let tree_size = Self::estimate_tree_size(&tree);

        let mut total_size: usize = entries.values().map(|e| e.size_bytes).sum();

        // Evict LRU entries if needed
        while (entries.len() >= self.max_entries || total_size + tree_size > self.max_total_bytes)
            && !entries.is_empty()
        {
            if let Some((lru_key, _)) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(k, v)| (k.clone(), v.size_bytes))
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

/// GitLab source adapter
#[derive(Debug, Clone)]
pub struct GitLabAdapter {
    base_url: String,
    tree_cache: TreeCache,
}

impl Default for GitLabAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitLabAdapter {
    /// Tree cache TTL in seconds (5 minutes)
    const TREE_CACHE_TTL_SECS: u64 = 300;

    /// Create a new GitLab adapter with the default GitLab API URL
    pub fn new() -> Self {
        Self {
            base_url: "https://gitlab.com/api/v4".to_string(),
            tree_cache: TreeCache::new(Self::TREE_CACHE_TTL_SECS),
        }
    }

    /// Create a new GitLab adapter with a custom base URL (for testing or self-hosted)
    pub fn with_base_url(base_url: String) -> Self {
        Self {
            base_url,
            tree_cache: TreeCache::new(Self::TREE_CACHE_TTL_SECS),
        }
    }

    /// Validate GitLab identifier to prevent URL injection
    fn validate_gitlab_identifier(identifier: &str, field_name: &str) -> Result<()> {
        if identifier.is_empty() {
            return Err(ContextError::InvalidSourceConfig(format!(
                "{} cannot be empty",
                field_name
            )));
        }

        // Check for injection characters (but allow / for project paths)
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

    /// Validate path to prevent path traversal attacks
    fn validate_path(path: &str) -> Result<()> {
        // Block path traversal
        if path.contains("..") {
            return Err(ContextError::InvalidSourceConfig(
                "Path cannot contain '..'".to_string(),
            ));
        }
        // Block absolute paths
        if path.starts_with('/') {
            return Err(ContextError::InvalidSourceConfig(
                "Path cannot be absolute".to_string(),
            ));
        }
        Ok(())
    }

    /// Parse GitLab config from source
    fn parse_config(&self, source: &Source) -> Result<GitLabConfig> {
        serde_json::from_value(source.config.clone())
            .map_err(|e| ContextError::InvalidSourceConfig(format!("Invalid GitLab config: {}", e)))
    }

    /// Build HTTP client with GitLab API configuration
    fn build_client(&self, token: Option<&str>) -> Result<reqwest::Client> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            "zone-context"
                .parse()
                .map_err(|e| ContextError::Config(format!("Invalid header: {}", e)))?,
        );

        if let Some(token) = token {
            headers.insert(
                "PRIVATE-TOKEN",
                token.parse().map_err(|_| {
                    ContextError::Config(
                        "Invalid authorization token format (token not logged for security)"
                            .to_string(),
                    )
                })?,
            );
        }

        reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ContextError::Config(format!("Failed to build HTTP client: {}", e)))
    }

    /// Fetch with retry logic for rate limiting
    async fn fetch_with_retry(
        &self,
        url: &str,
        client: &reqwest::Client,
    ) -> Result<reqwest::Response> {
        let config = self.rate_limit_config();
        let mut retries = 0;

        loop {
            let response =
                client.get(url).send().await.map_err(|e| {
                    ContextError::adapter("gitlab", format!("Request failed: {}", e))
                })?;

            // Check for rate limiting
            let is_rate_limited = response.status() == 429;

            if is_rate_limited && retries < config.max_retries {
                retries += 1;
                let backoff_ms = config.backoff_base_ms * (2_u64.pow(retries - 1));
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                continue;
            }

            if is_rate_limited {
                return Err(ContextError::RateLimited {
                    retry_after_secs: 60,
                });
            }

            return Ok(response);
        }
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

    /// Map file extension to content type (same as GitHub/Filesystem)
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

        true
    }

    /// URL-encode project ID for GitLab API (handles both numeric IDs and paths)
    fn encode_project_id(project: &str) -> String {
        utf8_percent_encode(project, NON_ALPHANUMERIC).to_string()
    }

    /// Fetch project info to get default branch
    async fn get_project_info(&self, project: &str, token: Option<&str>) -> Result<GitLabProject> {
        Self::validate_gitlab_identifier(project, "project")?;

        let client = self.build_client(token)?;
        let encoded_project = Self::encode_project_id(project);
        let url = format!("{}/projects/{}", self.base_url, encoded_project);

        let response = self.fetch_with_retry(&url, &client).await?;

        if response.status() == 404 {
            return Err(ContextError::InvalidSourceConfig(format!(
                "Project not found: {}",
                project
            )));
        } else if response.status() == 403 || response.status() == 401 {
            return Err(ContextError::PermissionDenied(
                "Access forbidden - check token permissions".to_string(),
            ));
        } else if !response.status().is_success() {
            return Err(ContextError::adapter(
                "gitlab",
                format!("API error: {}", response.status()),
            ));
        }

        response.json::<GitLabProject>().await.map_err(|e| {
            ContextError::adapter("gitlab", format!("Failed to parse project info: {}", e))
        })
    }

    /// Fetch repository tree (recursive)
    async fn get_tree(
        &self,
        project: &str,
        branch: &str,
        path: Option<&str>,
        token: Option<&str>,
    ) -> Result<Vec<GitLabTreeEntry>> {
        Self::validate_gitlab_identifier(project, "project")?;
        Self::validate_gitlab_identifier(branch, "branch")?;
        if let Some(p) = path
            && !p.is_empty()
        {
            Self::validate_path(p)?;
        }

        let client = self.build_client(token)?;
        let encoded_project = Self::encode_project_id(project);
        let encoded_branch = utf8_percent_encode(branch, NON_ALPHANUMERIC).to_string();

        let mut url = format!(
            "{}/projects/{}/repository/tree?ref={}&recursive=true&per_page=100",
            self.base_url, encoded_project, encoded_branch
        );

        if let Some(p) = path
            && !p.is_empty()
        {
            url.push_str(&format!(
                "&path={}",
                utf8_percent_encode(p, NON_ALPHANUMERIC)
            ));
        }

        // GitLab paginates results, so we may need to fetch multiple pages
        let mut all_entries = Vec::new();
        let mut current_url = url;

        loop {
            let response = self.fetch_with_retry(&current_url, &client).await?;

            if !response.status().is_success() {
                return Err(ContextError::adapter(
                    "gitlab",
                    format!("Failed to fetch tree: {}", response.status()),
                ));
            }

            // Check for next page link
            let next_link = response
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .and_then(|links| {
                    links.split(',').find_map(|link| {
                        if link.contains("rel=\"next\"") {
                            link.split(';').next().map(|url| {
                                url.trim().trim_matches('<').trim_matches('>').to_string()
                            })
                        } else {
                            None
                        }
                    })
                });

            // Get response text first for better error reporting in tests
            let response_text = response.text().await.map_err(|e| {
                ContextError::adapter("gitlab", format!("Failed to read response: {}", e))
            })?;

            let entries: Vec<GitLabTreeEntry> =
                serde_json::from_str(&response_text).map_err(|e| {
                    #[cfg(test)]
                    eprintln!("Failed to parse JSON. Response was: {}", response_text);
                    ContextError::adapter("gitlab", format!("Failed to parse tree: {}", e))
                })?;

            all_entries.extend(entries);

            // Check if there's a next page
            if let Some(next) = next_link {
                current_url = next;
            } else {
                break;
            }
        }

        Ok(all_entries)
    }

    /// Get tree with caching and in-flight tracking
    async fn get_tree_cached(
        &self,
        project: &str,
        branch: &str,
        path: Option<&str>,
        token: Option<&str>,
    ) -> Result<Vec<GitLabTreeEntry>> {
        let key = (project.to_string(), branch.to_string());

        // Check cache first
        if let Some(tree) = self.tree_cache.get(&key).await {
            return Ok(tree);
        }

        // Check if already being fetched
        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            if in_flight.contains(&key) {
                drop(in_flight);

                // Wait for in-flight fetch to complete
                for _ in 0..10 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    if let Some(tree) = self.tree_cache.get(&key).await {
                        return Ok(tree);
                    }
                }

                // After wait loop times out, check if still in flight
                {
                    let in_flight = self.tree_cache.in_flight.read().await;
                    if in_flight.contains(&key) {
                        // Still in flight - another request is handling it, but taking too long
                        return Err(ContextError::Timeout {
                            operation: "tree cache wait".to_string(),
                            timeout_ms: 1000,
                        });
                    }
                }
                // If no longer in flight, fall through to fetch
            } else {
                in_flight.insert(key.clone());
            }
        }

        // Fetch from API
        let fetch_result = self.get_tree(project, branch, path, token).await;

        // Remove from in-flight
        {
            let mut in_flight = self.tree_cache.in_flight.write().await;
            in_flight.remove(&key);
        }

        let tree = fetch_result?;

        // Store in cache
        self.tree_cache.insert(key, tree.clone()).await;

        Ok(tree)
    }

    /// Fetch file content
    async fn get_file_content(
        &self,
        project: &str,
        path: &str,
        branch: &str,
        token: Option<&str>,
    ) -> Result<GitLabFile> {
        Self::validate_gitlab_identifier(project, "project")?;
        Self::validate_gitlab_identifier(branch, "branch")?;
        Self::validate_path(path)?;

        let client = self.build_client(token)?;
        let encoded_project = Self::encode_project_id(project);
        let encoded_path = utf8_percent_encode(path, NON_ALPHANUMERIC).to_string();
        let encoded_branch = utf8_percent_encode(branch, NON_ALPHANUMERIC).to_string();

        let url = format!(
            "{}/projects/{}/repository/files/{}?ref={}",
            self.base_url, encoded_project, encoded_path, encoded_branch
        );

        let response = self.fetch_with_retry(&url, &client).await?;

        if !response.status().is_success() {
            return Err(ContextError::adapter(
                "gitlab",
                format!("Failed to fetch content: {}", response.status()),
            ));
        }

        response
            .json::<GitLabFile>()
            .await
            .map_err(|e| ContextError::adapter("gitlab", format!("Failed to parse content: {}", e)))
    }

    /// Decode base64 content
    fn decode_content(content: &str) -> Result<String> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(content.replace('\n', ""))
            .map_err(|e| ContextError::Parse(format!("Failed to decode base64: {}", e)))?;

        String::from_utf8(decoded)
            .map_err(|e| ContextError::Parse(format!("Failed to decode UTF-8: {}", e)))
    }

    /// Create a ContentItem from a GitLab file
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
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();

        let uri = format!("gitlab://{}/{}@{}", project, path, branch);
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
            commit_hash: Some(blob_id.to_string()),
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

    /// Estimate file size from tree entry (GitLab doesn't provide size in tree API)
    fn estimate_file_size(_entry: &GitLabTreeEntry) -> usize {
        // GitLab tree API doesn't include file sizes, so we use a conservative estimate
        // This will be updated when we fetch the actual file
        2048 // 2KB default estimate
    }
}

#[async_trait]
impl SourceAdapter for GitLabAdapter {
    fn source_type(&self) -> &str {
        "gitlab"
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        // GitLab rate limits: varies by plan, but conservative default
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

        // Apply base_url if provided
        if config.base_url.is_some() {
            // Validation happens in get_project_info
        }

        // Validate required fields
        if config.project.is_empty() {
            return Err(ContextError::InvalidSourceConfig(
                "project is required".to_string(),
            ));
        }

        Self::validate_gitlab_identifier(&config.project, "project")?;
        if let Some(ref branch) = config.branch {
            Self::validate_gitlab_identifier(branch, "branch")?;
        }

        // Try to fetch project info to verify it exists and is accessible
        let _ = self
            .get_project_info(&config.project, config.token.as_deref())
            .await?;

        Ok(())
    }

    async fn estimate_tokens(&self, source: &Source) -> Result<usize> {
        let config = self.parse_config(source)?;

        // Get project info to determine branch
        let project_info = self
            .get_project_info(&config.project, config.token.as_deref())
            .await?;

        let branch = config
            .branch
            .as_deref()
            .unwrap_or(&project_info.default_branch);

        // Fetch tree (with caching)
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
            // Skip directories
            if entry.entry_type != "blob" {
                continue;
            }

            // Skip binary files
            if Self::is_binary_file(&entry.path) {
                continue;
            }

            // Apply path filter if specified
            if let Some(ref path_filter) = config.path
                && !path_filter.is_empty()
                && !entry.path.starts_with(path_filter)
            {
                continue;
            }

            // Estimate tokens from size (using conservative estimate)
            let size = Self::estimate_file_size(entry);
            total_tokens += estimate_tokens_from_bytes(size);
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

        // Get project info to determine branch
        let project_info = self
            .get_project_info(&config.project, config.token.as_deref())
            .await?;

        let branch = config
            .branch
            .as_deref()
            .unwrap_or(&project_info.default_branch)
            .to_string();

        // Fetch tree (with caching)
        let tree = self
            .get_tree_cached(
                &config.project,
                &branch,
                config.path.as_deref(),
                config.token.as_deref(),
            )
            .await?;

        // Filter files
        let files: Vec<&GitLabTreeEntry> = tree
            .iter()
            .filter(|entry| {
                // Only blobs (files)
                if entry.entry_type != "blob" {
                    return false;
                }

                // Skip binary files
                if Self::is_binary_file(&entry.path) {
                    return false;
                }

                // Apply path filter
                if let Some(ref path_filter) = config.path
                    && !path_filter.is_empty()
                    && !entry.path.starts_with(path_filter)
                {
                    return false;
                }

                // Apply include/exclude patterns
                Self::should_include_file(
                    &entry.path,
                    &fetch_config.include_patterns,
                    &fetch_config.exclude_patterns,
                )
            })
            .collect();

        let total_files = files.len();
        let mut result = FetchResult::new(source.id, false);

        match strategy {
            FetchStrategy::Full => {
                progress.on_message(&format!("Fetching {} files from GitLab", total_files));
                for (idx, entry) in files.iter().enumerate() {
                    let file_content = self
                        .get_file_content(
                            &config.project,
                            &entry.path,
                            &branch,
                            config.token.as_deref(),
                        )
                        .await?;

                    let content = if let Some(ref encoded) = file_content.content {
                        Some(Self::decode_content(encoded)?)
                    } else {
                        None
                    };

                    let item = Self::create_content_item(
                        source.id,
                        &config.project,
                        &branch,
                        &entry.path,
                        &entry.id,
                        file_content.size,
                        content,
                        false,
                    )?;

                    progress.on_item(&item);
                    result.add_item(item);
                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::MetadataOnly => {
                progress.on_message(&format!(
                    "Fetching metadata for {} files from GitLab",
                    total_files
                ));
                for (idx, entry) in files.iter().enumerate() {
                    let item = Self::create_content_item(
                        source.id,
                        &config.project,
                        &branch,
                        &entry.path,
                        &entry.id,
                        Self::estimate_file_size(entry),
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
                    "Fetching files from GitLab (budget: {} tokens)",
                    max_tokens
                ));
                let mut budget = TokenBudget::new(max_tokens);

                for (idx, entry) in files.iter().enumerate() {
                    let estimated_tokens =
                        estimate_tokens_from_bytes(Self::estimate_file_size(entry));

                    if budget.can_fit(estimated_tokens) {
                        let file_content = self
                            .get_file_content(
                                &config.project,
                                &entry.path,
                                &branch,
                                config.token.as_deref(),
                            )
                            .await?;

                        let content = if let Some(ref encoded) = file_content.content {
                            Some(Self::decode_content(encoded)?)
                        } else {
                            None
                        };

                        let item = Self::create_content_item(
                            source.id,
                            &config.project,
                            &branch,
                            &entry.path,
                            &entry.id,
                            file_content.size,
                            content,
                            false,
                        )?;

                        budget.try_add(&entry.path, item.token_count);
                        progress.on_item(&item);
                        result.add_item(item);
                    } else {
                        break;
                    }

                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::Progressive { priority_order } => {
                progress.on_message("Fetching files from GitLab by priority");

                // Build priority scores
                let paths_with_sizes: Vec<(String, Option<usize>)> = files
                    .iter()
                    .map(|entry| (entry.path.clone(), Some(Self::estimate_file_size(entry))))
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
                    if let Some(entry) = files.iter().find(|e| e.path == priority_item.path) {
                        let file_content = self
                            .get_file_content(
                                &config.project,
                                &entry.path,
                                &branch,
                                config.token.as_deref(),
                            )
                            .await?;

                        let content = if let Some(ref encoded) = file_content.content {
                            Some(Self::decode_content(encoded)?)
                        } else {
                            None
                        };

                        let item = Self::create_content_item(
                            source.id,
                            &config.project,
                            &branch,
                            &entry.path,
                            &entry.id,
                            file_content.size,
                            content,
                            false,
                        )?;

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

        // Get project info to determine branch
        let project_info = self
            .get_project_info(&config.project, config.token.as_deref())
            .await?;

        let branch = config
            .branch
            .as_deref()
            .unwrap_or(&project_info.default_branch);

        // For GitLab, we could fetch the latest commit SHA, but for now we'll use timestamp
        Ok(SyncState {
            source_id: source.id,
            last_sync_at: Some(Utc::now()),
            version: Some(branch.to_string()),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::NoOpProgress;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;
    use wiremock::matchers::{header, method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    #[test]
    fn test_gitlab_adapter_source_type() {
        let adapter = GitLabAdapter::new();
        assert_eq!(adapter.source_type(), "gitlab");
    }

    #[test]
    fn test_validate_gitlab_identifier() {
        // Valid identifiers
        assert!(GitLabAdapter::validate_gitlab_identifier("myproject", "project").is_ok());
        assert!(GitLabAdapter::validate_gitlab_identifier("owner/project", "project").is_ok());
        assert!(GitLabAdapter::validate_gitlab_identifier("123", "project").is_ok());

        // Invalid identifiers
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

        // Mock project info endpoint
        Mock::given(method("GET"))
            .and(path_regex("/projects/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = GitLabAdapter::with_base_url(mock_server.uri());
        let source = create_test_source(json!({
            "project": "test-owner/test-project"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_ok());
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

        let adapter = GitLabAdapter::with_base_url(mock_server.uri());
        let source = create_test_source(json!({
            "project": "test-owner/test-project",
            "token": "test-token"
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_ok());
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
    fn test_decode_content() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("Hello, World!");
        let decoded = GitLabAdapter::decode_content(&encoded).unwrap();
        assert_eq!(decoded, "Hello, World!");
    }

    #[test]
    fn test_create_content_item() {
        let source_id = Uuid::new_v4();
        let item = GitLabAdapter::create_content_item(
            source_id,
            "owner/project",
            "main",
            "src/lib.rs",
            "abc123",
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

        // Mock tree endpoint FIRST (more specific match)
        Mock::given(method("GET"))
            .and(path_regex(r".*/repository/tree.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "path": "README.md",
                    "type": "blob",
                    "mode": "100644",
                    "id": "abc123"
                }
            ])))
            .mount(&mock_server)
            .await;

        // Mock project info (less specific match, mounted after)
        Mock::given(method("GET"))
            .and(path_regex("/projects/[^/]+"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = GitLabAdapter::with_base_url(mock_server.uri());
        let source = create_test_source(json!({
            "project": "test/project"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        if let Err(ref e) = result {
            eprintln!("Test failed with error: {:?}", e);
        }
        assert!(result.is_ok());
        let fetch_result = result.unwrap();
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
        // Valid paths
        assert!(GitLabAdapter::validate_path("src/main.rs").is_ok());
        assert!(GitLabAdapter::validate_path("README.md").is_ok());
        assert!(GitLabAdapter::validate_path("path/to/file.txt").is_ok());

        // Invalid paths with traversal
        assert!(GitLabAdapter::validate_path("../etc/passwd").is_err());
        assert!(GitLabAdapter::validate_path("src/../etc/passwd").is_err());
        assert!(GitLabAdapter::validate_path("src/../../file").is_err());

        // Invalid absolute paths
        assert!(GitLabAdapter::validate_path("/etc/passwd").is_err());
        assert!(GitLabAdapter::validate_path("/home/user/file").is_err());
    }

    #[tokio::test]
    async fn test_gitlab_adapter_branch_encoding() {
        let mock_server = MockServer::start().await;

        // Mock tree endpoint with encoded branch name (more specific, must come first)
        Mock::given(method("GET"))
            .and(path_regex(r".*/repository/tree.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "path": "README.md",
                    "type": "blob",
                    "mode": "100644",
                    "id": "abc123"
                }
            ])))
            .mount(&mock_server)
            .await;

        // Mock project info endpoint (less specific)
        Mock::given(method("GET"))
            .and(path_regex("/projects/[^/]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = GitLabAdapter::with_base_url(mock_server.uri());
        let source = create_test_source(json!({
            "project": "test/project",
            "branch": "feature/my-branch"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        // Should succeed with properly encoded branch name
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_gitlab_adapter_special_chars_in_branch() {
        let mock_server = MockServer::start().await;

        // Mock tree endpoint (more specific, must come first)
        Mock::given(method("GET"))
            .and(path_regex(r".*/repository/tree.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "path": "file.txt",
                    "type": "blob",
                    "mode": "100644",
                    "id": "xyz789"
                }
            ])))
            .mount(&mock_server)
            .await;

        // Mock project info endpoint (less specific)
        Mock::given(method("GET"))
            .and(path_regex("/projects/[^/]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "name": "test-project",
                "default_branch": "main"
            })))
            .mount(&mock_server)
            .await;

        let adapter = GitLabAdapter::with_base_url(mock_server.uri());

        // Test branch name with special characters
        let source = create_test_source(json!({
            "project": "test/project",
            "branch": "fix/bug-123&feature=new"
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        // Should succeed - branch name should be properly URL-encoded
        assert!(result.is_ok());
    }
}
