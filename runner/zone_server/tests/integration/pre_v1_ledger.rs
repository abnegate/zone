use sqlx::migrate::{MigrateError, Migrator};
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::db::migrations::legacy::BASELINE;
use zone_server::db::migrations::legacy::unfinished::Unfinished;

use crate::common::Database;

const PREFIX: &str = "pre_v1_ledger";

/// `_sqlx_migrations` as recorded by a database migrated through 024 from
/// commit 218739e7, kept apart from `legacy::HISTORY` so a typo there fails here.
const PRE_V1_LEDGER: &str = include_str!("../fixtures/pre_v1_ledger.tsv");

type Row = (i64, String, Vec<u8>);

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

fn pre_v1() -> Vec<Row> {
    PRE_V1_LEDGER
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            let mut field = || fields.next().expect("a version, description and checksum");
            let version = field().parse().expect("a numeric version");
            let description = field().to_string();
            let checksum = hex::decode(field()).expect("a hex checksum");
            (version, description, checksum)
        })
        .collect()
}

fn embedded() -> Vec<Row> {
    MIGRATOR
        .iter()
        .map(|migration| {
            (
                migration.version,
                migration.description.to_string(),
                migration.checksum.to_vec(),
            )
        })
        .collect()
}

/// A database whose schema is the baseline's and nothing later, as every
/// database that ran the pre-v1 history is.
async fn at_baseline() -> Database {
    let database = Database::new(PREFIX).await;
    MIGRATOR.run_to(BASELINE, &database.pool).await.unwrap();
    database
}

async fn forge(database: &Database, ledger: &[Row]) {
    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&database.pool)
        .await
        .unwrap();
    for (version, description, checksum) in ledger {
        sqlx::query(
            "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) \
             VALUES($1,$2,TRUE,$3,1)",
        )
        .bind(version)
        .bind(description)
        .bind(checksum)
        .execute(&database.pool)
        .await
        .unwrap();
    }
}

async fn ledger(database: &Database) -> Vec<Row> {
    sqlx::query_as("SELECT version, description, checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&database.pool)
        .await
        .unwrap()
}

async fn chat(database: &Database) -> Uuid {
    let chat = Uuid::new_v4();
    sqlx::query("INSERT INTO chats(id,workspace_id,title,model_name) VALUES($1,'00000000-0000-0000-0000-000000000001','Kept','sonnet')")
        .bind(chat)
        .execute(&database.pool)
        .await
        .unwrap();
    chat
}

#[tokio::test]
async fn a_database_migrated_through_the_last_pre_v1_migration_moves_onto_the_baseline() {
    let database = at_baseline().await;
    forge(&database, &pre_v1()).await;
    let kept = chat(&database).await;

    migrations::run(&database.pool).await.unwrap();

    let upgraded = ledger(&database).await;
    assert_eq!(
        upgraded,
        embedded(),
        "the pre-v1 ledger collapses into the baseline's row, then every later migration applies"
    );
    let title: String = sqlx::query_scalar("SELECT title FROM chats WHERE id=$1")
        .bind(kept)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(title, "Kept", "the rebase keeps every row of data");

    migrations::run(&database.pool).await.unwrap();
    assert_eq!(
        ledger(&database).await,
        upgraded,
        "a second boot changes nothing"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_database_short_of_the_last_pre_v1_migration_is_refused_untouched() {
    let database = at_baseline().await;
    let mut short = pre_v1();
    short.pop();
    forge(&database, &short).await;
    let before = ledger(&database).await;

    let error = migrations::run(&database.pool)
        .await
        .expect_err("a partial pre-v1 history cannot be rebased");

    let MigrateError::Source(source) = error else {
        panic!("expected the unfinished history, got {error}");
    };
    assert_eq!(
        source.downcast_ref::<Unfinished>(),
        Some(&Unfinished {
            missing: vec![24],
            failed: vec![],
            unexpected: vec![],
        })
    );
    assert_eq!(
        ledger(&database).await,
        before,
        "a refused rebase writes nothing"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn an_index_an_interrupted_pre_v1_migration_left_invalid_is_rebuilt() {
    const VALID: &str =
        "SELECT indisvalid FROM pg_index WHERE indexrelid = 'idx_messages_typed'::regclass";
    let database = at_baseline().await;
    forge(&database, &pre_v1()).await;
    sqlx::query(
        "UPDATE pg_index SET indisvalid = FALSE WHERE indexrelid = 'idx_messages_typed'::regclass",
    )
    .execute(&database.pool)
    .await
    .unwrap();

    migrations::run(&database.pool).await.unwrap();

    let valid: bool = sqlx::query_scalar(VALID)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!(valid, "the rebase rebuilds what 019 left invalid");
    database.cleanup().await;
}

#[tokio::test]
async fn a_database_holding_a_branch_only_migration_is_refused_untouched() {
    let database = at_baseline().await;
    forge(&database, &pre_v1()).await;
    sqlx::query(
        "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) \
         VALUES(16,'branch only',TRUE,'\\x00'::bytea,1)",
    )
    .execute(&database.pool)
    .await
    .unwrap();
    let before = ledger(&database).await;

    let error = migrations::run(&database.pool)
        .await
        .expect_err("a migration main never had cannot be rebased");

    assert!(
        error.to_string().contains("unrecognised [16]"),
        "the refusal names the branch's migration: {error}"
    );
    assert_eq!(
        ledger(&database).await,
        before,
        "a refused rebase writes nothing"
    );
    database.cleanup().await;
}
