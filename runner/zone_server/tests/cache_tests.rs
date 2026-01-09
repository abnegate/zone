//! Cache integration tests
//!
//! These tests require a running Valkey/Redis instance on localhost:6379

use serde::{Deserialize, Serialize};
use std::time::Duration;

use zone_server::cache::Cache;

/// Test data structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestData {
    id: i32,
    name: String,
    tags: Vec<String>,
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

#[tokio::test]
async fn test_cache_connect() {
    let cache = Cache::connect(&redis_url()).await;
    assert!(cache.is_ok(), "Should connect to Valkey");
}

#[tokio::test]
async fn test_cache_set_and_get() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    let data = TestData {
        id: 1,
        name: "Test".to_string(),
        tags: vec!["tag1".to_string(), "tag2".to_string()],
    };

    // Set with TTL
    cache
        .set("test:set_get", &data, Duration::from_secs(60))
        .await
        .expect("set");

    // Get it back
    let retrieved: Option<TestData> = cache.get("test:set_get").await.expect("get");
    assert_eq!(retrieved, Some(data));

    // Cleanup
    cache.delete("test:set_get").await.ok();
}

#[tokio::test]
async fn test_cache_get_nonexistent() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    let result: Option<TestData> = cache
        .get("nonexistent:key:that:does:not:exist")
        .await
        .expect("get");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cache_set_no_ttl() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    let data = TestData {
        id: 2,
        name: "NoTTL".to_string(),
        tags: vec![],
    };

    cache
        .set_no_ttl("test:no_ttl", &data)
        .await
        .expect("set_no_ttl");

    let retrieved: Option<TestData> = cache.get("test:no_ttl").await.expect("get");
    assert_eq!(retrieved, Some(data));

    // Cleanup
    cache.delete("test:no_ttl").await.ok();
}

#[tokio::test]
async fn test_cache_delete() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Set a value
    cache
        .set("test:delete", &42i32, Duration::from_secs(60))
        .await
        .expect("set");

    // Verify it exists
    let exists = cache.exists("test:delete").await.expect("exists");
    assert!(exists);

    // Delete it
    let deleted = cache.delete("test:delete").await.expect("delete");
    assert!(deleted);

    // Verify it's gone
    let exists = cache.exists("test:delete").await.expect("exists");
    assert!(!exists);

    // Delete non-existent key returns false
    let deleted = cache.delete("test:delete").await.expect("delete");
    assert!(!deleted);
}

#[tokio::test]
async fn test_cache_exists() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Non-existent key
    let exists = cache
        .exists("test:exists:nonexistent")
        .await
        .expect("exists");
    assert!(!exists);

    // Create key
    cache
        .set("test:exists:key", &"value", Duration::from_secs(60))
        .await
        .expect("set");

    // Now it exists
    let exists = cache.exists("test:exists:key").await.expect("exists");
    assert!(exists);

    // Cleanup
    cache.delete("test:exists:key").await.ok();
}

#[tokio::test]
async fn test_cache_incr() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Clean up first
    cache.delete("test:incr").await.ok();

    // First increment creates key with value 1
    let val = cache.incr("test:incr").await.expect("incr");
    assert_eq!(val, 1);

    // Second increment
    let val = cache.incr("test:incr").await.expect("incr");
    assert_eq!(val, 2);

    // Third increment
    let val = cache.incr("test:incr").await.expect("incr");
    assert_eq!(val, 3);

    // Cleanup
    cache.delete("test:incr").await.ok();
}

#[tokio::test]
async fn test_cache_incr_with_ttl() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Clean up first
    cache.delete("test:incr_ttl").await.ok();

    let val = cache
        .incr_with_ttl("test:incr_ttl", Duration::from_secs(60))
        .await
        .expect("incr_with_ttl");
    assert_eq!(val, 1);

    // Check TTL was set
    let ttl = cache.ttl("test:incr_ttl").await.expect("ttl");
    assert!(ttl.is_some());
    assert!(ttl.unwrap() > 0);

    // Cleanup
    cache.delete("test:incr_ttl").await.ok();
}

