use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tokio::time::Instant;
use uuid::Uuid;
use zone_server::db::audit::events::{self, Bookmark, Expired, Range};
use zone_server::db::audit::position::Position;
use zone_server::db::audit::scope::Scope;
use zone_server::db::audit::{AuditContext, actions, log_action, resources};

use crate::common::audit::{
    Destination, created_at, database, organization, position, settle, write,
};
use crate::common::create_test_pool;

const BATCH: i64 = 2;
const LOCK_WAIT: StdDuration = StdDuration::from_secs(30);
const LOCK_POLL: StdDuration = StdDuration::from_millis(20);
const RESTORED: i64 = 1 << 40;
const SEEDED: i64 = 200_000;
const SEEDED_COLLIDED: StdDuration = StdDuration::from_secs(5);
const SCANNING: [(&str, &str); 3] = [
    ("enable_indexscan", "off"),
    ("enable_indexonlyscan", "off"),
    ("enable_bitmapscan", "off"),
];
const SERVER_SEEDED: i64 = 20_000;
const SERVER_PAGE: i64 = 100;
const SERVER_PAGE_READS: i64 = 1_000;
const ORIGIN: Position = Position {
    transaction_id: 0,
    sequence: 0,
};

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Delivery {
    transaction_id: i64,
    sequence: i64,
    batch_transaction_id: Option<i64>,
    batch_sequence: Option<i64>,
    lease_owner: Option<Uuid>,
    lease_expires_at: Option<DateTime<Utc>>,
}

impl Delivery {
    fn position(&self) -> Position {
        Position {
            transaction_id: self.transaction_id,
            sequence: self.sequence,
        }
    }
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Stored {
    id: Uuid,
    transaction_id: i64,
    sequence: i64,
}

async fn written(pool: &PgPool, organization: Option<Uuid>, count: usize) -> Vec<Uuid> {
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(write(pool, organization).await);
    }
    ids
}

async fn age(pool: &PgPool, ids: &[Uuid], created_at: DateTime<Utc>) {
    sqlx::query("UPDATE audit_logs SET created_at = $2 WHERE id = ANY($1)")
        .bind(ids)
        .bind(created_at)
        .execute(pool)
        .await
        .expect("the audit rows are dated");
}

async fn remaining(pool: &PgPool, organization: Option<Uuid>) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT id FROM audit_logs WHERE organization_id IS NOT DISTINCT FROM $1 \
         ORDER BY transaction_id, sequence",
    )
    .bind(organization)
    .fetch_all(pool)
    .await
    .expect("the audit rows are readable")
}

async fn stored(pool: &PgPool) -> Vec<Stored> {
    sqlx::query_as("SELECT id, transaction_id, sequence FROM audit_logs ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("the audit rows are readable")
}

async fn drain(pool: &PgPool, scope: Scope, mut cursor: Position) -> (Vec<Uuid>, Position) {
    let mut delivered = Vec::new();
    loop {
        let rows = events::after(pool, scope, cursor, None, BATCH)
            .await
            .expect("the final rows are readable");
        let Some(last) = rows.last() else {
            return (delivered, cursor);
        };
        cursor = last.position();
        delivered.extend(rows.iter().map(|row| row.id));
    }
}

async fn restore(pool: &PgPool, ids: &[Uuid]) {
    sqlx::query("UPDATE audit_logs SET transaction_id = transaction_id + $2 WHERE id = ANY($1)")
        .bind(ids)
        .bind(RESTORED)
        .execute(pool)
        .await
        .expect("the restored rows move above the horizon");
}

fn restored(position: Position) -> Position {
    Position {
        transaction_id: position.transaction_id + RESTORED,
        sequence: position.sequence,
    }
}

async fn lease(pool: &PgPool, destination: Uuid, batch: Position, owner: Uuid) {
    sqlx::query(
        "UPDATE audit_deliveries SET batch_transaction_id = $2, batch_sequence = $3, \
         lease_owner = $4, lease_expires_at = now() + interval '2 minutes' \
         WHERE destination_id = $1",
    )
    .bind(destination)
    .bind(batch.transaction_id)
    .bind(batch.sequence)
    .bind(owner)
    .execute(pool)
    .await
    .expect("the delivery is leased");
}

async fn delivery(pool: &PgPool, destination: Uuid) -> Delivery {
    sqlx::query_as(
        "SELECT transaction_id, sequence, batch_transaction_id, batch_sequence, lease_owner, \
         lease_expires_at FROM audit_deliveries WHERE destination_id = $1",
    )
    .bind(destination)
    .fetch_one(pool)
    .await
    .expect("the delivery exists")
}

async fn pending(pool: &PgPool, scope: Scope, destination: Uuid) -> Vec<Uuid> {
    let cursor = delivery(pool, destination).await.position();
    drain(pool, scope, cursor).await.0
}

async fn seed(pool: &PgPool, organization: Option<Uuid>, count: i64) {
    sqlx::query(
        "INSERT INTO audit_logs (organization_id, action, resource_type) \
         SELECT $1, $2, $3 FROM generate_series(1, $4)",
    )
    .bind(organization)
    .bind(actions::SETTINGS_UPDATED)
    .bind(resources::ORGANIZATION)
    .bind(count)
    .execute(pool)
    .await
    .expect("the audit rows are seeded");
}

async fn analyze(pool: &PgPool) {
    sqlx::query("ANALYZE audit_logs")
        .execute(pool)
        .await
        .expect("the audit log is analyzed");
}

/// A pool whose sessions read every table by scanning it, as the planner may choose for a log
/// restored moments ago, so a statement's cost cannot hide behind a lucky index path.
async fn scanning(pool: &PgPool) -> PgPool {
    let options = (*pool.connect_options()).clone().options(SCANNING);
    PgPool::connect_with(options)
        .await
        .expect("the scanning pool connects")
}

async fn rebase(pool: &PgPool) -> bool {
    events::rebase(pool).await.expect("the rebase succeeds")
}

fn counted(total: i64, held: i64) -> Expired {
    Expired {
        total,
        total_capped: false,
        held,
        held_capped: false,
    }
}

async fn waiting_for_rebase_lock(pool: &PgPool) {
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'advisory' AND NOT granted \
             AND database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
             AND (classid::bigint << 32 | objid::bigint) = $1)",
        )
        .bind(events::REBASE_LOCK)
        .fetch_one(pool)
        .await
        .expect("the locks are readable");
        if waiting {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "nothing waited on the rebase lock within {LOCK_WAIT:?}"
        );
        tokio::time::sleep(LOCK_POLL).await;
    }
}

