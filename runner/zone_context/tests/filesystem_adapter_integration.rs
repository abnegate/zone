//! Integration tests for FilesystemAdapter

use chrono::Utc;
use serde_json::json;
use std::fs;
use std::io::Write;
use tempfile::TempDir;
use uuid::Uuid;
use zone_context::adapters::{FilesystemAdapter, NoOpProgress, SourceAdapter};
use zone_context::{ContentCategory, FetchConfig, FetchStrategy};
use zone_core::{Source, SourceCategory, SourceType};

fn create_filesystem_source(path: &str, recursive: bool) -> Source {
    Source {
        id: Uuid::new_v4(),
        name: "Test Filesystem Source".to_string(),
        source_type: SourceType::Filesystem,
        category: SourceCategory::File,
        config: json!({
            "path": path,
            "recursive": recursive
        }),
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn create_test_file(dir: &std::path::Path, name: &str, content: &str) {
    let file_path = dir.join(name);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut file = fs::File::create(&file_path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

#[tokio::test]
async fn test_filesystem_fetch_real_directory() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    // Create a realistic directory structure
    create_test_file(
        temp_dir.path(),
        "README.md",
        "# Test Project\n\nThis is a test project.",
    );
    create_test_file(temp_dir.path(), "Cargo.toml", "[package]\nname = \"test\"");
    create_test_file(
        temp_dir.path(),
        "src/main.rs",
        "fn main() {\n    println!(\"Hello\");\n}",
    );
    create_test_file(
        temp_dir.path(),
        "src/lib.rs",
        "pub fn hello() -> String {\n    \"world\".to_string()\n}",
    );
    create_test_file(
        temp_dir.path(),
        "tests/integration.rs",
        "#[test]\nfn test_it() {}",
    );

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    // Step 1: Verify the source
    let verify_result = adapter.verify(&source).await;
    assert!(verify_result.is_ok(), "Verify should succeed");

    // Step 2: Estimate tokens
    let token_estimate = adapter.estimate_tokens(&source).await.unwrap();
    assert!(token_estimate > 0, "Token estimate should be positive");
    assert!(
        token_estimate < 500,
        "Small project should have reasonable token count"
    );

    // Step 3: Fetch full content
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 5, "Should fetch exactly 5 files");

    // Verify all files have content
    for item in &result.items {
        assert!(item.content.is_some(), "All items should have content");
        assert!(!item.metadata_only, "No items should be metadata only");
        assert!(item.token_count > 0, "All items should have token count");
        assert_eq!(item.category, ContentCategory::File);
    }

    // Verify fetch stats
    assert_eq!(result.stats.items_fetched, 5);
    assert!(result.stats.total_tokens > 0);
    assert_eq!(result.stats.metadata_only_count, 0);
}

#[tokio::test]
async fn test_filesystem_large_directory_sizing() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    // Create multiple files with varying sizes
    create_test_file(temp_dir.path(), "small.txt", "small");
    create_test_file(temp_dir.path(), "medium.txt", &"a".repeat(1000));
    create_test_file(temp_dir.path(), "large.txt", &"b".repeat(5000));

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    // Estimate tokens
    let token_estimate = adapter.estimate_tokens(&source).await.unwrap();

    // Should be approximately (5 + 1000 + 5000) / 4 = ~1500 tokens
    assert!(
        token_estimate > 1400 && token_estimate < 1600,
        "Token estimate should be around 1500, got {}",
        token_estimate
    );

    // Fetch with metadata only strategy
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 3);
    for item in &result.items {
        assert!(
            item.content.is_none(),
            "Metadata-only should have no content"
        );
        assert!(item.metadata_only);
        assert!(
            item.metadata.size_bytes.is_some(),
            "Should have size metadata"
        );
    }
}

#[tokio::test]
async fn test_filesystem_exclude_patterns() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    // Create files that should be excluded
    create_test_file(temp_dir.path(), "src/main.rs", "fn main() {}");
    create_test_file(
        temp_dir.path(),
        "node_modules/package/index.js",
        "module.exports = {}",
    );
    create_test_file(temp_dir.path(), ".git/config", "git config");
    create_test_file(temp_dir.path(), "target/debug/app", "binary");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    // Default FetchConfig has node_modules, .git, target in exclude patterns
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    // Should only get main.rs, not the excluded files
    assert_eq!(
        result.items.len(),
        1,
        "Should only fetch non-excluded files"
    );
    assert!(result.items[0].title.contains("main.rs"));
}

