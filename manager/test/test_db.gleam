import database/connection.{type Connection}
import envoy
import gleam/erlang/process
import gleam/int
import gleam/option.{None, Some}
import gleam/otp/actor
import gleam/result
import pog

// Persistent storage for the database connection
// Using Erlang's persistent_term for efficient cross-process sharing
@external(erlang, "persistent_term", "get")
fn persistent_term_get(key: a, default: b) -> b

@external(erlang, "persistent_term", "put")
fn persistent_term_put(key: a, value: b) -> Nil

// Global lock for schema initialization
@external(erlang, "global", "set_lock")
fn global_set_lock(lock_id: #(a, b)) -> Bool

@external(erlang, "global", "del_lock")
fn global_del_lock(lock_id: #(a, b)) -> Bool

// Create a fixed pool name (same name every time, unlike process.new_name which adds unique suffixes)
@external(erlang, "test_db_ffi", "fixed_pool_name")
fn fixed_pool_name() -> process.Name(msg)

/// Initialize the test database pool and schema
/// Call this once in main() before running tests
pub fn setup() -> Nil {
  let db = wait_for_db(30)
  let assert Ok(_) = init_schema(db)
  Nil
}

/// Get or create the test database connection pool
/// Uses TEST_POSTGRES_* env vars, falling back to defaults for local dev
/// Stores the connection in persistent_term for reuse across tests
fn get_or_create_pool() -> Result(Connection, String) {
  let storage_key = "test_db_connection"

  // Fast path: connection already stored
  case persistent_term_get(storage_key, None) {
    Some(conn) -> Ok(conn)
    None -> create_new_pool(storage_key)
  }
}

fn create_new_pool(storage_key: String) -> Result(Connection, String) {
  let host = get_env("TEST_POSTGRES_HOST", "localhost")
  let port = get_env_int("TEST_POSTGRES_PORT", 5432)
  let database = get_env("TEST_POSTGRES_DB", "manager_test")
  let user = get_env("TEST_POSTGRES_USER", "manager")
  let password = get_env("TEST_POSTGRES_PASSWORD", "manager")

  // Use a fixed pool name (atom) so only one pool is created
  let pool_name = fixed_pool_name()

  let config =
    pog.default_config(pool_name)
    |> pog.host(host)
    |> pog.port(port)
    |> pog.database(database)
    |> pog.user(user)
    |> pog.password(Some(password))
    |> pog.pool_size(100)

  case pog.start(config) {
    Ok(actor.Started(_, conn)) -> {
      // Store the connection for reuse
      let _ = persistent_term_put(storage_key, Some(conn))
      Ok(conn)
    }
    Error(actor.InitFailed(_)) -> {
      // Pool already exists (race condition), get the named connection
      let conn = pog.named_connection(pool_name)
      let _ = persistent_term_put(storage_key, Some(conn))
      Ok(conn)
    }
    Error(actor.InitTimeout) -> Error("Test database connection timeout")
    Error(actor.InitExited(_)) -> Error("Test database connection exited")
  }
}

/// Initialize the test database schema (drops and recreates tables)
/// Called exactly once at the start of test run
pub fn init_schema(db: Connection) -> Result(Nil, String) {
  // Drop existing tables - execute separately (PostgreSQL prepared statements can't have multiple commands)
  use _ <- result.try(
    pog.query("DROP TABLE IF EXISTS messages CASCADE")
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )
  use _ <- result.try(
    pog.query("DROP TABLE IF EXISTS chats CASCADE")
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )
  use _ <- result.try(
    pog.query("DROP TABLE IF EXISTS projects CASCADE")
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )
  use _ <- result.try(
    pog.query("DROP TABLE IF EXISTS workspaces CASCADE")
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )
  use _ <- result.try(
    pog.query("DROP TABLE IF EXISTS organizations CASCADE")
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create organizations table
  let organizations_sql =
    "CREATE TABLE IF NOT EXISTS organizations (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      name TEXT NOT NULL,
      slug TEXT NOT NULL UNIQUE,
      description TEXT,
      is_active BOOLEAN DEFAULT TRUE,
      created_at TIMESTAMP DEFAULT NOW(),
      updated_at TIMESTAMP DEFAULT NOW()
    )"

  use _ <- result.try(
    pog.query(organizations_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create workspaces table
  let workspaces_sql =
    "CREATE TABLE IF NOT EXISTS workspaces (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
      name TEXT NOT NULL,
      slug TEXT NOT NULL,
      description TEXT,
      is_active BOOLEAN DEFAULT TRUE,
      created_at TIMESTAMP DEFAULT NOW(),
      updated_at TIMESTAMP DEFAULT NOW(),
      UNIQUE(organization_id, slug)
    )"

  use _ <- result.try(
    pog.query(workspaces_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create chats table
  let chats_sql =
    "CREATE TABLE IF NOT EXISTS chats (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
      title TEXT NOT NULL,
      model_name TEXT NOT NULL,
      created_at TIMESTAMP DEFAULT NOW(),
      updated_at TIMESTAMP DEFAULT NOW(),
      archived BOOLEAN DEFAULT FALSE
    )"

  use _ <- result.try(
    pog.query(chats_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create messages table
  let messages_sql =
    "CREATE TABLE IF NOT EXISTS messages (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
      role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
      content TEXT NOT NULL,
      created_at TIMESTAMP DEFAULT NOW(),
      metadata JSONB DEFAULT '{}'::jsonb
    )"

  use _ <- result.try(
    pog.query(messages_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create projects table
  let projects_sql =
    "CREATE TABLE IF NOT EXISTS projects (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
      name TEXT NOT NULL,
      description TEXT,
      status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'on_hold', 'cancelled')),
      github_repo_url TEXT,
      created_at TIMESTAMP DEFAULT NOW(),
      updated_at TIMESTAMP DEFAULT NOW()
    )"

  use _ <- result.try(
    pog.query(projects_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  Ok(Nil)
}

/// Clean all data from tables (keeps schema)
pub fn clean_tables(db: Connection) -> Result(Nil, String) {
  let sql = "TRUNCATE TABLE messages, chats, projects RESTART IDENTITY CASCADE"

  pog.query(sql)
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(connection.query_error_to_string)
}

/// Run a test with a database connection
/// Schema is initialized in setup() which is called from main() before tests run
pub fn with_db(f: fn(Connection) -> Nil) -> Nil {
  let storage_key = "test_db_connection"
  // Connection should already be in persistent_term from setup()
  case persistent_term_get(storage_key, None) {
    Some(conn) -> f(conn)
    None -> {
      // Fallback: create pool if not yet initialized (shouldn't happen normally)
      let db = wait_for_db(30)
      f(db)
    }
  }
}

/// Wait for database to be ready, retrying with backoff
fn wait_for_db(max_attempts: Int) -> Connection {
  wait_for_db_loop(max_attempts, 1)
}

fn wait_for_db_loop(remaining: Int, attempt: Int) -> Connection {
  case remaining {
    0 -> panic as "Database not available after max retries"
    _ -> {
      case get_or_create_pool() {
        Ok(db) -> db
        Error(_) -> {
          // Linear backoff: 500ms between retries
          process.sleep(500)
          wait_for_db_loop(remaining - 1, attempt + 1)
        }
      }
    }
  }
}

// =============================================================================
// Test helpers
// =============================================================================

/// Generate a unique slug for test data to avoid conflicts in parallel tests
pub fn unique_slug(prefix: String) -> String {
  let unique = int.to_string(erlang_unique_integer())
  prefix <> "-" <> unique
}

@external(erlang, "erlang", "unique_integer")
fn erlang_unique_integer() -> Int

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
