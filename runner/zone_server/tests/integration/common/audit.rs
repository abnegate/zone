use std::time::Duration;

use axum::http::StatusCode;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;
use sqlx::{Executor, PgPool, Postgres};
use tokio::time::{Instant, sleep};
use uuid::Uuid;
use wiremock::Request;
use zone_enterprise::audit::{Event, Kind, VERSION};
use zone_server::db::audit::position::Position;
use zone_server::db::audit::scope::Scope;
use zone_server::db::audit::{actions, resources};
use zone_server::db::{migrations, organizations};
use zone_server::services::audit::transport::Batch;

use crate::common::TestResponse;
use crate::common::database::Database;

const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);
const SETTLE_POLL: Duration = Duration::from_millis(50);
const SETTLED: &str = "SELECT ((pg_snapshot_xmin(pg_current_snapshot()))::text)::bigint \
    > COALESCE((SELECT max(transaction_id) FROM audit_logs), 0)";

pub const UNVERIFIABLE: &str = "not a license key";

pub struct Destination {
    pub organization: Option<Uuid>,
    pub kind: Kind,
    pub name: String,
    pub enabled: bool,
}

impl Destination {
    pub fn new(organization: Option<Uuid>) -> Self {
        Self {
            organization,
            kind: Kind::Webhook,
            name: format!("destination-{}", Uuid::new_v4().simple()),
            enabled: true,
        }
    }

    pub async fn create(self, pool: &PgPool, delivered: Position) -> Uuid {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO audit_destinations (organization_id, kind, name, enabled, settings, secrets) \
             VALUES ($1, $2, $3, $4, '{}'::jsonb, '') RETURNING id",
        )
        .bind(self.organization)
        .bind(self.kind.as_str())
        .bind(self.name)
        .bind(self.enabled)
        .fetch_one(pool)
        .await
        .expect("the destination is created");
        sqlx::query(
            "INSERT INTO audit_deliveries (destination_id, transaction_id, sequence) \
             VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(delivered.transaction_id)
        .bind(delivered.sequence)
        .execute(pool)
        .await
        .expect("the delivery is created");
        id
    }
}

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

pub async fn organization(pool: &PgPool) -> Uuid {
    let slug = format!("audit-{}", Uuid::new_v4().simple());
    organizations::create_organization(pool, &slug, &slug, None)
        .await
        .expect("the organization is created")
        .id
}

pub async fn write<'e, E>(executor: E, organization: Option<Uuid>) -> Uuid
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(
        "INSERT INTO audit_logs (organization_id, action, resource_type) VALUES ($1, $2, $3) \
         RETURNING id",
    )
    .bind(organization)
    .bind(actions::SETTINGS_UPDATED)
    .bind(resources::ORGANIZATION)
    .fetch_one(executor)
    .await
    .expect("the audit row is written")
}

pub async fn position(pool: &PgPool, id: Uuid) -> Position {
    let (transaction_id, sequence): (i64, i64) =
        sqlx::query_as("SELECT transaction_id, sequence FROM audit_logs WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the audit row exists");
    Position {
        transaction_id,
        sequence,
    }
}

pub async fn created_at(pool: &PgPool, id: Uuid) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT created_at FROM audit_logs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("the audit row exists")
}

pub fn refused(response: &TestResponse, status: StatusCode, code: &str) {
    response.assert_status(status);
    assert_eq!(response.json_value()["code"], code, "{}", response.text());
}

pub fn header<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .unwrap_or_else(|| panic!("the request carries {name}"))
        .to_str()
        .unwrap()
}
