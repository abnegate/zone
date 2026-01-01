import database/connection.{type Connection, query_error_to_string}
import gleam/int
import gleam/list
import gleam/result
import gleam/string
import pog
import simplifile

/// Migration file with version number and content
pub type MigrationFile {
  MigrationFile(version: Int, filename: String, content: String)
}

/// Run all pending migrations
pub fn run_migrations(db: Connection) -> Result(Int, String) {
  // Ensure migrations table exists
  use _ <- result.try(create_migrations_table(db))

  // Get already-run migrations
  use completed <- result.try(get_completed_migrations(db))

  // Read migration files
  use files <- result.try(read_migration_files())

  // Filter to only pending migrations
  let pending =
    files
    |> list.filter(fn(m) { !list.contains(completed, m.version) })

  // Run each pending migration
  use count <- result.try(run_pending_migrations(db, pending, 0))

  Ok(count)
}

/// Create the migrations tracking table if it doesn't exist
fn create_migrations_table(db: Connection) -> Result(Nil, String) {
  let sql =
    "CREATE TABLE IF NOT EXISTS schema_migrations (
      version INTEGER PRIMARY KEY,
      filename TEXT NOT NULL,
      applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )"

  pog.query(sql)
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Get list of already-completed migration versions
fn get_completed_migrations(db: Connection) -> Result(List(Int), String) {
  let sql = "SELECT version FROM schema_migrations ORDER BY version"

  pog.query(sql)
  |> pog.returning(version_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { r.rows })
  |> result.map_error(query_error_to_string)
}

import gleam/dynamic/decode

fn version_decoder() -> decode.Decoder(Int) {
  use version <- decode.field(0, decode.int)
  decode.success(version)
}

/// Run pending migrations in order
fn run_pending_migrations(
  db: Connection,
  migrations: List(MigrationFile),
  count: Int,
) -> Result(Int, String) {
  case migrations {
    [] -> Ok(count)
    [migration, ..rest] -> {
      use _ <- result.try(run_single_migration(db, migration))
      run_pending_migrations(db, rest, count + 1)
    }
  }
}

/// Run a single migration and record it
fn run_single_migration(
  db: Connection,
  migration: MigrationFile,
) -> Result(Nil, String) {
  // Execute the migration SQL
  use _ <- result.try(
    pog.query(migration.content)
    |> pog.execute(db)
    |> result.map_error(fn(e) {
      "Migration "
      <> migration.filename
      <> " failed: "
      <> query_error_to_string(e)
    }),
  )

  // Record the migration as complete
  let record_sql =
    "INSERT INTO schema_migrations (version, filename) VALUES ($1, $2)"

  pog.query(record_sql)
  |> pog.parameter(pog.int(migration.version))
  |> pog.parameter(pog.text(migration.filename))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Read all migration files from the migrations directory
pub fn read_migration_files() -> Result(List(MigrationFile), String) {
  let migrations_dir = "migrations"

  case simplifile.read_directory(migrations_dir) {
    Error(_) -> Error("Migrations directory not found: " <> migrations_dir)
    Ok(files) -> {
      let migrations =
        files
        |> list.filter(fn(f) { string.ends_with(f, ".sql") })
        |> list.filter_map(fn(filename) {
          parse_migration_file(migrations_dir, filename)
        })
        |> list.sort(fn(a, b) { int.compare(a.version, b.version) })

      Ok(migrations)
    }
  }
}

/// Parse a migration filename and read its content
fn parse_migration_file(
  dir: String,
  filename: String,
) -> Result(MigrationFile, Nil) {
  // Filename format: 001_description.sql
  case string.split(filename, "_") {
    [version_str, ..] -> {
      case int.parse(version_str) {
        Ok(version) -> {
          let filepath = dir <> "/" <> filename
          case simplifile.read(filepath) {
            Ok(content) -> Ok(MigrationFile(version, filename, content))
            Error(_) -> Error(Nil)
          }
        }
        Error(_) -> Error(Nil)
      }
    }
    _ -> Error(Nil)
  }
}

/// Get the version number from a migration file
pub fn get_version(migration: MigrationFile) -> Int {
  migration.version
}

/// Get the filename from a migration file
pub fn get_filename(migration: MigrationFile) -> String {
  migration.filename
}

/// Get the content from a migration file
pub fn get_content(migration: MigrationFile) -> String {
  migration.content
}
