//! Redis cache layer
//!
//! Provides caching for frequently accessed data to reduce database load.

use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

/// Cache error type
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type CacheResult<T> = Result<T, CacheError>;

/// Redis cache client
#[derive(Clone)]
#[allow(dead_code)]
pub struct Cache {
    conn: MultiplexedConnection,
    prefix: String,
}

#[allow(dead_code)]
impl Cache {
    /// Connect to Redis
    pub async fn connect(redis_url: &str) -> CacheResult<Self> {
        let client = redis::Client::open(redis_url)?;
        let conn = client.get_multiplexed_async_connection().await?;

        Ok(Self {
            conn,
            prefix: "zone:".to_string(),
        })
    }

    /// Build a prefixed key
    fn key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }

    /// Get a cached value
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> CacheResult<Option<T>> {
        let mut conn = self.conn.clone();
        let data: Option<String> = conn.get(self.key(key)).await?;

        match data {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    /// Set a cached value with TTL
    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) -> CacheResult<()> {
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(value)?;
        let _: () = conn.set_ex(self.key(key), json, ttl.as_secs()).await?;
        Ok(())
    }

    /// Set a cached value without TTL
    pub async fn set_no_ttl<T: Serialize>(&self, key: &str, value: &T) -> CacheResult<()> {
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(value)?;
        let _: () = conn.set(self.key(key), json).await?;
        Ok(())
    }

    /// Delete a cached value
    pub async fn delete(&self, key: &str) -> CacheResult<bool> {
        let mut conn = self.conn.clone();
        let deleted: i32 = conn.del(self.key(key)).await?;
        Ok(deleted > 0)
    }

    /// Delete all keys matching a pattern
    pub async fn delete_pattern(&self, pattern: &str) -> CacheResult<u64> {
        let mut conn = self.conn.clone();
        let full_pattern = self.key(pattern);

        // Get all matching keys
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&full_pattern)
            .query_async(&mut conn)
            .await?;

        if keys.is_empty() {
            return Ok(0);
        }

        // Delete all matching keys
        let deleted: i32 = conn.del(keys).await?;
        Ok(deleted as u64)
    }

    /// Check if a key exists
    pub async fn exists(&self, key: &str) -> CacheResult<bool> {
        let mut conn = self.conn.clone();
        let exists: bool = conn.exists(self.key(key)).await?;
        Ok(exists)
    }

    /// Get remaining TTL for a key (in seconds)
    pub async fn ttl(&self, key: &str) -> CacheResult<Option<i64>> {
        let mut conn = self.conn.clone();
        let ttl: i64 = conn.ttl(self.key(key)).await?;

        if ttl < 0 {
            Ok(None) // Key doesn't exist or has no TTL
        } else {
            Ok(Some(ttl))
        }
    }

    /// Increment a counter
    pub async fn incr(&self, key: &str) -> CacheResult<i64> {
        let mut conn = self.conn.clone();
        let value: i64 = conn.incr(self.key(key), 1).await?;
        Ok(value)
    }

    /// Increment a counter with expiry
    pub async fn incr_with_ttl(&self, key: &str, ttl: Duration) -> CacheResult<i64> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);

        // Use a pipeline for atomic incr + expire
        let (value,): (i64,) = redis::pipe()
            .atomic()
            .incr(&full_key, 1)
            .expire(&full_key, ttl.as_secs() as i64)
            .ignore()
            .query_async(&mut conn)
            .await?;

        Ok(value)
    }

    /// Get or set a cached value (cache-aside pattern)
    pub async fn get_or_set<T, F, Fut>(&self, key: &str, ttl: Duration, fetch: F) -> CacheResult<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = CacheResult<T>>,
    {
        // Try to get from cache first
        if let Some(cached) = self.get::<T>(key).await? {
            return Ok(cached);
        }

        // Fetch the value
        let value = fetch().await?;

        // Cache it
        self.set(key, &value, ttl).await?;

        Ok(value)
    }
}

/// Cache key builders for common entities
pub mod keys {
    use uuid::Uuid;

    pub fn organization(id: Uuid) -> String {
        format!("org:{}", id)
    }

    pub fn organizations_list() -> String {
        "orgs:list".to_string()
    }

    pub fn workspace(id: Uuid) -> String {
        format!("ws:{}", id)
    }

    pub fn workspaces_by_org(org_id: Uuid) -> String {
        format!("ws:org:{}", org_id)
    }

    pub fn project(id: Uuid) -> String {
        format!("project:{}", id)
    }

    pub fn projects_by_workspace(workspace_id: Uuid) -> String {
        format!("projects:ws:{}", workspace_id)
    }

    pub fn task(id: Uuid) -> String {
        format!("task:{}", id)
    }

    pub fn tasks_by_project(project_id: Uuid) -> String {
        format!("tasks:project:{}", project_id)
    }

    pub fn user(id: Uuid) -> String {
        format!("user:{}", id)
    }

    pub fn user_permissions(user_id: Uuid) -> String {
        format!("user:perms:{}", user_id)
    }

    pub fn source(id: Uuid) -> String {
        format!("source:{}", id)
    }

