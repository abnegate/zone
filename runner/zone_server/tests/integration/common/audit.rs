use std::time::Duration;

use sqlx::PgPool;
use tokio::time::{Instant, sleep};
use zone_server::db::migrations;

use crate::common::database::Database;

const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);
const SETTLE_POLL: Duration = Duration::from_millis(50);
const SETTLED: &str = "SELECT ((pg_snapshot_xmin(pg_current_snapshot()))::text)::bigint \
    > COALESCE((SELECT max(transaction_id) FROM audit_logs), 0)";

pub async fn database(prefix: &'static str) -> Database {
    let database = Database::new(prefix).await;
    migrations::run(&database.pool)
        .await
        .expect("the test database migrates");
    database
}

pub async fn settle(pool: &PgPool) {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    loop {
        let settled: bool = sqlx::query_scalar(SETTLED)
            .fetch_one(pool)
            .await
            .expect("the snapshot horizon is readable");
        if settled {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "audit rows stayed at or above the snapshot horizon for {SETTLE_TIMEOUT:?}: a transaction elsewhere in the cluster is still open"
        );
        sleep(SETTLE_POLL).await;
    }
}
