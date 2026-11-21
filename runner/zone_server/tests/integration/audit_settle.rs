use std::time::Duration;

use sqlx::{Connection, PgConnection};
use tokio::time::{sleep, timeout};

use crate::common::Database;
use crate::common::audit::{database, settle, settle_within, write};
use crate::common::database::{DROPPING, url};

const PREFIX: &str = "audit_settle";
const BLOCKED: Duration = Duration::from_millis(300);
const PROMPTLY: Duration = Duration::from_secs(5);
const HOLD: Duration = Duration::from_millis(1_800);
const LIMIT: Duration = Duration::from_millis(1_200);
const SHARED: &str = "SELECT pg_advisory_lock_shared($1)";
const EXCLUSIVE: &str = "SELECT pg_advisory_lock($1)";
const EXISTS: &str = "SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)";
const RUNNING: &str = "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE query = $1)";

async fn gate(statement: &'static str) -> PgConnection {
    let mut connection = PgConnection::connect(&url()).await.unwrap();
    sqlx::query(statement)
        .bind(DROPPING)
        .execute(&mut connection)
        .await
        .unwrap();
    connection
}

async fn exists(name: &str) -> bool {
    ask(EXISTS, name.to_owned()).await
}

async fn running(name: &str) -> bool {
    ask(RUNNING, format!("DROP DATABASE {name}")).await
}

async fn ask(question: &'static str, value: String) -> bool {
    let mut connection = PgConnection::connect(&url()).await.unwrap();
    let answer = sqlx::query_scalar(question)
        .bind(value)
        .fetch_one(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    answer
}

#[tokio::test]
async fn settle_starts_its_time_limit_only_once_running_drops_finish() {
    let database = database(PREFIX).await;
    let mut dropper = gate(SHARED).await;
    sqlx::query("BEGIN").execute(&mut dropper).await.unwrap();
    sqlx::query("SELECT pg_current_xact_id()")
        .execute(&mut dropper)
        .await
        .unwrap();
    write(&database.pool, None).await;
    let finish = async move {
        sleep(HOLD).await;
        sqlx::query("COMMIT").execute(&mut dropper).await.unwrap();
        dropper.close().await.unwrap();
    };

    tokio::join!(settle_within(&database.pool, LIMIT), finish);
    database.cleanup().await;
}

#[tokio::test]
async fn settle_does_not_wait_for_a_drop_its_rows_are_already_past() {
    let database = database(PREFIX).await;
    let dropper = gate(SHARED).await;

    assert!(
        timeout(PROMPTLY, settle(&database.pool)).await.is_ok(),
        "settle waited for a drop that holds no row back"
    );
    dropper.close().await.unwrap();
    database.cleanup().await;
}

#[tokio::test]
async fn a_test_database_is_dropped_only_through_the_drop_gate() {
    let doomed = Database::new(PREFIX).await;
    let quiescing = gate(EXCLUSIVE).await;
    let name = doomed.name.clone();
    let mut cleanup = tokio::spawn(doomed.cleanup());

    assert!(
        timeout(BLOCKED, &mut cleanup).await.is_err(),
        "the drop ran while settle held the gate"
    );
    assert!(
        !running(&name).await,
        "the drop started while settle held the gate"
    );
    assert!(exists(&name).await, "the database outlives the closed gate");
    quiescing.close().await.unwrap();
    cleanup.await.expect("the cleanup finishes");
    assert!(
        !exists(&name).await,
        "the database is dropped once the gate opens"
    );
}