#[tokio::test]
async fn test_cache_ttl() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Non-existent key has no TTL
    let ttl = cache.ttl("test:ttl:nonexistent").await.expect("ttl");
    assert!(ttl.is_none());

    // Key without TTL
    cache.set_no_ttl("test:ttl:no_expire", &1i32).await.ok();
    let ttl = cache.ttl("test:ttl:no_expire").await.expect("ttl");
    assert!(ttl.is_none()); // -1 means no TTL, should return None

    // Key with TTL
    cache
        .set("test:ttl:with_expire", &1i32, Duration::from_secs(120))
        .await
        .expect("set");
    let ttl = cache.ttl("test:ttl:with_expire").await.expect("ttl");
    assert!(ttl.is_some());
    assert!(ttl.unwrap() > 100 && ttl.unwrap() <= 120);

    // Cleanup
    cache.delete("test:ttl:no_expire").await.ok();
    cache.delete("test:ttl:with_expire").await.ok();
}

#[tokio::test]
async fn test_cache_delete_pattern() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Create multiple keys with a pattern
    cache
        .set("test:pattern:a", &1i32, Duration::from_secs(60))
        .await
        .ok();
    cache
        .set("test:pattern:b", &2i32, Duration::from_secs(60))
        .await
        .ok();
    cache
        .set("test:pattern:c", &3i32, Duration::from_secs(60))
        .await
        .ok();
    cache
        .set("test:other:x", &4i32, Duration::from_secs(60))
        .await
        .ok();

    // Delete only pattern keys
    let deleted = cache
        .delete_pattern("test:pattern:*")
        .await
        .expect("delete_pattern");
    assert!(deleted >= 3, "Should delete at least 3 keys");

    // Verify pattern keys are gone
    assert!(!cache.exists("test:pattern:a").await.unwrap());
    assert!(!cache.exists("test:pattern:b").await.unwrap());
    assert!(!cache.exists("test:pattern:c").await.unwrap());

    // Other key should still exist
    assert!(cache.exists("test:other:x").await.unwrap());

    // Cleanup
    cache.delete("test:other:x").await.ok();
}

#[tokio::test]
async fn test_cache_get_or_set_cached() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    // Clean up first
    cache.delete("test:get_or_set").await.ok();

    // First call should fetch and cache
    let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let fetch_count_clone = fetch_count.clone();

    let result: i32 = cache
        .get_or_set("test:get_or_set", Duration::from_secs(60), || {
            let fc = fetch_count_clone.clone();
            async move {
                fc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(42)
            }
        })
        .await
        .expect("get_or_set");

    assert_eq!(result, 42);
    assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);

    // Second call should use cached value
    let fetch_count_clone = fetch_count.clone();
    let result: i32 = cache
        .get_or_set("test:get_or_set", Duration::from_secs(60), || {
            let fc = fetch_count_clone.clone();
            async move {
                fc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(99) // Different value that shouldn't be used
            }
        })
        .await
        .expect("get_or_set");

    assert_eq!(result, 42); // Should be cached value
    assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1); // Fetch not called again

    // Cleanup
    cache.delete("test:get_or_set").await.ok();
}

#[tokio::test]
async fn test_cache_string_values() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    cache
        .set("test:string", &"hello world", Duration::from_secs(60))
        .await
        .expect("set");

    let val: Option<String> = cache.get("test:string").await.expect("get");
    assert_eq!(val, Some("hello world".to_string()));

    cache.delete("test:string").await.ok();
}

#[tokio::test]
async fn test_cache_complex_json() {
    let cache = Cache::connect(&redis_url()).await.expect("connect");

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct ComplexData {
        nested: NestedData,
        optional: Option<String>,
        list: Vec<i32>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct NestedData {
        value: f64,
    }

    let data = ComplexData {
        nested: NestedData { value: 42.5 },
        optional: Some("present".to_string()),
        list: vec![1, 2, 3],
    };

    cache
        .set("test:complex", &data, Duration::from_secs(60))
        .await
        .expect("set");

    let retrieved: Option<ComplexData> = cache.get("test:complex").await.expect("get");
    assert_eq!(retrieved, Some(data));

    cache.delete("test:complex").await.ok();
}
