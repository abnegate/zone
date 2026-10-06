//! sqlx wraps each migration and its bookkeeping row in one transaction
//! "so we never execute migrations twice" (sqlx-postgres/src/migrate.rs).
//!
//! A migration that opens `BEGIN;` and closes `COMMIT;` itself ends that
//! transaction early, so a crash before the `_sqlx_migrations` row is written
//! leaves the schema applied but unrecorded. Fresh migrations leave the
//! transaction to sqlx.

use sqlx::{AssertSqlSafe, Connection, PgConnection};
use std::path::{Path, PathBuf};

const SCRATCH_DATABASE: &str = "zone_migration_atomicity";

fn migrations_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn migrations() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = std::fs::read_dir(migrations_directory())
        .expect("migrations directory is readable")
        .map(|entry| entry.expect("migration directory entry is readable").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .map(|path| {
            let name = path
                .file_name()
                .expect("migration path has a file name")
                .to_string_lossy()
                .into_owned();
            (
                name,
                std::fs::read_to_string(&path).expect("migration is readable"),
            )
        })
        .collect();

    found.sort_by(|(left, _), (right, _)| left.cmp(right));
    assert!(!found.is_empty(), "no migrations found to check");
    found
}

fn commits_itself(sql: &str) -> bool {
    sql.lines().any(|line| {
        matches!(
            line.trim().to_ascii_uppercase().as_str(),
            "BEGIN;" | "COMMIT;"
        )
    })
}

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string())
}

/// Swaps the database out of DATABASE_URL, keeping the server, credentials
/// and any query parameters that got us onto it.
fn url_for_database(name: &str) -> String {
    let base = database_url();
    let (server, tail) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database");
    let parameters = tail.find('?').map(|at| &tail[at..]).unwrap_or("");

    format!("{server}/{name}{parameters}")
}

/// `CREATE DATABASE` cannot run inside the database it creates, so this
/// connects to a sibling on the same server.
async fn connect_to_maintenance_database() -> PgConnection {
    PgConnection::connect(&url_for_database("postgres"))
        .await
        .expect("test database server is reachable")
}

async fn connect_to_scratch_database() -> PgConnection {
    PgConnection::connect(&url_for_database(SCRATCH_DATABASE))
        .await
        .expect("scratch database is reachable")
}

#[tokio::test]
async fn the_initial_schema_applies_to_an_empty_database() {
    let drop_scratch = format!("DROP DATABASE IF EXISTS {SCRATCH_DATABASE} WITH (FORCE)");
    let create_scratch = format!("CREATE DATABASE {SCRATCH_DATABASE}");

    let mut maintenance = connect_to_maintenance_database().await;
    sqlx::raw_sql(AssertSqlSafe(drop_scratch.clone()))
        .execute(&mut maintenance)
        .await
        .expect("scratch database can be dropped");
    sqlx::raw_sql(AssertSqlSafe(create_scratch))
        .execute(&mut maintenance)
        .await
        .expect("scratch database can be created");
    drop(maintenance);

    let migrations = migrations();
    let mut scratch = connect_to_scratch_database().await;

    for (name, sql) in &migrations {
        sqlx::raw_sql(AssertSqlSafe(sql.clone()))
            .execute(&mut scratch)
            .await
            .unwrap_or_else(|error| panic!("{name} did not apply to an empty database: {error}"));
    }

    drop(scratch);

    let mut maintenance = connect_to_maintenance_database().await;
    let _ = sqlx::raw_sql(AssertSqlSafe(drop_scratch))
        .execute(&mut maintenance)
        .await;
}

#[test]
fn a_new_migration_leaves_the_transaction_to_sqlx() {
    let offenders: Vec<String> = migrations()
        .into_iter()
        .filter(|(_, sql)| commits_itself(sql))
        .map(|(name, _)| name)
        .collect();

    assert!(
        offenders.is_empty(),
        "these migrations manage their own transaction: {offenders:?}\n\n\
         Remove their BEGIN;/COMMIT;. sqlx already wraps each migration and its \
         bookkeeping row in one transaction; committing inside that ends it early, \
         so a crash before the row is written leaves the migration applied but unrecorded."
    );
}
