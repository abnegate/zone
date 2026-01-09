//! Integration tests for GitHub adapter using wiremock

use base64::Engine;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_context::adapters::{GitHubAdapter, NoOpProgress, SourceAdapter};
use zone_context::content::{FetchConfig, FetchStrategy};
use zone_core::{Source, SourceCategory, SourceType};

fn create_test_source(
    owner: &str,
    repo: &str,
    token: Option<&str>,
    branch: Option<&str>,
    path_filter: Option<&str>,
) -> Source {
    let mut config = json!({
        "owner": owner,
        "repo": repo,
    });

    if let Some(t) = token {
        config["token"] = json!(t);
    }

    if let Some(b) = branch {
        config["branch"] = json!(b);
    }

    if let Some(p) = path_filter {
        config["path"] = json!(p);
    }

    Source {
        id: Uuid::new_v4(),
        name: format!("{}/{}", owner, repo),
        source_type: SourceType::GitHub,
        category: SourceCategory::File,
        config,
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_github_fetch_mocked_repo() {
    let mock_server = MockServer::start().await;

    // Mock repository info endpoint
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    // Mock tree endpoint
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "README.md",
                    "type": "blob",
                    "size": 100,
                    "sha": "def456"
                },
                {
                    "path": "src/lib.rs",
                    "type": "blob",
                    "size": 500,
                    "sha": "ghi789"
                },
                {
                    "path": "image.png",
                    "type": "blob",
                    "size": 1000,
                    "sha": "jkl012"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    // Mock contents endpoints
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/README%2Emd"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "README.md",
            "path": "README.md",
            "sha": "def456",
            "size": 100,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("# Test Repo\n\nThis is a test repository."),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/src%2Flib%2Ers"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "lib.rs",
            "path": "src/lib.rs",
            "sha": "ghi789",
            "size": 500,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("pub fn hello() -> &'static str {\n    \"Hello, world!\"\n}"),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await;

    // Should now succeed with the mock server
    assert!(result.is_ok());
    let fetch_result = result.unwrap();

    // Should have 2 files (README.md and lib.rs, not the PNG)
    assert_eq!(fetch_result.items.len(), 2);

    // Verify the files
    let readme = fetch_result.items.iter().find(|i| i.title == "README.md");
    assert!(readme.is_some());
    assert!(readme.unwrap().content.is_some());

    let lib_rs = fetch_result.items.iter().find(|i| i.title == "lib.rs");
    assert!(lib_rs.is_some());
    assert!(lib_rs.unwrap().content.is_some());
}

#[tokio::test]
async fn test_github_fetch_with_branch() {
    let mock_server = MockServer::start().await;

    // Mock repository info endpoint
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    // Mock tree endpoint for develop branch
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/develop"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "xyz789",
            "tree": [
                {
                    "path": "dev.md",
                    "type": "blob",
                    "size": 50,
                    "sha": "dev123"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/dev%2Emd"))
        .and(query_param("ref", "develop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "dev.md",
            "path": "dev.md",
            "sha": "dev123",
            "size": 50,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("# Dev Branch"),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, Some("develop"), None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await;

    assert!(result.is_ok());
    let fetch_result = result.unwrap();
    assert_eq!(fetch_result.items.len(), 1);
    assert_eq!(fetch_result.items[0].title, "dev.md");
}

#[tokio::test]
async fn test_github_fetch_with_path() {
    let mock_server = MockServer::start().await;

    // Mock repository info endpoint
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    // Mock tree endpoint
    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "README.md",
                    "type": "blob",
                    "size": 100,
                    "sha": "def456"
                },
                {
                    "path": "src/lib.rs",
                    "type": "blob",
                    "size": 500,
                    "sha": "ghi789"
                },
                {
                    "path": "docs/guide.md",
                    "type": "blob",
                    "size": 200,
                    "sha": "doc123"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, Some("src"));
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
        .await;

    assert!(result.is_ok());
    let fetch_result = result.unwrap();
    // Only src/lib.rs should be included
    assert_eq!(fetch_result.items.len(), 1);
    assert_eq!(fetch_result.items[0].title, "lib.rs");
}

#[tokio::test]
async fn test_github_registry_integration() {
    use zone_context::adapters::AdapterRegistry;

    let mut registry = AdapterRegistry::new();
    registry.register(GitHubAdapter::new());

    let adapter = registry.get("github");
    assert!(adapter.is_some());

    let adapter = adapter.unwrap();
    assert_eq!(adapter.source_type(), "github");
}

#[tokio::test]
async fn test_github_verify_repository_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/nonexistent-user-999/nonexistent-repo-999"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let source = create_test_source(
        "nonexistent-user-999",
        "nonexistent-repo-999",
        None,
        None,
        None,
    );
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());

    let result = adapter.verify(&source).await;

    // Should fail with 404
    assert!(result.is_err());
}

#[tokio::test]
async fn test_github_estimate_tokens_structure() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "README.md",
                    "type": "blob",
                    "size": 1000,
                    "sha": "def456"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());

    let result = adapter.estimate_tokens(&source).await;

    assert!(result.is_ok());
    assert!(result.unwrap() > 0);
}