#[tokio::test]
async fn after_never_skips_a_row_an_open_transaction_commits_later() {
    let database = database("audit_events_open").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    settle(pool).await;
    let start = events::start(pool).await.expect("the start is readable");

    let first = write(pool, Some(organization)).await;
    let mut open = pool.begin().await.expect("a transaction begins");
    let held = write(&mut *open, Some(organization)).await;
    let last = write(pool, Some(organization)).await;
    let (early, cursor) = drain(pool, scope, start).await;
    open.commit().await.expect("the open transaction commits");

    assert!(
        !early.contains(&held) && !early.contains(&last),
        "a row from an open transaction, and every row after it, waits for that transaction"
    );
    settle(pool).await;
    let (late, _) = drain(pool, scope, cursor).await;
    let delivered: Vec<Uuid> = early.into_iter().chain(late).collect();
    assert_eq!(
        delivered,
        vec![first, held, last],
        "every row is delivered once, in commit-safe position order"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn stalled_reports_a_committed_row_behind_an_open_transaction() {
    let database = database("audit_events_stalled").await;
    let pool = &database.pool;
    let other = organization(pool).await;
    let organization = organization(pool).await;
    settle(pool).await;

    let mut open = pool.begin().await.expect("a transaction begins");
    sqlx::query("SELECT pg_current_xact_id()")
        .execute(&mut *open)
        .await
        .expect("the open transaction takes an xid");
    let waiting = write(pool, Some(organization)).await;
    let scope = Scope::Organization(organization);
    let stalled = events::stalled(pool, scope)
        .await
        .expect("stalled is readable");
    let elsewhere = events::stalled(pool, Scope::Organization(other))
        .await
        .expect("stalled is readable");
    let server = events::stalled(pool, Scope::Server)
        .await
        .expect("stalled is readable");
    open.rollback().await.expect("the open transaction ends");

    assert_eq!(stalled, Some(created_at(pool, waiting).await));
    assert_eq!(elsewhere, None, "another organization has nothing stalled");
    assert_eq!(server, None, "the server scope has nothing stalled");
    settle(pool).await;
    assert_eq!(
        events::stalled(pool, scope)
            .await
            .expect("stalled is readable"),
        None,
        "nothing is stalled once the transaction ends"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn reads_are_scoped_to_one_organization_or_the_server() {
    let database = database("audit_events_scope").await;
    let pool = &database.pool;
    let first = organization(pool).await;
    let second = organization(pool).await;
    let ours = written(pool, Some(first), 3).await;
    let theirs = written(pool, Some(second), 1).await;
    let context = AuditContext {
        org_id: None,
        workspace_id: None,
        actor_id: None,
        actor_email: Some("server@example.com".to_string()),
        ip_address: Some("192.168.1.1".to_string()),
        user_agent: None,
    };
    let server = log_action(
        pool,
        &context,
        actions::LICENSE_SET,
        resources::LICENSE,
        None,
        None,
        None,
    )
    .await
    .expect("the server row is written");
    settle(pool).await;

    assert_eq!(
        drain(pool, Scope::Organization(first), ORIGIN).await.0,
        ours
    );
    assert_eq!(
        drain(pool, Scope::Organization(second), ORIGIN).await.0,
        theirs
    );
    let rows = events::after(pool, Scope::Server, ORIGIN, None, BATCH)
        .await
        .expect("the server rows are readable");
    assert_eq!(
        rows.len(),
        1,
        "only the organization-less row is the server's"
    );
    assert_eq!(rows[0].id, server);
    assert_eq!(rows[0].ip_address.as_deref(), Some("192.168.1.1"));
    assert_eq!(rows[0].actor_email.as_deref(), Some("server@example.com"));

    let through = position(pool, ours[1]).await;
    let bounded = events::after(pool, Scope::Organization(first), ORIGIN, Some(through), 10)
        .await
        .expect("the bounded rows are readable");
    assert_eq!(
        bounded.iter().map(|row| row.id).collect::<Vec<_>>(),
        ours[..2].to_vec(),
        "rows past `through` are left for a later batch"
    );

    let backlog = events::backlog(pool, Scope::Organization(first), through)
        .await
        .expect("the backlog is readable");
    assert_eq!(backlog.count, 1);
    assert_eq!(backlog.oldest, Some(created_at(pool, ours[2]).await));
    let server_backlog = events::backlog(pool, Scope::Server, ORIGIN)
        .await
        .expect("the backlog is readable");
    assert_eq!(server_backlog.count, 1);
    let caught_up = events::backlog(pool, Scope::Server, position(pool, server).await)
        .await
        .expect("the backlog is readable");
    assert_eq!(caught_up.count, 0);
    assert_eq!(caught_up.oldest, None);
    database.cleanup().await;
}

#[tokio::test]
async fn prune_deletes_only_expired_rows_at_or_below_the_floor() {
    let database = database("audit_events_prune").await;
    let pool = &database.pool;
    let other = organization(pool).await;
    let organization = organization(pool).await;
    let old = written(pool, Some(organization), 3).await;
    let fresh = write(pool, Some(organization)).await;
    let elsewhere = write(pool, Some(other)).await;
    let long_ago = Utc::now() - Duration::days(10);
    age(pool, &old, long_ago).await;
    age(pool, &[elsewhere], long_ago).await;
    settle(pool).await;
    let scope = Scope::Organization(organization);
    let before = Utc::now() - Duration::days(1);
    let floor = position(pool, old[1]).await;
    let ours = Destination::new(Some(organization))
        .create(pool, floor)
        .await;
    Destination::new(Some(other)).create(pool, ORIGIN).await;

    assert_eq!(
        events::expired(pool, scope, before, Some(floor))
            .await
            .expect("expired is readable"),
        counted(3, 1)
    );
    assert_eq!(
        events::expired(pool, scope, before, None)
            .await
            .expect("expired is readable"),
        counted(3, 0),
        "with no enabled destination nothing is held"
    );
    let deleted = events::prune(pool, scope, before, 100)
        .await
        .expect("the prune succeeds");
    assert_eq!(deleted, 2);
    assert_eq!(
        remaining(pool, Some(organization)).await,
        vec![old[2], fresh]
    );
    assert_eq!(
        remaining(pool, Some(other)).await,
        vec![elsewhere],
        "another organization's rows are never pruned"
    );

    sqlx::query("UPDATE audit_destinations SET enabled = false WHERE id = $1")
        .bind(ours)
        .execute(pool)
        .await
        .expect("the destination is disabled");
    let deleted = events::prune(pool, scope, before, 100)
        .await
        .expect("the prune succeeds");
    assert_eq!(deleted, 1, "a disabled destination holds nothing back");
    assert_eq!(remaining(pool, Some(organization)).await, vec![fresh]);
    database.cleanup().await;
}

#[tokio::test]
async fn prune_respects_its_limit() {
    let database = database("audit_events_limit").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let old = written(pool, Some(organization), 5).await;
    age(pool, &old, Utc::now() - Duration::days(10)).await;
    settle(pool).await;
    let scope = Scope::Organization(organization);
    let before = Utc::now() - Duration::days(1);

    let mut batches = Vec::new();
    loop {
        let deleted = events::prune(pool, scope, before, BATCH)
            .await
            .expect("the prune succeeds");
        if deleted == 0 {
            break;
        }
        batches.push(deleted);
    }
    assert_eq!(batches, vec![2, 2, 1]);
    assert!(remaining(pool, Some(organization)).await.is_empty());
    database.cleanup().await;
}

#[tokio::test]
async fn page_walks_the_range_in_keyset_order() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let base = Utc::now() - Duration::days(3);
    let early = write(&pool, Some(organization)).await;
    let inside = written(&pool, Some(organization), 5).await;
    let late = write(&pool, Some(organization)).await;
    let moments = [
        base,
        base,
        base + Duration::hours(1),
        base + Duration::hours(2),
        base + Duration::hours(3),
    ];
    for (id, moment) in inside.iter().zip(moments) {
        age(&pool, &[*id], moment).await;
    }
    age(&pool, &[early], base - Duration::hours(1)).await;
    age(&pool, &[late], base + Duration::hours(4)).await;
    let range = Range {
        start: base,
        end: base + Duration::hours(3),
    };
    let mut expected: Vec<(DateTime<Utc>, Uuid)> = inside
        .iter()
        .zip(moments)
        .map(|(id, moment)| (moment, *id))
        .collect();
    expected.sort();

    let mut pages = Vec::new();
    let mut bookmark: Option<Bookmark> = None;
    loop {
        let rows = events::page(
            &pool,
            Scope::Organization(organization),
            range,
            bookmark,
            BATCH,
        )
        .await
        .expect("the page is readable");
        let Some(last) = rows.last() else {
            break;
        };
        bookmark = Some(last.bookmark());
        pages.push(rows.iter().map(|row| row.id).collect::<Vec<_>>());
    }

    assert_eq!(
        pages.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![2, 2, 1]
    );
    assert_eq!(
        pages.concat(),
        expected.into_iter().map(|(_, id)| id).collect::<Vec<_>>(),
        "pages follow (created_at, id) and stay inside the range"
    );
}

#[tokio::test]
async fn start_is_at_or_above_every_final_row() {
    let database = database("audit_events_start").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let ids = written(pool, Some(organization), 3).await;
    settle(pool).await;
    let start = events::start(pool).await.expect("the start is readable");

    for id in ids {
        assert!(position(pool, id).await <= start);
    }
    assert!(drain(pool, scope, start).await.0.is_empty());
    let next = write(pool, Some(organization)).await;
    settle(pool).await;
    assert_eq!(drain(pool, scope, start).await.0, vec![next]);
    database.cleanup().await;
}

#[tokio::test]
async fn rebase_after_a_restore_keeps_every_undelivered_row_after_its_cursor() {
    let database = database("audit_events_rebase").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let old = written(pool, Some(organization), 4).await;
    let server_old = written(pool, None, 2).await;
    settle(pool).await;
    let delivered = position(pool, old[1]).await;
    let batch = position(pool, old[2]).await;
    let server_delivered = position(pool, server_old[0]).await;
    restore(pool, &[old.as_slice(), server_old.as_slice()].concat()).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    let owner = Uuid::new_v4();
    lease(pool, ours, restored(batch), owner).await;
    let server = Destination::new(None)
        .create(pool, restored(server_delivered))
        .await;
    let new = write(pool, Some(organization)).await;
    let server_new = write(pool, None).await;

    assert!(
        rebase(pool).await,
        "a restored row above the horizon is rebased"
    );
    settle(pool).await;

    let rebased = delivery(pool, ours).await;
    assert_eq!(rebased.transaction_id, 0);
    assert_eq!(
        (rebased.batch_transaction_id, rebased.batch_sequence),
        (None, None),
        "the pinned batch is recomputed from the new cursor"
    );
    assert_eq!(
        (rebased.lease_owner, rebased.lease_expires_at),
        (None, None),
        "the lease is taken back"
    );
    assert_eq!(
        pending(pool, scope, ours).await,
        vec![old[2], old[3], new],
        "every undelivered row and the new cluster's row sort after the cursor"
    );
    assert_eq!(
        pending(pool, Scope::Server, server).await,
        vec![server_old[1], server_new]
    );
    let unmoved: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE transaction_id <> 0")
            .fetch_one(pool)
            .await
            .expect("the audit rows are readable");
    assert_eq!(unmoved, 0);

    let floor = rebased.position();
    let future = Utc::now() + Duration::days(1);
    assert_eq!(
        events::expired(pool, scope, future, Some(floor))
            .await
            .expect("expired is readable"),
        counted(5, 3),
        "the floor covers only rows delivered under the old order"
    );
    let deleted = events::prune(pool, scope, future, 100)
        .await
        .expect("the prune succeeds");
    assert_eq!(deleted, 2);
    assert_eq!(
        remaining(pool, Some(organization)).await,
        vec![old[2], old[3], new]
    );

    let after = write(pool, Some(organization)).await;
    settle(pool).await;
    assert_eq!(
        pending(pool, scope, ours).await,
        vec![old[2], old[3], new, after],
        "a row written after the rebase follows the rebased rows"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn rebase_does_not_resend_rows_delivered_below_the_new_horizon() {
    let database = database("audit_events_ancient").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let ancient = written(pool, Some(organization), 2).await;
    let old = written(pool, Some(organization), 2).await;
    settle(pool).await;
    let delivered = position(pool, old[0]).await;
    restore(pool, &old).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    let new = write(pool, Some(organization)).await;

    assert!(rebase(pool).await);
    settle(pool).await;

    let resent = pending(pool, scope, ours).await;
    assert_eq!(resent, vec![old[1], new]);
    assert!(
        ancient.iter().all(|id| !resent.contains(id)),
        "old rows that kept an xid below the horizon were delivered and are not sent again"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn rebase_renumbers_rows_written_while_the_identity_lagged() {
    let database = database("audit_events_lagging").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let old = written(pool, Some(organization), 3).await;
    settle(pool).await;
    let delivered = position(pool, old[2]).await;
    restore(pool, &old).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    sqlx::query("SELECT setval(pg_get_serial_sequence('public.audit_logs', 'sequence'), 1, false)")
        .execute(pool)
        .await
        .expect("the identity is reset like a logical replica's");
    let new = written(pool, Some(organization), 2).await;

    assert!(rebase(pool).await);
    settle(pool).await;

    assert_eq!(pending(pool, scope, ours).await, new);
    let (rows, distinct, highest, issued): (i64, i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT count(*), count(DISTINCT sequence), max(sequence), \
         pg_sequence_last_value(pg_get_serial_sequence('public.audit_logs', 'sequence')::regclass) \
         FROM audit_logs",
    )
    .fetch_one(pool)
    .await
    .expect("the sequences are readable");
    assert_eq!(rows, distinct, "no two rows share a sequence");
    assert!(issued >= highest, "the identity issues past every row");
    database.cleanup().await;
}

#[tokio::test]
async fn a_second_rebase_finds_nothing() {
    let database = database("audit_events_twice").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    assert!(!rebase(pool).await, "an empty log has nothing to rebase");
    let old = written(pool, Some(organization), 2).await;
    settle(pool).await;
    assert!(!rebase(pool).await, "rows below the horizon need no rebase");
    let delivered = position(pool, old[0]).await;
    restore(pool, &old).await;
    Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    write(pool, Some(organization)).await;

    assert!(rebase(pool).await);
    assert!(!rebase(pool).await);
    database.cleanup().await;
}

#[tokio::test]
async fn a_cursor_alone_above_the_horizon_is_rebased() {
    let database = database("audit_events_cursor").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let ours = Destination::new(Some(organization))
        .create(
            pool,
            Position {
                transaction_id: RESTORED,
                sequence: i64::MAX,
            },
        )
        .await;
    let new = write(pool, Some(organization)).await;

    assert!(rebase(pool).await);
    settle(pool).await;
    assert_eq!(pending(pool, scope, ours).await, vec![new]);
    database.cleanup().await;
}

#[tokio::test]
async fn the_old_owner_cannot_advance_after_a_rebase() {
    let database = database("audit_events_owner").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let old = written(pool, Some(organization), 3).await;
    settle(pool).await;
    let delivered = position(pool, old[0]).await;
    let batch = position(pool, old[2]).await;
    restore(pool, &old).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    let owner = Uuid::new_v4();
    lease(pool, ours, restored(batch), owner).await;

    assert!(rebase(pool).await);
    let rebased = delivery(pool, ours).await;
    let advanced = sqlx::query(
        "UPDATE audit_deliveries SET transaction_id = $2, sequence = $3, \
         batch_transaction_id = NULL, batch_sequence = NULL, attempts = 0, \
         last_success_at = now(), lease_owner = NULL, lease_expires_at = NULL \
         WHERE destination_id = $1 AND lease_owner = $4",
    )
    .bind(ours)
    .bind(restored(batch).transaction_id)
    .bind(restored(batch).sequence)
    .bind(owner)
    .execute(pool)
    .await
    .expect("the advance runs")
    .rows_affected();

    assert_eq!(advanced, 0, "the lease the advance relied on is gone");
    assert_eq!(delivery(pool, ours).await, rebased);
    database.cleanup().await;
}

#[tokio::test]
async fn a_failed_rebase_changes_nothing() {
    let database = database("audit_events_rollback").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let old = written(pool, Some(organization), 3).await;
    settle(pool).await;
    let delivered = position(pool, old[0]).await;
    let batch = position(pool, old[1]).await;
    restore(pool, &old).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    lease(pool, ours, restored(batch), Uuid::new_v4()).await;
    write(pool, Some(organization)).await;
    sqlx::raw_sql(
        "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'refused'; END $$; \
         CREATE TRIGGER refuse BEFORE UPDATE ON audit_logs \
         FOR EACH ROW EXECUTE FUNCTION refuse();",
    )
    .execute(pool)
    .await
    .expect("the refusing trigger is installed");
    let rows = stored(pool).await;
    let cursor = delivery(pool, ours).await;

    assert!(
        events::rebase(pool).await.is_err(),
        "the rebase reports the failure"
    );
    assert_eq!(stored(pool).await, rows, "no audit row moved");
    assert_eq!(delivery(pool, ours).await, cursor, "no cursor moved");

    sqlx::raw_sql("DROP TRIGGER refuse ON audit_logs")
        .execute(pool)
        .await
        .expect("the refusing trigger is removed");
    assert!(
        rebase(pool).await,
        "the rebase succeeds once the fault clears"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn concurrent_rebases_rebase_once() {
    let database = database("audit_events_concurrent").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let old = written(pool, Some(organization), 3).await;
    settle(pool).await;
    let delivered = position(pool, old[0]).await;
    restore(pool, &old).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    let new = write(pool, Some(organization)).await;

    let (first, second) = tokio::join!(events::rebase(pool), events::rebase(pool));
    let outcomes = [
        first.expect("the first rebase succeeds"),
        second.expect("the second rebase succeeds"),
    ];
    assert_eq!(
        outcomes.iter().filter(|rebased| **rebased).count(),
        1,
        "exactly one rebase moves the positions: {outcomes:?}"
    );
    settle(pool).await;
    assert_eq!(pending(pool, scope, ours).await, vec![old[1], old[2], new]);
    database.cleanup().await;
}

#[tokio::test]
async fn rebase_renumbers_rows_that_reuse_restored_sequences_after_the_identity_caught_up() {
    let database = database("audit_events_collided").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let old = written(pool, Some(organization), 3).await;
    settle(pool).await;
    let delivered = position(pool, old[2]).await;
    restore(pool, &old).await;
    let ours = Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    let restored_highest: i64 = sqlx::query_scalar("SELECT max(sequence) FROM audit_logs")
        .fetch_one(pool)
        .await
        .expect("the sequences are readable");
    sqlx::query("SELECT setval(pg_get_serial_sequence('public.audit_logs', 'sequence'), 1, false)")
        .execute(pool)
        .await
        .expect("the identity is reset like a logical replica's");
    let count = usize::try_from(restored_highest + 2).expect("a small sequence");
    let mut new = written(pool, Some(organization), count).await;
    let issued: i64 = sqlx::query_scalar(
        "SELECT pg_sequence_last_value(pg_get_serial_sequence('public.audit_logs', 'sequence')::regclass)",
    )
    .fetch_one(pool)
    .await
    .expect("the identity is readable");
    assert!(
        issued > restored_highest,
        "the identity issued past every restored row before the rebase"
    );

    assert!(rebase(pool).await);
    settle(pool).await;

    let mut resent = pending(pool, scope, ours).await;
    resent.sort();
    new.sort();
    assert_eq!(
        resent, new,
        "every row written before the rebase is sent, including those that reused a restored sequence"
    );
    let (rows, distinct): (i64, i64) =
        sqlx::query_as("SELECT count(*), count(DISTINCT sequence) FROM audit_logs")
            .fetch_one(pool)
            .await
            .expect("the sequences are readable");
    assert_eq!(rows, distinct, "no two rows share a sequence");
    database.cleanup().await;
}

#[tokio::test]
async fn a_rebase_waits_for_a_prune_batch_in_flight_and_sees_what_it_deleted() {
    let database = database("audit_events_prune_lock").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let old = written(pool, Some(organization), 3).await;
    age(pool, &old[..1], Utc::now() - Duration::days(10)).await;
    settle(pool).await;
    let delivered = position(pool, old[0]).await;
    let ours = Destination::new(Some(organization))
        .create(pool, delivered)
        .await;
    let mut batch = pool.begin().await.expect("the batch begins");
    sqlx::query("SELECT pg_advisory_xact_lock_shared($1)")
        .bind(events::REBASE_LOCK)
        .execute(&mut *batch)
        .await
        .expect("the batch holds positions");
    let pruned = events::prune(&mut *batch, scope, Utc::now() - Duration::days(1), 100)
        .await
        .expect("the prune succeeds");
    assert_eq!(pruned, 1);
    restore(pool, &old[1..]).await;
    sqlx::query("UPDATE audit_deliveries SET transaction_id = $2 WHERE destination_id = $1")
        .bind(ours)
        .bind(restored(delivered).transaction_id)
        .execute(pool)
        .await
        .expect("the cursor is restored");

    let rebasing = tokio::spawn({
        let pool = pool.clone();
        async move { events::rebase(&pool).await }
    });
    waiting_for_rebase_lock(pool).await;
    assert!(!rebasing.is_finished(), "the rebase waits for the batch");
    assert_eq!(delivery(pool, ours).await.position(), restored(delivered));
    batch.commit().await.expect("the batch commits");

    assert!(
        rebasing
            .await
            .expect("the rebase task finishes")
            .expect("the rebase succeeds"),
        "the rebase runs once the batch is done"
    );
    settle(pool).await;
    assert_eq!(remaining(pool, Some(organization)).await, old[1..].to_vec());
    assert_eq!(
        pending(pool, scope, ours).await,
        old[1..].to_vec(),
        "the rows after the cursor are still owed"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_prune_batch_waits_for_a_rebase_in_flight() {
    let database = database("audit_events_rebase_lock").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let old = written(pool, Some(organization), 2).await;
    age(pool, &old, Utc::now() - Duration::days(10)).await;
    settle(pool).await;
    let mut rebase = pool.acquire().await.expect("a connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(events::REBASE_LOCK)
        .execute(&mut *rebase)
        .await
        .expect("the rebase holds the lock");

    let pruning = tokio::spawn({
        let pool = pool.clone();
        async move {
            let mut batch = pool.begin().await?;
            sqlx::query("SELECT pg_advisory_xact_lock_shared($1)")
                .bind(events::REBASE_LOCK)
                .execute(&mut *batch)
                .await?;
            let pruned =
                events::prune(&mut *batch, scope, Utc::now() - Duration::days(1), 100).await?;
            batch.commit().await?;
            Ok::<u64, sqlx::Error>(pruned)
        }
    });
    waiting_for_rebase_lock(pool).await;
    assert!(!pruning.is_finished(), "the batch waits for the rebase");
    assert_eq!(remaining(pool, Some(organization)).await, old);
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(events::REBASE_LOCK)
        .execute(&mut *rebase)
        .await
        .expect("the rebase ends");
    drop(rebase);

    assert_eq!(
        pruning
            .await
            .expect("the batch task finishes")
            .expect("the batch succeeds"),
        2
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_prune_never_deletes_a_row_a_destination_created_after_it_starts_below() {
    let database = database("audit_events_prune_horizon").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let before = Utc::now() - Duration::days(1);
    let mut open = pool.begin().await.expect("a transaction begins");
    sqlx::query("SELECT pg_current_xact_id()")
        .execute(&mut *open)
        .await
        .expect("the open transaction holds an xid");
    let row = write(pool, Some(organization)).await;
    age(pool, &[row], Utc::now() - Duration::days(10)).await;

    assert_eq!(
        events::prune(pool, scope, before, 100)
            .await
            .expect("the prune succeeds"),
        0,
        "a row above the horizon is not final, so it is not pruned"
    );
    let start = events::start(pool).await.expect("the start is readable");
    assert!(position(pool, row).await > start);
    let ours = Destination::new(Some(organization))
        .create(pool, start)
        .await;
    open.commit().await.expect("the open transaction ends");
    settle(pool).await;

    assert_eq!(
        events::prune(pool, scope, before, 100)
            .await
            .expect("the prune succeeds"),
        0,
        "the new destination holds the row it starts below"
    );
    assert_eq!(pending(pool, scope, ours).await, vec![row]);
    database.cleanup().await;
}

#[tokio::test]
async fn counts_stop_at_the_cap_and_say_there_are_more() {
    let database = database("audit_events_cap").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    let scope = Scope::Organization(organization);
    let rows = usize::try_from(events::COUNT_CAP + 1).expect("the cap fits");
    sqlx::query(
        "INSERT INTO audit_logs (organization_id, action, resource_type, created_at) \
         SELECT $1, $2, $3, now() - interval '10 days' FROM generate_series(1, $4)",
    )
    .bind(organization)
    .bind(actions::SETTINGS_UPDATED)
    .bind(resources::ORGANIZATION)
    .bind(i64::try_from(rows).expect("the cap fits"))
    .execute(pool)
    .await
    .expect("the rows are written");
    settle(pool).await;
    let before = Utc::now() - Duration::days(1);

    let backlog = events::backlog(pool, scope, ORIGIN)
        .await
        .expect("the backlog is readable");
    assert_eq!((backlog.count, backlog.capped), (events::COUNT_CAP, true));
    assert!(backlog.oldest.is_some());
    assert_eq!(
        events::expired(pool, scope, before, Some(ORIGIN))
            .await
            .expect("expired is readable"),
        Expired {
            total: events::COUNT_CAP,
            total_capped: true,
            held: events::COUNT_CAP,
            held_capped: true,
        }
    );

    sqlx::query(
        "DELETE FROM audit_logs WHERE id = (SELECT id FROM audit_logs \
         WHERE organization_id = $1 ORDER BY transaction_id, sequence LIMIT 1)",
    )
    .bind(organization)
    .execute(pool)
    .await
    .expect("one row is deleted");
    let backlog = events::backlog(pool, scope, ORIGIN)
        .await
        .expect("the backlog is readable");
    assert_eq!((backlog.count, backlog.capped), (events::COUNT_CAP, false));
    assert_eq!(
        events::expired(pool, scope, before, Some(ORIGIN))
            .await
            .expect("expired is readable"),
        counted(events::COUNT_CAP, events::COUNT_CAP)
    );
    database.cleanup().await;
}

/// A restored organization log of [`SEEDED`] rows behind a destination with a restored cursor.
async fn restored_large_log(pool: &PgPool) {
    let organization = organization(pool).await;
    let old = written(pool, Some(organization), 2).await;
    settle(pool).await;
    let delivered = position(pool, old[0]).await;
    restore(pool, &old).await;
    Destination::new(Some(organization))
        .create(pool, restored(delivered))
        .await;
    seed(pool, Some(organization), SEEDED).await;
    analyze(pool).await;
}

#[tokio::test]
async fn finding_shared_sequences_reads_a_large_log_in_one_pass() {
    let database = database("audit_events_collided_large").await;
    let pool = &database.pool;
    restored_large_log(pool).await;
    let horizon: i64 =
        sqlx::query_scalar("SELECT ((pg_snapshot_xmax(pg_current_snapshot()))::text)::bigint")
            .fetch_one(pool)
            .await
            .expect("the horizon is readable");

    let scanning = scanning(pool).await;
    let started = Instant::now();
    events::collided(&scanning, horizon)
        .await
        .expect("shared sequences are readable");
    let elapsed = started.elapsed();
    scanning.close().await;

    assert!(
        elapsed < SEEDED_COLLIDED,
        "finding shared sequences over {SEEDED} rows took {elapsed:?}; it must not compare every pair of rows"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_rebase_over_a_large_log_moves_every_row() {
    let database = database("audit_events_rebase_large").await;
    let pool = &database.pool;
    restored_large_log(pool).await;

    let scanning = scanning(pool).await;
    assert!(rebase(&scanning).await, "the restored rows are rebased");
    scanning.close().await;

    let (unmoved, shared): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE transaction_id <> 0), count(*) - count(DISTINCT sequence) \
         FROM audit_logs",
    )
    .fetch_one(pool)
    .await
    .expect("the audit rows are readable");
    assert_eq!(unmoved, 0, "every row is rebased");
    assert_eq!(shared, 0, "no row shares its sequence");
    database.cleanup().await;
}

#[tokio::test]
async fn server_reads_take_the_next_page_from_the_server_position_index() {
    let database = database("audit_events_server_index").await;
    let pool = &database.pool;
    let organization = organization(pool).await;
    seed(pool, Some(organization), 4 * SERVER_SEEDED).await;
    seed(pool, None, SERVER_SEEDED).await;
    analyze(pool).await;
    settle(pool).await;

    let mut transaction = pool.begin().await.expect("a transaction begins");
    for _ in 0..8 {
        let rows = events::after(&mut *transaction, Scope::Server, ORIGIN, None, SERVER_PAGE)
            .await
            .expect("the final rows are readable");
        assert_eq!(rows.len(), usize::try_from(SERVER_PAGE).unwrap());
    }
    let read: i64 = sqlx::query_scalar(
        "SELECT seq_tup_read + COALESCE(idx_tup_fetch, 0) FROM pg_stat_xact_user_tables \
         WHERE relname = 'audit_logs'",
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("the transaction's table statistics are readable");
    transaction.rollback().await.expect("the transaction ends");

    assert!(
        read < 8 * SERVER_PAGE_READS,
        "eight pages of {SERVER_PAGE} server rows read {read} rows of a log with {SERVER_SEEDED} server rows behind {} organization rows",
        4 * SERVER_SEEDED
    );
    database.cleanup().await;
}
