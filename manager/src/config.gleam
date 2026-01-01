import envoy
import gleam/option.{type Option, None, Some}
import gleam/string

/// Get environment variable with default value
pub fn get_env(key: String, default: String) -> String {
  case envoy.get(key) {
    Ok(value) -> value
    Error(_) -> default
  }
}

/// Get Ollama host URL
pub fn get_ollama_host() -> String {
  get_env("OLLAMA_HOST", "http://ollama:11434")
}

/// Get LiteLLM host URL
pub fn get_litellm_host() -> String {
  get_env("LITELLM_HOST", "http://litellm:4000")
}

/// Get LiteLLM master key
pub fn get_litellm_key() -> String {
  get_env("SECURITY_LITELLM_MASTER_KEY", "")
}

/// Get Manager API key
pub fn get_manager_api_key() -> String {
  get_env("SECURITY_MANAGER_API_KEY", "")
}

/// Get PostgreSQL host
pub fn get_postgres_host() -> String {
  get_env("POSTGRES_HOST", "postgres")
}

/// Get PostgreSQL port
pub fn get_postgres_port() -> Int {
  case get_env("POSTGRES_PORT", "5432") {
    port_str -> {
      case int.parse(port_str) {
        Ok(port) -> port
        Error(_) -> 5432
      }
    }
  }
}

import gleam/int

/// Get PostgreSQL database name
pub fn get_postgres_database() -> String {
  get_env("POSTGRES_DB", "manager")
}

/// Get PostgreSQL user
pub fn get_postgres_user() -> String {
  get_env("POSTGRES_USER", "manager")
}

/// Get PostgreSQL password
pub fn get_postgres_password() -> String {
  get_env("POSTGRES_PASSWORD", "")
}

/// Get GitHub token for API access (used by agentic tasks)
pub fn get_github_token() -> Option(String) {
  case get_env("GITHUB_TOKEN", "") {
    "" -> None
    token -> Some(token)
  }
}

/// Get Valkey host
pub fn get_valkey_host() -> String {
  get_env("VALKEY_HOST", "valkey")
}

/// Get Valkey port
pub fn get_valkey_port() -> Int {
  case get_env("VALKEY_PORT", "6379") {
    port_str -> {
      case int.parse(port_str) {
        Ok(port) -> port
        Error(_) -> 6379
      }
    }
  }
}

/// Default cache TTL in seconds (5 minutes)
pub fn get_cache_ttl() -> Int {
  case get_env("CACHE_TTL", "300") {
    ttl_str -> {
      case int.parse(ttl_str) {
        Ok(ttl) -> ttl
        Error(_) -> 300
      }
    }
  }
}

/// Get JWT secret key (required for production)
/// Panics if JWT_SECRET is not set - this is intentional for security
pub fn get_jwt_secret() -> String {
  case envoy.get("JWT_SECRET") {
    Ok(secret) -> {
      case string.length(secret) >= 32 {
        True -> secret
        False ->
          panic as "JWT_SECRET must be at least 32 characters for security"
      }
    }
    Error(_) ->
      panic as "JWT_SECRET environment variable must be set in production"
  }
}

/// Get JWT access token lifetime in seconds (default: 15 minutes)
pub fn get_jwt_access_lifetime() -> Int {
  case get_env("JWT_ACCESS_LIFETIME", "900") {
    lifetime_str -> {
      case int.parse(lifetime_str) {
        Ok(lifetime) -> lifetime
        Error(_) -> 900
      }
    }
  }
}

/// Get JWT refresh token lifetime in seconds (default: 7 days)
pub fn get_jwt_refresh_lifetime() -> Int {
  case get_env("JWT_REFRESH_LIFETIME", "604800") {
    lifetime_str -> {
      case int.parse(lifetime_str) {
        Ok(lifetime) -> lifetime
        Error(_) -> 604_800
      }
    }
  }
}
