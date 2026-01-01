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

/// Get or create the test database connection pool
/// Uses TEST_POSTGRES_* env vars, falling back to defaults for local dev
/// Stores the connection in persistent_term for reuse across tests
fn get_or_create_pool() -> Result(Connection, String) {
  // Check if we already have a connection stored
  let key = "test_db_connection"
  case persistent_term_get(key, None) {
    Some(conn) -> Ok(conn)
    None -> create_new_pool(key)
  }
}

fn create_new_pool(storage_key: String) -> Result(Connection, String) {
  let host = get_env("TEST_POSTGRES_HOST", "localhost")
  let port = get_env_int("TEST_POSTGRES_PORT", 5432)
  let database = get_env("TEST_POSTGRES_DB", "manager_test")
  let user = get_env("TEST_POSTGRES_USER", "manager")
  let password = get_env("TEST_POSTGRES_PASSWORD", "manager")

  // Use a fixed pool name so we can reference it
  let pool_name = process.new_name("test_db_pool")

  let config =
    pog.default_config(pool_name)
    |> pog.host(host)
    |> pog.port(port)
    |> pog.database(database)
    |> pog.user(user)
    |> pog.password(Some(password))
    |> pog.pool_size(20)

  case pog.start(config) {
    Ok(actor.Started(_, conn)) -> {
      // Store the connection for reuse
      let _ = persistent_term_put(storage_key, Some(conn))
      Ok(conn)
    }
    Error(actor.InitFailed(_)) -> {
      // Pool might already exist (race condition), check persistent_term again
      case persistent_term_get(storage_key, None) {
        Some(conn) -> Ok(conn)
        None ->
          Error("Test database connection failed and no cached connection")
      }
    }
    Error(actor.InitTimeout) -> Error("Test database connection timeout")
    Error(actor.InitExited(_)) -> Error("Test database connection exited")
  }
}

/// Initialize the test database schema (drops and recreates tables)
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
    "CREATE TABLE organizations (
      id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
      name TEXT NOT NULL,
      slug TEXT NOT NULL UNIQUE,
      description TEXT,
      is_active BOOLEAN NOT NULL DEFAULT true,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    )"

  use _ <- result.try(
    pog.query(organizations_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create workspaces table
  let workspaces_sql =
    "CREATE TABLE workspaces (
      id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
      organization_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
      name TEXT NOT NULL,
      slug TEXT NOT NULL,
      description TEXT,
      is_active BOOLEAN NOT NULL DEFAULT true,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(organization_id, slug)
    )"

  use _ <- result.try(
    pog.query(workspaces_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create chats table
  let chats_sql =
    "CREATE TABLE chats (
      id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
      workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
      title TEXT NOT NULL,
      model_name TEXT NOT NULL,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      archived BOOLEAN NOT NULL DEFAULT false
    )"

  use _ <- result.try(
    pog.query(chats_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create messages table
  let messages_sql =
    "CREATE TABLE messages (
      id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
      chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
      role TEXT NOT NULL,
      content TEXT NOT NULL,
      created_at TEXT NOT NULL
    )"

  use _ <- result.try(
    pog.query(messages_sql)
    |> pog.execute(db)
    |> result.map_error(connection.query_error_to_string),
  )

  // Create projects table
  let projects_sql =
    "CREATE TABLE projects (
      id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
      workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
      name TEXT NOT NULL,
      description TEXT,
      status TEXT NOT NULL DEFAULT 'active',
      github_repo_url TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
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

/// Run a test with a clean database
/// Waits for the database to be ready before running
pub fn with_db(f: fn(Connection) -> Nil) -> Nil {
  let db = wait_for_db(30)
  // Wait up to 30 retries
  let assert Ok(_) = init_schema(db)
  f(db)
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
