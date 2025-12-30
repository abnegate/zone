import envoy

/// Database configuration
pub type DbConfig {
  DbConfig(host: String, database: String, user: String, password: String)
}

/// Placeholder connection type - will be replaced with actual DB connection
pub type Connection {
  Connection(config: DbConfig)
}

/// Get database configuration from environment variables
pub fn get_config() -> DbConfig {
  let host = get_env("POSTGRES_HOST", "localhost")
  let database = get_env("POSTGRES_DB", "voiz")
  let user = get_env("POSTGRES_USER", "postgres")
  let password = get_env("POSTGRES_PASSWORD", "postgres")

  DbConfig(host: host, database: database, user: user, password: password)
}

/// Create a connection (placeholder - actual implementation will use epgsql)
pub fn connect() -> Result(Connection, String) {
  let config = get_config()
  Ok(Connection(config: config))
}

/// Get environment variable with default
fn get_env(key: String, default: String) -> String {
  case envoy.get(key) {
    Ok(value) -> value
    Error(_) -> default
  }
}
