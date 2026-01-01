/// Cache connection module - Valkey/Redis connection management
///
/// Provides connection pooling and cache operations for the application.
import config
import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}
import mug
import valkyrie.{type Config, type Connection, type Error, Config}

/// Cache connection type (valkyrie connection)
pub type CacheConnection =
  Connection

/// Initialize the cache connection
/// Should be called once at application startup
pub fn connect() -> Result(CacheConnection, String) {
  let host = config.get_valkey_host()
  let port = config.get_valkey_port()

  let cfg =
    Config(
      host: host,
      port: port,
      auth: valkyrie.NoAuth,
      ip_version_preference: mug.Ipv4Only,
    )

  case valkyrie.create_connection(cfg, 5000) {
    Ok(conn) -> Ok(conn)
    Error(err) -> Error(cache_error_to_string(err))
  }
}

/// Convert cache error to string
pub fn cache_error_to_string(err: Error) -> String {
  case err {
    valkyrie.NotFound -> "Key not found"
    valkyrie.Conflict -> "Operation conflict"
    valkyrie.Timeout -> "Cache operation timeout"
    valkyrie.ServerError(msg) -> "Cache server error: " <> msg
    valkyrie.RespError(msg) -> "Cache RESP protocol error: " <> msg
    valkyrie.ConnectError(_) -> "Cache connection error"
    valkyrie.TcpError(_) -> "Cache TCP error"
    valkyrie.PoolError(_) -> "Cache pool error"
  }
}

// =============================================================================
// Cache Operations
// =============================================================================

/// Get a value from cache
pub fn get(conn: CacheConnection, key: String) -> Result(Option(String), String) {
  case valkyrie.get(conn, key, 5000) {
    Ok(value) -> Ok(Some(value))
    Error(valkyrie.NotFound) -> Ok(None)
    Error(err) -> Error(cache_error_to_string(err))
  }
}

/// Set a value in cache with TTL (in seconds)
pub fn set(
  conn: CacheConnection,
  key: String,
  value: String,
  ttl_seconds: Int,
) -> Result(Nil, String) {
  // Use SetOptions with expiry in seconds
  let options =
    Some(valkyrie.SetOptions(
      existence_condition: None,
      return_old: False,
      expiry_option: Some(valkyrie.ExpirySeconds(ttl_seconds)),
    ))
  case valkyrie.set(conn, key, value, options, 5000) {
    Ok(_) -> Ok(Nil)
    Error(err) -> Error(cache_error_to_string(err))
  }
}

/// Delete a key from cache
pub fn delete(conn: CacheConnection, key: String) -> Result(Nil, String) {
  case valkyrie.del(conn, [key], 5000) {
    Ok(_) -> Ok(Nil)
    Error(err) -> Error(cache_error_to_string(err))
  }
}

/// Delete keys matching a pattern (for cache invalidation)
pub fn delete_pattern(
  conn: CacheConnection,
  pattern: String,
) -> Result(Nil, String) {
  case valkyrie.keys(conn, pattern, 5000) {
    Ok(keys) -> {
      case keys {
        [] -> Ok(Nil)
        _ -> {
          case valkyrie.del(conn, keys, 5000) {
            Ok(_) -> Ok(Nil)
            Error(err) -> Error(cache_error_to_string(err))
          }
        }
      }
    }
    Error(err) -> Error(cache_error_to_string(err))
  }
}

// =============================================================================
// Cache-Aside Pattern Helpers
// =============================================================================

/// Get or compute a value (cache-aside pattern)
/// If key exists in cache, return it; otherwise compute and cache the result
pub fn get_or_compute(
  conn: CacheConnection,
  key: String,
  ttl_seconds: Int,
  compute: fn() -> Result(String, String),
) -> Result(String, String) {
  case get(conn, key) {
    Ok(Some(cached)) -> Ok(cached)
    Ok(None) -> {
      case compute() {
        Ok(value) -> {
          // Cache the computed value (ignore cache errors)
          let _ = set(conn, key, value, ttl_seconds)
          Ok(value)
        }
        Error(err) -> Error(err)
      }
    }
    Error(_) -> {
      // Cache error, fallback to compute
      compute()
    }
  }
}

/// Get or compute with JSON serialization
pub fn get_or_compute_json(
  conn: CacheConnection,
  key: String,
  ttl_seconds: Int,
  compute: fn() -> Result(json.Json, String),
) -> Result(json.Json, String) {
  case get(conn, key) {
    Ok(Some(cached)) -> {
      case json.parse(cached, decode.dynamic) {
        Ok(_) -> {
          // Return the raw JSON string wrapped
          // For actual use, the caller handles decoding
          Ok(json.string(cached))
        }
        Error(_) -> compute()
      }
    }
    Ok(None) -> {
      case compute() {
        Ok(value) -> {
          let json_str = json.to_string(value)
          let _ = set(conn, key, json_str, ttl_seconds)
          Ok(value)
        }
        Error(err) -> Error(err)
      }
    }
    Error(_) -> compute()
  }
}

// =============================================================================
// Key Generation Helpers
// =============================================================================

/// Generate a cache key for entity by ID
pub fn entity_key(entity_type: String, id: String) -> String {
  entity_type <> ":" <> id
}

/// Generate a cache key for entity list
pub fn list_key(entity_type: String) -> String {
  entity_type <> ":list"
}

/// Generate a cache key for filtered entity list
pub fn filtered_list_key(entity_type: String, filter: String) -> String {
  entity_type <> ":list:" <> filter
}

/// Invalidation pattern for all keys of an entity type
pub fn invalidation_pattern(entity_type: String) -> String {
  entity_type <> ":*"
}
