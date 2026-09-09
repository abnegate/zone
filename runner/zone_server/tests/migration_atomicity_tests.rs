//! sqlx wraps each migration and its bookkeeping row in one transaction
//! "so we never execute migrations twice" (sqlx-postgres/src/migrate.rs).
//!
//! Six of ours open `BEGIN;` and close `COMMIT;` themselves. Postgres treats
//! the inner `BEGIN` as a no-op warning and the inner `COMMIT` as a commit of
//! the transaction sqlx opened, so the schema lands before sqlx writes the
//! `_sqlx_migrations` row. A crash in that window leaves a database that is
//! migrated but does not say so, and the next boot runs the migration again.
//!
//! Deleting those `BEGIN;`/`COMMIT;` lines is not the fix. sqlx keys a
//! migration by a hash of its bytes and refuses to start when a recorded
//! migration's bytes have changed (`MigrateError::VersionMismatch`), and
//! `main.rs` runs migrations with `.expect(...)`. Editing a migration that has
//! already been applied anywhere panics that server at boot -- a worse bug
//! than the one it fixes, and unrecoverable without hand-editing the table.
//!
//! What makes the window survivable is that all six are idempotent, so the
//! re-run is a no-op. That is the property under test. Both tests are here to
//! keep it true: a new migration must let sqlx own the transaction, and the
//! six grandfathered ones must stay safe to apply twice.

use sqlx::{AssertSqlSafe, Connection, PgConnection};
use std::path::{Path, PathBuf};

/// Migrations that commit themselves, and so are outside sqlx's protection.
/// Closed set: entries may not be added, and may not be removed while the file
/// still contains the statements (see the checksum note above).
const SELF_COMMITTING: &[&str] = &[
    "001_initial_schema.sql",
    "002_chat_agent.sql",
    "003_chat_agent_sandbox.sql",
    "009_chat_auto_approve.sql",
    "013_chat_character.sql",
    "015_chat_reasoning_effort.sql",
];

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
async fn a_migration_that_commits_itself_is_safe_to_apply_twice() {
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

    // Replays the crash window: the statements ran and committed, sqlx never
    // recorded them, and the next boot hands them to Postgres a second time.
    for (name, sql) in migrations
        .iter()
        .filter(|(name, _)| SELF_COMMITTING.contains(&name.as_str()))
    {
        sqlx::raw_sql(AssertSqlSafe(sql.clone()))
            .execute(&mut scratch)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{name} commits itself, so sqlx cannot roll it back and cannot \
                 guarantee it is recorded. A server that dies before the \
                 bookkeeping row is written applies it again on the next boot, \
                 and that second apply fails here: {error}\n\n\
                 Make every statement in {name} idempotent (IF NOT EXISTS, \
                 DROP ... IF EXISTS before CREATE). Do not remove its BEGIN;/\
                 COMMIT; instead -- sqlx hashes the file, and changing an \
                 already-applied migration stops every existing install from \
                 booting."
                )
            });
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

    let expected: Vec<String> = SELF_COMMITTING
        .iter()
        .map(|name| name.to_string())
        .collect();

    assert_eq!(
        offenders, expected,
        "the set of migrations that manage their own transaction has changed.\n\n\
         If a migration was ADDED to this set: remove its BEGIN;/COMMIT;. sqlx \
         already wraps each migration and its bookkeeping row in one \
         transaction; committing inside that ends it early, so a crash before \
         the row is written leaves the migration applied but unrecorded.\n\n\
         If a migration was REMOVED from this set: put the lines back. sqlx \
         keys a migration by a hash of its bytes and refuses to start when the \
         bytes of an applied migration change, so editing one of these panics \
         every database that already ran it. The listed six are grandfathered \
         and stay as they are."
    );
}
