//! Filesystem adapter for directory-based content sources
//!
//! Provides intelligent file system traversal with pattern matching,
//! binary file detection, and progressive fetching strategies.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::adapters::{ProgressCallback, RateLimitConfig, SourceAdapter, SyncState};
use crate::content::{
    ContentCategory, ContentItem, ContentMetadata, FetchConfig, FetchResult, FetchStrategy,
    TokenBudget, estimate_tokens_from_bytes, prioritize_paths,
};
use crate::error::{ContextError, Result};
use zone_core::Source;

/// Binary file extensions to skip
const BINARY_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".webp", ".svg", ".pdf", ".zip", ".tar",
    ".gz", ".7z", ".rar", ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib", ".bin", ".wasm",
    ".mp3", ".mp4", ".avi", ".mov", ".wav", ".flac", ".ogg", ".ttf", ".otf", ".woff", ".woff2",
    ".eot",
];

/// Maximum file size to process (10MB)
const MAX_FILE_SIZE_BYTES: usize = 10 * 1024 * 1024;

/// Number of bytes to check for binary content detection
const BINARY_CHECK_BYTES: usize = 8192;

/// Configuration for filesystem sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemConfig {
    /// Directory path
    pub path: String,
    /// Whether to recurse into subdirectories
    #[serde(default = "default_recursive")]
    pub recursive: bool,
}

fn default_recursive() -> bool {
    true
}

/// Filesystem source adapter
#[derive(Debug, Default)]
pub struct FilesystemAdapter;

impl FilesystemAdapter {
    /// Create a new filesystem adapter
    pub fn new() -> Self {
        Self
    }

    /// Parse filesystem config from source
    fn parse_config(&self, source: &Source) -> Result<FilesystemConfig> {
        serde_json::from_value(source.config.clone()).map_err(|e| {
            ContextError::InvalidSourceConfig(format!("Invalid filesystem config: {}", e))
        })
    }

    /// Verify that a path is contained within the root directory (path traversal protection)
    fn verify_path_containment(root: &Path, target: &Path) -> Result<()> {
        // Canonicalize both paths to resolve symlinks and relative components
        let canonical_root = root.canonicalize().map_err(|_e| {
            ContextError::InvalidSourceConfig(format!(
                "Cannot canonicalize root path: {}",
                root.display()
            ))
        })?;

        let canonical_target = target.canonicalize().map_err(|_e| {
            ContextError::InvalidSourceConfig(format!(
                "Cannot canonicalize target path: {}",
                target.display()
            ))
        })?;

        // Verify target is within root
        if !canonical_target.starts_with(&canonical_root) {
            return Err(ContextError::PermissionDenied(format!(
                "Path traversal detected: {} is outside {}",
                target.display(),
                root.display()
            )));
        }

        Ok(())
    }