#[tokio::test]
async fn test_github_sync_state() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());

    let result = adapter.get_sync_state(&source).await;

    assert!(result.is_ok());
    let sync_state = result.unwrap();
    assert_eq!(sync_state.version, Some("abc123".to_string()));
}

#[tokio::test]
async fn test_github_metadata_only_strategy() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "file.rs",
                    "type": "blob",
                    "size": 100,
                    "sha": "def456"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
        .await;

    assert!(result.is_ok());
    let fetch_result = result.unwrap();
    assert_eq!(fetch_result.items.len(), 1);
    // Metadata only should not have content
    assert!(fetch_result.items[0].content.is_none());
}

#[tokio::test]
async fn test_github_partial_strategy() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "small.rs",
                    "type": "blob",
                    "size": 50,
                    "sha": "def456"
                },
                {
                    "path": "large.rs",
                    "type": "blob",
                    "size": 50000,
                    "sha": "ghi789"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/small%2Ers"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "small.rs",
            "path": "small.rs",
            "sha": "def456",
            "size": 50,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("fn main() {}"),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(
            &source,
            &config,
            FetchStrategy::Partial { max_tokens: 100 },
            &progress,
        )
        .await;

    assert!(result.is_ok());
    let fetch_result = result.unwrap();
    // Should only get the small file within budget
    assert_eq!(fetch_result.items.len(), 1);
    assert_eq!(fetch_result.items[0].title, "small.rs");
}

#[tokio::test]
async fn test_github_progressive_strategy() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "README.md",
                    "type": "blob",
                    "size": 100,
                    "sha": "def456"
                },
                {
                    "path": "src/lib.rs",
                    "type": "blob",
                    "size": 200,
                    "sha": "ghi789"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/README%2Emd"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "README.md",
            "path": "README.md",
            "sha": "def456",
            "size": 100,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("# README"),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/src%2Flib%2Ers"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "lib.rs",
            "path": "src/lib.rs",
            "sha": "ghi789",
            "size": 200,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("fn lib() {}"),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(
            &source,
            &config,
            FetchStrategy::Progressive {
                priority_order: vec!["README.md".to_string(), "*.rs".to_string()],
            },
            &progress,
        )
        .await;

    assert!(result.is_ok());
    let fetch_result = result.unwrap();
    assert_eq!(fetch_result.items.len(), 2);
}

// Security validation tests

#[tokio::test]
async fn test_github_adapter_validates_owner() {
    let source = create_test_source("owner/with/slash", "repo", None, None, None);
    let adapter = GitHubAdapter::new();

    let result = adapter.verify(&source).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("invalid characters")
    );
}

#[tokio::test]
async fn test_github_adapter_validates_repo() {
    let source = create_test_source("owner", "repo\\with\\backslash", None, None, None);
    let adapter = GitHubAdapter::new();

    let result = adapter.verify(&source).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("invalid characters")
    );
}

#[tokio::test]
async fn test_github_adapter_validates_branch() {
    let source = create_test_source("owner", "repo", None, Some("branch\nwith\nnewline"), None);
    let adapter = GitHubAdapter::new();

    let result = adapter.verify(&source).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("invalid characters")
    );
}

#[tokio::test]
async fn test_github_adapter_max_file_size() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "testrepo",
            "default_branch": "main"
        })))
        .mount(&mock_server)
        .await;

    // File larger than 10MB
    let large_file_size = 11 * 1024 * 1024;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/git/trees/main"))
        .and(query_param("recursive", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [
                {
                    "path": "small.rs",
                    "type": "blob",
                    "size": 100,
                    "sha": "def456"
                },
                {
                    "path": "large.bin",
                    "type": "blob",
                    "size": large_file_size,
                    "sha": "ghi789"
                }
            ],
            "truncated": false
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testowner/testrepo/contents/small%2Ers"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "small.rs",
            "path": "small.rs",
            "sha": "def456",
            "size": 100,
            "type": "file",
            "content": base64::engine::general_purpose::STANDARD.encode("fn main() {}"),
            "encoding": "base64"
        })))
        .mount(&mock_server)
        .await;

    let source = create_test_source("testowner", "testrepo", None, None, None);
    let adapter = GitHubAdapter::with_base_url(mock_server.uri());
    let config = FetchConfig::default();
    let progress = NoOpProgress;

    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await;

    assert!(result.is_ok());
    let fetch_result = result.unwrap();
    // Should only get small.rs, large file should be skipped
    assert_eq!(fetch_result.items.len(), 1);
    assert_eq!(fetch_result.items[0].title, "small.rs");
}
