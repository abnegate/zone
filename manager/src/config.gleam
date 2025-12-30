import envoy

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
