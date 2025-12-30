import database/connection.{type Connection}
import gleam/int
import gleam/list
import gleam/string
import simplifile

/// Migration file with version number and content
pub type MigrationFile {
  MigrationFile(version: Int, filename: String, content: String)
}

/// Run all pending migrations (placeholder - actual implementation needs DB)
pub fn run_migrations(_db: Connection) -> Result(Int, String) {
  // Read migration files to validate they exist
  case read_migration_files() {
    Error(err) -> Error(err)
    Ok(files) -> {
      // For now, just return the count of migrations that would be run
      // Actual DB execution will be implemented with epgsql
      Ok(list.length(files))
    }
  }
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