#[tokio::test]
async fn test_filesystem_registry_integration() {
    use zone_context::adapters::AdapterRegistry;

    let mut registry = AdapterRegistry::new();
    registry.register(FilesystemAdapter::new());

    let temp_dir = TempDir::new().unwrap();
    create_test_file(temp_dir.path(), "test.txt", "test content");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    // Get adapter from registry
    let adapter = registry.get("filesystem");
    assert!(
        adapter.is_some(),
        "Should find filesystem adapter in registry"
    );

    let adapter = adapter.unwrap();
    assert_eq!(adapter.source_type(), "filesystem");

    // Verify it works through the registry
    let verify_result = adapter.verify(&source).await;
    assert!(verify_result.is_ok());

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 1);
}

#[tokio::test]
async fn test_filesystem_binary_file_exclusion() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    // Create various file types
    create_test_file(temp_dir.path(), "document.txt", "text content");
    create_test_file(temp_dir.path(), "image.png", "PNG binary data");
    create_test_file(temp_dir.path(), "archive.zip", "ZIP binary data");
    create_test_file(temp_dir.path(), "video.mp4", "MP4 binary data");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    // Should only get the text file, not binary files
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].title.contains("document.txt"));
}

#[tokio::test]
async fn test_filesystem_content_types() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    create_test_file(temp_dir.path(), "file.rs", "fn main() {}");
    create_test_file(temp_dir.path(), "file.py", "print('hello')");
    create_test_file(temp_dir.path(), "file.js", "console.log('hi')");
    create_test_file(temp_dir.path(), "file.ts", "const x: string = 'test';");
    create_test_file(temp_dir.path(), "file.md", "# Markdown");
    create_test_file(temp_dir.path(), "file.json", "{}");
    create_test_file(temp_dir.path(), "file.yaml", "key: value");
    create_test_file(temp_dir.path(), "file.toml", "[section]");
    create_test_file(temp_dir.path(), "file.html", "<html></html>");
    create_test_file(temp_dir.path(), "file.css", "body { }");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 10);

    // Verify content types
    for item in &result.items {
        if item.title.ends_with(".rs") {
            assert_eq!(item.content_type, "text/rust");
        } else if item.title.ends_with(".py") {
            assert_eq!(item.content_type, "text/python");
        } else if item.title.ends_with(".json") {
            assert_eq!(item.content_type, "application/json");
        } else if item.title.ends_with(".js") {
            assert_eq!(item.content_type, "text/javascript");
        } else if item.title.ends_with(".ts") {
            assert_eq!(item.content_type, "text/typescript");
        } else if item.title.ends_with(".md") {
            assert_eq!(item.content_type, "text/markdown");
        } else if item.title.ends_with(".yaml") {
            assert_eq!(item.content_type, "application/yaml");
        } else if item.title.ends_with(".toml") {
            assert_eq!(item.content_type, "application/toml");
        } else if item.title.ends_with(".html") {
            assert_eq!(item.content_type, "text/html");
        } else if item.title.ends_with(".css") {
            assert_eq!(item.content_type, "text/css");
        }
    }
}

#[tokio::test]
async fn test_filesystem_progressive_fetch_priority() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    // Create files with different priorities
    create_test_file(temp_dir.path(), "CLAUDE.md", "Claude instructions");
    create_test_file(temp_dir.path(), "README.md", "Project readme");
    create_test_file(temp_dir.path(), "Cargo.toml", "[package]");
    create_test_file(temp_dir.path(), "src/lib.rs", "pub fn test() {}");
    create_test_file(temp_dir.path(), "src/main.rs", "fn main() {}");
    create_test_file(temp_dir.path(), "src/utils/helper.rs", "// helper");
    create_test_file(temp_dir.path(), "tests/test.rs", "#[test] fn t() {}");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(
            &source,
            &config,
            FetchStrategy::Progressive {
                priority_order: vec!["*.md".to_string(), "*.toml".to_string()],
            },
            &progress,
        )
        .await
        .unwrap();

    assert_eq!(result.items.len(), 7);

    // CLAUDE.md should be first (highest priority)
    assert!(result.items[0].title.contains("CLAUDE.md"));

    // README.md should be second
    assert!(result.items[1].title.contains("README.md"));

    // Test files should have lower priority than main code files
    let test_idx = result
        .items
        .iter()
        .position(|i| i.title.contains("test.rs"))
        .unwrap();
    let lib_idx = result
        .items
        .iter()
        .position(|i| i.title.contains("lib.rs"))
        .unwrap();
    assert!(test_idx > lib_idx, "Test files should come after lib.rs");
}