    pub fn sources_list() -> String {
        "sources:list".to_string()
    }

    pub fn chat(id: Uuid) -> String {
        format!("chat:{}", id)
    }

    pub fn rate_limit(user_id: Uuid, action: &str) -> String {
        format!("rate:{}:{}", user_id, action)
    }
}

/// Default cache TTLs
pub mod ttl {
    use std::time::Duration;

    /// Short TTL for frequently changing data (1 minute)
    pub const SHORT: Duration = Duration::from_secs(60);

    /// Medium TTL for moderately stable data (5 minutes)
    pub const MEDIUM: Duration = Duration::from_secs(300);

    /// Long TTL for stable data (1 hour)
    pub const LONG: Duration = Duration::from_secs(3600);

    /// Very long TTL for rarely changing data (1 day)
    pub const VERY_LONG: Duration = Duration::from_secs(86400);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn test_cache_error_display_redis() {
        // We can't easily construct a redis::RedisError, so we test the enum variant exists
        // and the serialization error case
        let json_err: serde_json::Error = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err: CacheError = json_err.into();
        assert!(err.to_string().contains("Serialization error"));
    }

    #[test]
    fn test_cache_result_type() {
        fn get_result() -> CacheResult<i32> {
            Ok(42)
        }
        let ok = get_result();
        assert_eq!(ok.unwrap(), 42);
    }

    // Key builder tests
    #[test]
    fn test_key_organization() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::organization(id),
            "org:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_organizations_list() {
        assert_eq!(keys::organizations_list(), "orgs:list");
    }

    #[test]
    fn test_key_workspace() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::workspace(id),
            "ws:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_workspaces_by_org() {
        let org_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::workspaces_by_org(org_id),
            "ws:org:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_project() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::project(id),
            "project:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_projects_by_workspace() {
        let workspace_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::projects_by_workspace(workspace_id),
            "projects:ws:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_task() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(keys::task(id), "task:550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_key_tasks_by_project() {
        let project_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::tasks_by_project(project_id),
            "tasks:project:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_user() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(keys::user(id), "user:550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_key_user_permissions() {
        let user_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::user_permissions(user_id),
            "user:perms:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_source() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::source(id),
            "source:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_key_sources_list() {
        assert_eq!(keys::sources_list(), "sources:list");
    }

    #[test]
    fn test_key_chat() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(keys::chat(id), "chat:550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_key_rate_limit() {
        let user_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            keys::rate_limit(user_id, "login"),
            "rate:550e8400-e29b-41d4-a716-446655440000:login"
        );
    }

    #[test]
    fn test_key_rate_limit_various_actions() {
        let user_id = Uuid::new_v4();
        let key1 = keys::rate_limit(user_id, "api_call");
        let key2 = keys::rate_limit(user_id, "file_upload");

        assert!(key1.contains("api_call"));
        assert!(key2.contains("file_upload"));
        assert_ne!(key1, key2);
    }

    // TTL tests
    #[test]
    fn test_ttl_short() {
        assert_eq!(ttl::SHORT, Duration::from_secs(60));
        assert_eq!(ttl::SHORT.as_secs(), 60);
    }

    #[test]
    fn test_ttl_medium() {
        assert_eq!(ttl::MEDIUM, Duration::from_secs(300));
        assert_eq!(ttl::MEDIUM.as_secs(), 300); // 5 minutes
    }

    #[test]
    fn test_ttl_long() {
        assert_eq!(ttl::LONG, Duration::from_secs(3600));
        assert_eq!(ttl::LONG.as_secs(), 3600); // 1 hour
    }

    #[test]
    fn test_ttl_very_long() {
        assert_eq!(ttl::VERY_LONG, Duration::from_secs(86400));
        assert_eq!(ttl::VERY_LONG.as_secs(), 86400); // 1 day
    }

    #[test]
    fn test_ttl_ordering() {
        assert!(ttl::SHORT < ttl::MEDIUM);
        assert!(ttl::MEDIUM < ttl::LONG);
        assert!(ttl::LONG < ttl::VERY_LONG);
    }

    // Key uniqueness tests
    #[test]
    fn test_keys_are_unique_for_different_entities() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        let org_key = keys::organization(id);
        let ws_key = keys::workspace(id);
        let project_key = keys::project(id);
        let task_key = keys::task(id);
        let user_key = keys::user(id);
        let source_key = keys::source(id);
        let chat_key = keys::chat(id);

        // All keys should be different even with the same UUID
        let keys_arr = [
            &org_key,
            &ws_key,
            &project_key,
            &task_key,
            &user_key,
            &source_key,
            &chat_key,
        ];
        for i in 0..keys_arr.len() {
            for j in (i + 1)..keys_arr.len() {
                assert_ne!(
                    keys_arr[i], keys_arr[j],
                    "Keys should be unique: {} vs {}",
                    keys_arr[i], keys_arr[j]
                );
            }
        }
    }

    #[test]
    fn test_different_uuids_produce_different_keys() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        assert_ne!(keys::organization(id1), keys::organization(id2));
        assert_ne!(keys::workspace(id1), keys::workspace(id2));
        assert_ne!(keys::project(id1), keys::project(id2));
    }
}
