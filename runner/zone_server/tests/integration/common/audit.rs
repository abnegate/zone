use std::time::Duration;

use chrono::{TimeZone, Utc};
use serde_json::json;
use sqlx::PgPool;
use tokio::time::{Instant, sleep};
use uuid::Uuid;
use zone_enterprise::audit::{Event, VERSION};
use zone_server::db::audit::scope::Scope;
use zone_server::db::migrations;
use zone_server::services::audit::transport::Batch;

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

pub fn batch(scope: Scope, count: u32) -> Batch {
    let start = Utc.with_ymd_and_hms(2026, 11, 17, 9, 30, 5).unwrap();
    let events = (0..count)
        .map(|index| Event {
            id: Uuid::new_v4(),
            version: VERSION,
            created_at: start + chrono::Duration::microseconds(i64::from(index) * 1_001),
            organization_id: scope.organization_id(),
            workspace_id: Some(Uuid::new_v4()),
            actor_id: Some(Uuid::new_v4()),
            actor_email: Some(format!("actor{index}@example.com")),
            action: "member.removed".to_owned(),
            resource_type: "member".to_owned(),
            resource_id: Some(Uuid::new_v4()),
            old_values: Some(json!({"role": "admin", "note": "line\nbreak"})),
            new_values: None,
            ip_address: Some("192.0.2.10".to_owned()),
            user_agent: Some("Mozilla/5.0".to_owned()),
        })
        .collect();
    Batch {
        destination_id: Uuid::new_v4(),
        scope,
        events,
    }
}