    /// Check if a file is binary based on extension
    fn is_binary_file_by_extension(path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext_str = format!(".{}", ext.to_string_lossy().to_lowercase());
            BINARY_EXTENSIONS.contains(&ext_str.as_str())
        } else {
            false
        }
    }

    /// Check if file content is binary by detecting null bytes
    fn is_binary_content(path: &Path) -> bool {
        // First check extension
        if Self::is_binary_file_by_extension(path) {
            return true;
        }

        // Read first BINARY_CHECK_BYTES and check for null bytes
        if let Ok(mut file) = fs::File::open(path) {
            let mut buffer = vec![0u8; BINARY_CHECK_BYTES];
            if let Ok(bytes_read) = file.read(&mut buffer) {
                // Check for null bytes in the buffer
                return buffer[..bytes_read].contains(&0);
            }
        }

        false
    }

    /// Map file extension to content type
    fn get_content_type(path: &Path) -> String {
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            match ext_str.as_str() {
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
    fn matches_patterns(path: &Path, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }

        let path_str = path.to_string_lossy();

        patterns.iter().any(|pattern| {
            // Try to match the pattern using glob
            if let Ok(glob_pattern) = Pattern::new(pattern) {
                // Match against full path
                if glob_pattern.matches(&path_str) {
                    return true;
                }

                // For patterns like "node_modules/**", also check if any path component matches
                // the base directory name exactly
                if pattern.contains("**") {
                    let pattern_base = pattern.trim_end_matches("/**").trim_end_matches('/');
                    for component in path.components() {
                        if let Some(c_str) = component.as_os_str().to_str() {
                            if c_str == pattern_base {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        })
    }

    /// Check if a file should be included based on patterns
    fn should_include_file(
        path: &Path,
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

    /// Walk directory and collect file paths
    fn collect_files(
        &self,
        root_path: &Path,
        recursive: bool,
        include_patterns: &[String],
        exclude_patterns: &[String],
    ) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        let walker = if recursive {
            WalkDir::new(root_path).follow_links(false)
        } else {
            WalkDir::new(root_path).max_depth(1).follow_links(false)
        };

        for entry in walker {
            let entry_result = entry;
            let entry = entry_result.map_err(|_e| {
                ContextError::InvalidSourceConfig("Error walking directory".to_string())
            })?;

            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Verify path is contained within root (path traversal protection)
            if Self::verify_path_containment(root_path, path).is_err() {
                // Skip paths outside root directory
                continue;
            }

            // Use symlink_metadata to avoid following symlinks
            let metadata = match fs::symlink_metadata(path) {
                Ok(m) => m,
                Err(_) => continue, // Skip files we can't read metadata for
            };

            // Skip symlinks entirely (symlink attack protection)
            if metadata.file_type().is_symlink() {
                continue;
            }

            // Skip binary files
            if Self::is_binary_content(path) {
                continue;
            }

            // Check include/exclude patterns
            if !Self::should_include_file(path, include_patterns, exclude_patterns) {
                continue;
            }

            files.push(path.to_path_buf());
        }

        Ok(files)
    }

    /// Create a ContentItem from a file
    fn create_content_item(
        &self,
        source_id: uuid::Uuid,
        root_path: &Path,
        file_path: &Path,
        metadata_only: bool,
    ) -> Result<ContentItem> {
        // Verify path is contained within root (path traversal protection)
        Self::verify_path_containment(root_path, file_path)?;

        // Use symlink_metadata to avoid following symlinks
        let metadata = fs::symlink_metadata(file_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ContextError::PermissionDenied(format!("Cannot read file metadata: {}", e))
            } else {
                ContextError::Io(e)
            }
        })?;

        // Skip symlinks (should have been filtered earlier, but double-check)
        if metadata.file_type().is_symlink() {
            return Err(ContextError::PermissionDenied(
                "Symlinks are not allowed".to_string(),
            ));
        }

        // Check file size limit
        let file_size = metadata.len() as usize;
        if file_size > MAX_FILE_SIZE_BYTES {
            return Err(ContextError::InvalidSourceConfig(format!(
                "File too large: {} bytes (max: {} bytes)",
                file_size, MAX_FILE_SIZE_BYTES
            )));
        }

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let uri = format!("file://{}", file_path.display());
        let content_type = Self::get_content_type(file_path);

        let modified_at = metadata.modified().ok().and_then(|t| {
            DateTime::from_timestamp(
                t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64,
                0,
            )
        });

        let mut item = ContentItem::new(source_id, ContentCategory::File, uri, file_name)
            .with_content_type(content_type);

        if let Some(modified) = modified_at {
            item = item.with_modified_at(modified);
        }

        // Build metadata
        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string());
        let mut content_metadata = ContentMetadata {
            size_bytes: Some(file_size),
            extension,
            ..Default::default()
        };

        // Detect language from extension
        if let Some(ext) = file_path.extension() {
            let lang = match ext.to_string_lossy().to_lowercase().as_str() {
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

        // Read content if not metadata_only
        if !metadata_only {
            let content = fs::read_to_string(file_path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    ContextError::PermissionDenied(format!("Cannot read file content: {}", e))
                } else {
                    ContextError::Io(e)
                }
            })?;
            item = item.with_content(content);
        }

        Ok(item)
    }
}

#[async_trait]
impl SourceAdapter for FilesystemAdapter {
    fn source_type(&self) -> &str {
        "filesystem"
    }

    fn rate_limit_config(&self) -> RateLimitConfig {
        RateLimitConfig {
            requests_per_second: f64::INFINITY,
            burst_size: u32::MAX,
            retry_after_429: false,
            max_retries: 0,
            backoff_base_ms: 0,
        }
    }

    async fn verify(&self, source: &Source) -> Result<()> {
        let config = self.parse_config(source)?;

        let path = Path::new(&config.path);

        // Check path exists
        if !path.exists() {
            return Err(ContextError::InvalidSourceConfig(format!(
                "Path does not exist: {}",
                config.path
            )));
        }

        // Check path is a directory
        if !path.is_dir() {
            return Err(ContextError::InvalidSourceConfig(format!(
                "Path is not a directory: {}",
                config.path
            )));
        }

        // Check read permissions by trying to read directory
        fs::read_dir(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ContextError::PermissionDenied(format!("Cannot read directory: {}", e))
            } else {
                ContextError::Io(e)
            }
        })?;

        Ok(())
    }

    async fn estimate_tokens(&self, source: &Source) -> Result<usize> {
        let config = self.parse_config(source)?;
        let path = Path::new(&config.path);

        // Use empty patterns to get all files
        let files = self.collect_files(path, config.recursive, &[], &[])?;

        let mut total_tokens = 0;
        for file_path in files {
            if let Ok(metadata) = fs::metadata(&file_path) {
                total_tokens += estimate_tokens_from_bytes(metadata.len() as usize);
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
        let path = Path::new(&config.path);

        let files = self.collect_files(
            path,
            config.recursive,
            &fetch_config.include_patterns,
            &fetch_config.exclude_patterns,
        )?;

        let total_files = files.len();
        let mut result = FetchResult::new(source.id, false);

        match strategy {
            FetchStrategy::Full => {
                progress.on_message(&format!("Fetching {} files", total_files));
                for (idx, file_path) in files.iter().enumerate() {
                    let item = self.create_content_item(source.id, path, file_path, false)?;
                    progress.on_item(&item);
                    result.add_item(item);
                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::MetadataOnly => {
                progress.on_message(&format!("Fetching metadata for {} files", total_files));
                for (idx, file_path) in files.iter().enumerate() {
                    let item = self.create_content_item(source.id, path, file_path, true)?;
                    progress.on_item(&item);
                    result.add_item(item);
                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::Partial { max_tokens } => {
                progress.on_message(&format!("Fetching files (budget: {} tokens)", max_tokens));
                let mut budget = TokenBudget::new(max_tokens);

                for (idx, file_path) in files.iter().enumerate() {
                    // Read file first to get actual token count
                    let item = match self.create_content_item(source.id, path, file_path, false) {
                        Ok(i) => i,
                        Err(_) => continue, // Skip files we can't read
                    };

                    // Check if it fits in budget
                    if budget.can_fit(item.token_count) {
                        budget.try_add(file_path.to_string_lossy(), item.token_count);
                        progress.on_item(&item);
                        result.add_item(item);
                    } else {
                        // Budget exhausted
                        break;
                    }

                    progress.on_progress(idx + 1, Some(total_files));
                }
            }
            FetchStrategy::Progressive { priority_order } => {
                progress.on_message("Fetching files by priority");

                // Build priority scores
                let paths_with_sizes: Vec<(String, Option<usize>)> = files
                    .iter()
                    .map(|p| {
                        let size = fs::symlink_metadata(p).ok().map(|m| m.len() as usize);
                        (p.to_string_lossy().to_string(), size)
                    })
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
                    let file_path = Path::new(&priority_item.path);
                    if file_path.exists() {
                        let item = self.create_content_item(source.id, path, file_path, false)?;
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
        Ok(SyncState {
            source_id: source.id,
            last_sync_at: Some(Utc::now()),
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
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn create_test_source(config: serde_json::Value) -> Source {
        Source {
            id: Uuid::new_v4(),
            name: "Test Filesystem Source".to_string(),
            source_type: zone_core::SourceType::Filesystem,
            category: zone_core::SourceCategory::File,
            config,
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_test_directory() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn create_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let file_path = dir.join(name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file_path
    }

    #[test]
    fn test_filesystem_adapter_source_type() {
        let adapter = FilesystemAdapter::new();
        assert_eq!(adapter.source_type(), "filesystem");
    }

    #[tokio::test]
    async fn test_filesystem_adapter_verify_valid_directory() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_filesystem_adapter_verify_missing_path() {
        let adapter = FilesystemAdapter::new();
        let source = create_test_source(json!({
            "recursive": true
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("Invalid filesystem config"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_verify_path_not_exists() {
        let adapter = FilesystemAdapter::new();
        let source = create_test_source(json!({
            "path": "/nonexistent/path/that/does/not/exist",
            "recursive": true
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("does not exist"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_verify_path_is_file() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        let file_path = create_test_file(temp_dir.path(), "test.txt", "content");

        let source = create_test_source(json!({
            "path": file_path.to_str().unwrap(),
            "recursive": true
        }));

        let result = adapter.verify(&source).await;
        assert!(result.is_err());
        if let Err(ContextError::InvalidSourceConfig(msg)) = result {
            assert!(msg.contains("not a directory"));
        } else {
            panic!("Expected InvalidSourceConfig error");
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_estimate_tokens_empty_dir() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_filesystem_adapter_estimate_tokens_with_files() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file1.txt", "Hello world");
        create_test_file(temp_dir.path(), "file2.txt", "Test content here");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_ok());
        let tokens = result.unwrap();
        assert!(tokens > 0);
    }

    #[tokio::test]
    async fn test_filesystem_adapter_estimate_tokens_excludes_binary() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file.txt", "a".repeat(100).as_str());
        create_test_file(temp_dir.path(), "image.png", "binary data");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_ok());
        let tokens = result.unwrap();
        // Should only count text file (~25 tokens), not binary
        assert!(tokens > 20 && tokens < 35);
    }

    #[tokio::test]
    async fn test_filesystem_adapter_estimate_tokens_respects_patterns() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file.rs", "fn main() {}");
        create_test_file(temp_dir.path(), "file.txt", "text content");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        // Note: estimate_tokens doesn't use FetchConfig patterns, it gets all files
        // This test verifies the base behavior
        let result = adapter.estimate_tokens(&source).await;
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);
    }

    #[tokio::test]
    async fn test_filesystem_adapter_fetch_full() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file1.txt", "Hello");
        create_test_file(temp_dir.path(), "file2.txt", "World");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 2);

        for item in &fetch_result.items {
            assert!(item.content.is_some());
            assert!(!item.metadata_only);
            assert!(item.token_count > 0);
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_fetch_metadata_only() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file1.txt", "Hello");
        create_test_file(temp_dir.path(), "file2.txt", "World");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 2);

        for item in &fetch_result.items {
            assert!(item.content.is_none());
            assert!(item.metadata_only);
            assert_eq!(item.token_count, 0);
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_fetch_partial() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        // Create files with different sizes
        create_test_file(temp_dir.path(), "small.txt", "small");
        create_test_file(temp_dir.path(), "large.txt", &"a".repeat(1000));

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Partial { max_tokens: 10 },
                &progress,
            )
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        // Should stop before fetching all files due to budget
        assert!(fetch_result.items.len() < 2);
    }

    #[tokio::test]
    async fn test_filesystem_adapter_fetch_progressive() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "README.md", "readme content");
        create_test_file(temp_dir.path(), "lib.rs", "fn main() {}");
        create_test_file(temp_dir.path(), "test.rs", "test code");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Progressive {
                    priority_order: vec!["*.md".to_string(), "*.rs".to_string()],
                },
                &progress,
            )
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 3);

        // README should be first (highest priority)
        assert!(fetch_result.items[0].title.contains("README"));
    }

    #[tokio::test]
    async fn test_filesystem_adapter_fetch_recursive() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "root.txt", "root");
        create_test_file(temp_dir.path(), "subdir/nested.txt", "nested");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 2);
    }

    #[tokio::test]
    async fn test_filesystem_adapter_fetch_non_recursive() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "root.txt", "root");
        create_test_file(temp_dir.path(), "subdir/nested.txt", "nested");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": false
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        // Should only get root file, not nested
        assert_eq!(fetch_result.items.len(), 1);
        assert!(fetch_result.items[0].title.contains("root"));
    }

    #[tokio::test]
    async fn test_filesystem_adapter_content_type_detection() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file.rs", "fn main() {}");
        create_test_file(temp_dir.path(), "file.py", "print('hello')");
        create_test_file(temp_dir.path(), "file.md", "# Markdown");
        create_test_file(temp_dir.path(), "file.json", "{}");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await
            .unwrap();

        let items = &result.items;
        assert_eq!(items.len(), 4);

        // Find each file and check content type
        for item in items {
            if item.title.contains(".rs") {
                assert_eq!(item.content_type, "text/rust");
            } else if item.title.contains(".py") {
                assert_eq!(item.content_type, "text/python");
            } else if item.title.contains(".md") {
                assert_eq!(item.content_type, "text/markdown");
            } else if item.title.contains(".json") {
                assert_eq!(item.content_type, "application/json");
            }
        }
    }

    #[test]
    fn test_filesystem_adapter_supports_incremental() {
        let adapter = FilesystemAdapter::new();
        assert!(adapter.supports_incremental());
    }

    #[tokio::test]
    async fn test_filesystem_adapter_incremental_fetch() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file.txt", "content");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        // Get sync state
        let sync_state = adapter.get_sync_state(&source).await;
        assert!(sync_state.is_ok());
        let state = sync_state.unwrap();
        assert!(state.last_sync_at.is_some());
    }

    #[tokio::test]
    async fn test_filesystem_adapter_progress_callback() {
        use std::sync::{Arc, Mutex};

        struct TestProgress {
            items: Arc<Mutex<Vec<String>>>,
            messages: Arc<Mutex<Vec<String>>>,
        }

        impl ProgressCallback for TestProgress {
            fn on_item(&self, item: &ContentItem) {
                self.items.lock().unwrap().push(item.title.clone());
            }
            fn on_progress(&self, _current: usize, _total: Option<usize>) {}
            fn on_message(&self, message: &str) {
                self.messages.lock().unwrap().push(message.to_string());
            }
        }

        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "file1.txt", "content1");
        create_test_file(temp_dir.path(), "file2.txt", "content2");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = TestProgress {
            items: Arc::new(Mutex::new(Vec::new())),
            messages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let items = progress.items.lock().unwrap();
        assert_eq!(items.len(), 2);

        let messages = progress.messages.lock().unwrap();
        assert!(!messages.is_empty());
    }

    #[tokio::test]
    async fn test_filesystem_adapter_path_traversal_blocked() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "safe.txt", "safe content");

        // Create a file outside the temp dir to attempt traversal to
        let outside_dir = create_test_directory();
        let outside_file = create_test_file(outside_dir.path(), "secret.txt", "secret");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        // Attempt to create content item with path outside root
        let result = adapter.create_content_item(source.id, temp_dir.path(), &outside_file, false);

        // Should be denied
        assert!(result.is_err());
        match result {
            Err(ContextError::PermissionDenied(msg)) => {
                assert!(msg.contains("Path traversal detected"));
            }
            _ => panic!("Expected PermissionDenied error"),
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_symlink_excluded() {
        use std::os::unix::fs as unix_fs;

        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "real.txt", "real content");

        // Create a symlink
        let symlink_path = temp_dir.path().join("link.txt");
        let target_path = temp_dir.path().join("real.txt");
        unix_fs::symlink(&target_path, &symlink_path).unwrap();

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();

        // Should only have the real file, not the symlink
        assert_eq!(fetch_result.items.len(), 1);
        assert!(fetch_result.items[0].title.contains("real.txt"));
        assert!(!fetch_result.items[0].title.contains("link.txt"));
    }

    #[tokio::test]
    async fn test_filesystem_adapter_pattern_no_substring_match() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "secret/data.txt", "secret data");
        create_test_file(temp_dir.path(), "secretary.txt", "secretary notes");
        create_test_file(temp_dir.path(), "public.txt", "public data");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        // Pattern should match "secret/**" but NOT "secretary.txt"
        let config = FetchConfig {
            include_patterns: vec!["secret/**".to_string()],
            exclude_patterns: vec![],
            ..Default::default()
        };
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();

        // Should only match files in secret directory
        assert_eq!(fetch_result.items.len(), 1);
        assert!(fetch_result.items[0].title.contains("data.txt"));

        // Verify secretary.txt is NOT included
        for item in &fetch_result.items {
            assert!(!item.title.contains("secretary"));
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_max_file_size() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "small.txt", "small content");

        // Create a file larger than MAX_FILE_SIZE_BYTES (10MB)
        let large_content = "x".repeat(11 * 1024 * 1024); // 11MB
        create_test_file(temp_dir.path(), "large.txt", &large_content);

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        // Should fail when trying to read the large file
        assert!(result.is_err());
        match result {
            Err(ContextError::InvalidSourceConfig(msg)) => {
                assert!(msg.contains("File too large"));
            }
            _ => panic!("Expected InvalidSourceConfig error for large file"),
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_binary_detection_no_extension() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "textfile", "text content");

        // Create a binary file without extension (with null bytes)
        let binary_path = temp_dir.path().join("binaryfile");
        let mut file = fs::File::create(&binary_path).unwrap();
        file.write_all(&[0x00, 0x01, 0x02, 0x03, 0x04]).unwrap();

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;
        let result = adapter
            .fetch(&source, &config, FetchStrategy::Full, &progress)
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();

        // Should only have the text file, binary should be excluded
        assert_eq!(fetch_result.items.len(), 1);
        assert!(fetch_result.items[0].title.contains("textfile"));

        // Verify binary file is NOT included
        for item in &fetch_result.items {
            assert!(!item.title.contains("binaryfile"));
        }
    }

    #[tokio::test]
    async fn test_filesystem_adapter_progressive_respects_priority_order() {
        let adapter = FilesystemAdapter::new();
        let temp_dir = create_test_directory();
        create_test_file(temp_dir.path(), "README.md", "readme content");
        create_test_file(temp_dir.path(), "lib.rs", "fn main() {}");
        create_test_file(temp_dir.path(), "test.rs", "test code");
        create_test_file(temp_dir.path(), "data.json", "{}");

        let source = create_test_source(json!({
            "path": temp_dir.path().to_str().unwrap(),
            "recursive": true
        }));

        let config = FetchConfig::default();
        let progress = NoOpProgress;

        // Request specific priority order: JSON first, then Rust, then Markdown
        let result = adapter
            .fetch(
                &source,
                &config,
                FetchStrategy::Progressive {
                    priority_order: vec![
                        "*.json".to_string(),
                        "*.rs".to_string(),
                        "*.md".to_string(),
                    ],
                },
                &progress,
            )
            .await;

        assert!(result.is_ok());
        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.items.len(), 4);

        // Verify order: data.json should be first, README.md should be last
        assert!(fetch_result.items[0].title.contains("data.json"));
        assert!(fetch_result.items[3].title.contains("README.md"));
    }
}