#[tokio::test]
async fn test_filesystem_partial_strategy_respects_budget() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    // Create several files
    for i in 0..10 {
        create_test_file(
            temp_dir.path(),
            &format!("file{}.txt", i),
            &"content ".repeat(20), // ~140 chars = ~35 tokens each
        );
    }

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    let config = FetchConfig::default();
    let progress = NoOpProgress;

    // Set budget to only fit ~2-3 files
    let result = adapter
        .fetch(
            &source,
            &config,
            FetchStrategy::Partial { max_tokens: 100 },
            &progress,
        )
        .await
        .unwrap();

    // Should have fetched less than all files due to budget constraint
    assert!(
        result.items.len() < 10,
        "Partial fetch should respect token budget"
    );
    assert!(result.items.len() >= 2, "Should fetch at least some files");

    // Verify all fetched items have content
    for item in &result.items {
        assert!(item.content.is_some());
        assert!(!item.metadata_only);
    }
}

#[tokio::test]
async fn test_filesystem_incremental_sync_state() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();
    create_test_file(temp_dir.path(), "test.txt", "content");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    // Get sync state
    let sync_state = adapter.get_sync_state(&source).await.unwrap();

    assert_eq!(sync_state.source_id, source.id);
    assert!(
        sync_state.last_sync_at.is_some(),
        "Should have last_sync_at timestamp"
    );
    assert!(
        adapter.supports_incremental(),
        "Should support incremental sync"
    );
}

#[tokio::test]
async fn test_filesystem_metadata_includes_file_info() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    create_test_file(temp_dir.path(), "example.rs", "fn test() {}");

    let source = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);

    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 1);
    let item = &result.items[0];

    // Check metadata
    assert!(item.metadata.size_bytes.is_some(), "Should have file size");
    assert_eq!(item.metadata.extension, Some("rs".to_string()));
    assert_eq!(item.metadata.language, Some("rust".to_string()));
    assert!(item.modified_at.is_some(), "Should have modified timestamp");
    assert!(
        item.uri.starts_with("file://"),
        "URI should use file:// scheme"
    );
}

#[tokio::test]
async fn test_filesystem_recursive_vs_non_recursive() {
    let adapter = FilesystemAdapter::new();
    let temp_dir = TempDir::new().unwrap();

    create_test_file(temp_dir.path(), "root1.txt", "root content 1");
    create_test_file(temp_dir.path(), "root2.txt", "root content 2");
    create_test_file(temp_dir.path(), "subdir/nested1.txt", "nested content 1");
    create_test_file(temp_dir.path(), "subdir/nested2.txt", "nested content 2");
    create_test_file(temp_dir.path(), "subdir/deep/nested3.txt", "deep nested");

    // Test non-recursive
    let source_non_recursive = create_filesystem_source(temp_dir.path().to_str().unwrap(), false);
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result_non_recursive = adapter
        .fetch(
            &source_non_recursive,
            &config,
            FetchStrategy::Full,
            &progress,
        )
        .await
        .unwrap();

    assert_eq!(
        result_non_recursive.items.len(),
        2,
        "Non-recursive should only get root files"
    );

    // Test recursive
    let source_recursive = create_filesystem_source(temp_dir.path().to_str().unwrap(), true);
    let result_recursive = adapter
        .fetch(&source_recursive, &config, FetchStrategy::Full, &progress)
        .await
        .unwrap();

    assert_eq!(
        result_recursive.items.len(),
        5,
        "Recursive should get all files"
    );
}
