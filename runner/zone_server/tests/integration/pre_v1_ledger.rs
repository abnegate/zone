use sqlx::migrate::MigrateError;
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::db::migrations::legacy::{BASELINE, HISTORY};
use zone_server::db::migrations::unfinished::Unfinished;

use crate::common::Database;

const PREFIX: &str = "pre_v1_ledger";

async fn forge(database: &Database, history: &[(i64, &str)]) {
    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&database.pool)
        .await
        .unwrap();
    for (version, checksum) in history {
        sqlx::query(
            "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) \
             VALUES($1,'pre-v1',TRUE,$2,1)",
        )
        .bind(version)
        .bind(hex::decode(checksum).unwrap())
        .execute(&database.pool)
        .await
        .unwrap();
    }
}

async fn ledger(database: &Database) -> Vec<(i64, String, Vec<u8>)> {
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
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let baseline = ledger(&database).await;
    forge(&database, HISTORY).await;
    let kept = chat(&database).await;

    migrations::run(&database.pool).await.unwrap();

    assert_eq!(
        ledger(&database).await,
        baseline,
        "the pre-v1 ledger collapses into the baseline's single row"
    );
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0].0, BASELINE);
    let title: String = sqlx::query_scalar("SELECT title FROM chats WHERE id=$1")
        .bind(kept)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(title, "Kept", "the rebase keeps every row of data");

    migrations::run(&database.pool).await.unwrap();
    assert_eq!(
        ledger(&database).await,
        baseline,
        "a second boot changes nothing"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_database_short_of_the_last_pre_v1_migration_is_refused_untouched() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let short = &HISTORY[..HISTORY.len() - 1];
    forge(&database, short).await;
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
