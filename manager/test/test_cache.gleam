/// Test utilities for cache (Valkey) testing
/// Provides helper functions to connect to a test cache instance
import cache/connection as cache_conn
import envoy
import gleam/erlang/process
import gleam/int
import gleam/option.{None, Some}
import mug
import valkyrie.{Config}

// Persistent storage for the cache connection
@external(erlang, "persistent_term", "get")
fn persistent_term_get(key: a, default: b) -> b

@external(erlang, "persistent_term", "put")
fn persistent_term_put(key: a, value: b) -> Nil

/// Get or create the test cache connection
/// Uses TEST_VALKEY_* env vars, falling back to defaults for local dev
pub fn get_or_create_connection() -> Result(cache_conn.CacheConnection, String) {
  let key = "test_cache_connection"
  case persistent_term_get(key, None) {
    Some(conn) -> Ok(conn)
    None -> create_new_connection(key)
  }
}

fn create_new_connection(
  storage_key: String,
) -> Result(cache_conn.CacheConnection, String) {
  let host = get_env("TEST_VALKEY_HOST", "localhost")
  let port = get_env_int("TEST_VALKEY_PORT", 6379)

  let cfg =
    Config(
      host: host,
      port: port,
      auth: valkyrie.NoAuth,
      ip_version_preference: mug.Ipv4Only,
    )

  case valkyrie.create_connection(cfg, 5000) {
    Ok(conn) -> {
      let _ = persistent_term_put(storage_key, Some(conn))
      Ok(conn)
    }
    Error(err) -> Error(cache_conn.cache_error_to_string(err))
  }
}

/// Flush all keys from the test cache database
pub fn flush_cache(cache: cache_conn.CacheConnection) -> Result(Nil, String) {
  // Delete all test keys using pattern
  cache_conn.delete_pattern(cache, "*")
}

/// Run a test with a clean cache
/// Waits for the cache to be ready before running
pub fn with_cache(f: fn(cache_conn.CacheConnection) -> Nil) -> Nil {
  let cache = wait_for_cache(30)
  let assert Ok(_) = flush_cache(cache)
  f(cache)
}

/// Run a test with both database and cache
pub fn with_db_and_cache(
  f: fn(connection.Connection, cache_conn.CacheConnection) -> Nil,
) -> Nil {
  test_db.with_db(fn(db) {
    let cache = wait_for_cache(30)
    let assert Ok(_) = flush_cache(cache)
    f(db, cache)
  })
}

/// Wait for cache to be ready, retrying with backoff
fn wait_for_cache(max_attempts: Int) -> cache_conn.CacheConnection {
  wait_for_cache_loop(max_attempts, 1)
}

fn wait_for_cache_loop(
  remaining: Int,
  attempt: Int,
) -> cache_conn.CacheConnection {
  case remaining {
    0 -> panic as "Cache not available after max retries"
    _ -> {
      case get_or_create_connection() {
        Ok(cache) -> cache
        Error(_) -> {
          process.sleep(500)
          wait_for_cache_loop(remaining - 1, attempt + 1)
        }
      }
    }
  }
}

// =============================================================================
// Env helpers
// =============================================================================

fn get_env(name: String, default: String) -> String {
  case envoy.get(name) {
    Ok(value) -> value
    Error(_) -> default
  }
}

fn get_env_int(name: String, default: Int) -> Int {
  case envoy.get(name) {
    Ok(value) -> {
      case int.parse(value) {
        Ok(i) -> i
        Error(_) -> default
      }
    }
    Error(_) -> default
  }
}

import database/connection
import test_db
