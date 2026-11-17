use std::collections::BTreeSet;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use tokio::task::JoinSet;
use uuid::Uuid;
use zone_enterprise::audit::FailureCode;
use zone_server::db::audit::deliveries::{self, Delivery, Holding, Lease, Resume};
use zone_server::db::audit::events;
use zone_server::db::audit::position::Position;
use zone_server::db::audit::scope::Scope;

use crate::common::Database;
use crate::common::audit::{database, settle};

const PREFIX: &str = "audit_delivery_store";
const LEASE: Duration = Duration::from_secs(120);
const ORIGIN: Position = Position {
    transaction_id: 0,
    sequence: 0,
};
const CONTENDERS: usize = 16;
const DESTINATIONS: usize = 5;

struct Fixture {
    database: Database,
}

impl Fixture {
    async fn new() -> Self {
        Self {
            database: database(PREFIX).await,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn organization(&self) -> Uuid {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Audit', $1::text)")
            .bind(organization)
            .execute(self.pool())
            .await
            .unwrap();
        organization
    }

    async fn destination(&self, scope: Scope, enabled: bool, delivered: Position) -> Uuid {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO audit_destinations (organization_id, kind, name, enabled, settings, secrets) \
             VALUES ($1, 'webhook', $2, $3, '{}'::jsonb, 'sealed') RETURNING id",
        )
        .bind(scope.organization_id())
        .bind(Uuid::new_v4().to_string())
        .bind(enabled)
        .fetch_one(self.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO audit_deliveries (destination_id, transaction_id, sequence) \
             VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(delivered.transaction_id)
        .bind(delivered.sequence)
        .execute(self.pool())
        .await
        .unwrap();
        id
    }

    async fn name(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT name FROM audit_destinations WHERE id = $1")
            .bind(id)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn event(&self, scope: Scope, created_at: DateTime<Utc>) -> Position {
        let (transaction_id, sequence): (i64, i64) = sqlx::query_as(
            "INSERT INTO audit_logs (organization_id, action, resource_type, created_at) \
             VALUES ($1, 'user.login', 'user', $2) RETURNING transaction_id, sequence",
        )
        .bind(scope.organization_id())
        .bind(created_at)
        .fetch_one(self.pool())
        .await
        .unwrap();
        Position {
            transaction_id,
            sequence,
        }
    }

    async fn due(&self, id: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET next_attempt_at = now() - interval '1 second' \
             WHERE destination_id = $1",
        )
        .bind(id)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn expire(&self, id: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET lease_expires_at = now() - interval '1 second' \
             WHERE destination_id = $1",
        )
        .bind(id)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn claim(&self) -> Option<Lease> {
        deliveries::claim(self.pool(), LEASE).await.unwrap()
    }

    async fn clock(&self) -> DateTime<Utc> {
        sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn delivery(&self, id: Uuid) -> Delivery {
        deliveries::get(self.pool(), id)
            .await
            .unwrap()
            .expect("the delivery exists")
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn later(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(Utc::now().timestamp() + seconds, 0).unwrap()
}

fn shifted(at: DateTime<Utc>, delay: Duration) -> DateTime<Utc> {
    at + TimeDelta::from_std(delay).unwrap()
}

fn position(transaction_id: i64, sequence: i64) -> Position {
    Position {
        transaction_id,
        sequence,
    }
}

#[tokio::test]
async fn concurrent_claims_never_share_a_destination() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let mut created = BTreeSet::new();
    for _ in 0..DESTINATIONS {
        created.insert(
            fixture
                .destination(Scope::Organization(organization), true, ORIGIN)
                .await,
        );
    }
    settle(fixture.pool()).await;

    let mut contenders = JoinSet::new();
    for _ in 0..CONTENDERS {
        let pool = fixture.pool().clone();
        contenders.spawn(async move { deliveries::claim(&pool, LEASE).await.unwrap() });
    }
    let mut claimed = Vec::new();
    while let Some(result) = contenders.join_next().await {
        if let Some(lease) = result.unwrap() {
            assert_eq!(lease.delivery.lease_owner, Some(lease.token));
            claimed.push((lease.token, lease.delivery.destination_id));
        }
    }

    let distinct: BTreeSet<Uuid> = claimed.iter().map(|(_, id)| *id).collect();
    assert_eq!(
        distinct.len(),
        claimed.len(),
        "two claims took one destination"
    );
    assert_eq!(distinct, created, "every destination was claimed once");
    let tokens: BTreeSet<Uuid> = claimed.iter().map(|(token, _)| *token).collect();
    assert_eq!(tokens.len(), claimed.len(), "every claim has its own token");
    for (token, id) in &claimed {
        let delivery = fixture.delivery(*id).await;
        assert_eq!(delivery.lease_owner, Some(*token));
        assert!(
            delivery.lease_expires_at.unwrap() > fixture.clock().await + TimeDelta::seconds(60)
        );
    }
    assert_eq!(
        fixture.claim().await,
        None,
        "a leased delivery is not claimed again"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reclaim_by_the_same_caller_refuses_every_write_under_the_stale_token() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;

    let stale = fixture.claim().await.expect("the due delivery is claimed");
    assert_eq!(stale.delivery.destination_id, id);
    assert_eq!(fixture.claim().await, None);

    fixture.expire(id).await;
    let reclaimed = fixture
        .claim()
        .await
        .expect("a lapsed lease is claimed again");
    assert_eq!(reclaimed.delivery.destination_id, id);
    assert_ne!(
        reclaimed.token, stale.token,
        "a reclaim takes a fresh token"
    );
    assert_eq!(reclaimed.delivery.lease_owner, Some(reclaimed.token));
    let held = fixture.delivery(id).await;

    assert!(
        !deliveries::pin(fixture.pool(), id, stale.token, position(9, 9))
            .await
            .unwrap(),
        "the stale task can't pin"
    );
    assert!(
        !deliveries::advance(fixture.pool(), id, stale.token, position(9, 9), 1)
            .await
            .unwrap(),
        "the stale task can't advance"
    );
    assert_eq!(
        deliveries::fail(
            fixture.pool(),
            id,
            stale.token,
            FailureCode::Timeout,
            "abandoned",
            Duration::from_secs(60)
        )
        .await
        .unwrap(),
        None,
        "the stale task can't fail"
    );
    assert!(
        !deliveries::release(
            fixture.pool(),
            id,
            stale.token,
            Resume::After(Duration::from_secs(60))
        )
        .await
        .unwrap(),
        "the stale task can't release"
    );
    assert_eq!(
        fixture.delivery(id).await,
        held,
        "the live lease is untouched"
    );
    assert!(
        deliveries::advance(fixture.pool(), id, reclaimed.token, position(9, 9), 1)
            .await
            .unwrap()
    );
    assert_eq!(fixture.delivery(id).await.delivered, position(9, 9));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lease_dropped_by_a_rebase_refuses_the_stale_token_after_a_new_claim() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let stale = fixture.claim().await.expect("the due delivery is claimed");
    sqlx::query("UPDATE audit_deliveries SET transaction_id = $2 WHERE destination_id = $1")
        .bind(id)
        .bind(i64::from(i32::MAX))
        .execute(fixture.pool())
        .await
        .unwrap();

    assert!(
        events::rebase(fixture.pool()).await.unwrap(),
        "the restore is rebased"
    );
    let rebased = fixture.delivery(id).await;
    assert_eq!(rebased.lease_owner, None, "the rebase drops the lease");
    let reclaimed = fixture
        .claim()
        .await
        .expect("the rebased delivery is claimed");

    assert!(
        !deliveries::advance(
            fixture.pool(),
            id,
            stale.token,
            position(i64::from(i32::MAX), 1),
            1
        )
        .await
        .unwrap(),
        "a pre-rebase advance is refused"
    );
    let delivery = fixture.delivery(id).await;
    assert_eq!(delivery.delivered, rebased.delivered);
    assert_eq!(delivery.lease_owner, Some(reclaimed.token));
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_the_lease_holder_pins_advances_fails_or_releases() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let stranger = Uuid::new_v4();
    let through = position(7, 42);
    let retry = Duration::from_secs(300);

    let lease = fixture.claim().await.expect("the due delivery is claimed");
    let before = fixture.delivery(id).await;
    assert!(
        !deliveries::pin(fixture.pool(), id, stranger, through)
            .await
            .unwrap()
    );
    assert!(
        !deliveries::advance(fixture.pool(), id, stranger, through, 3)
            .await
            .unwrap()
    );
    assert_eq!(
        deliveries::fail(
            fixture.pool(),
            id,
            stranger,
            FailureCode::Status,
            "HTTP 503",
            retry
        )
        .await
        .unwrap(),
        None
    );
    assert!(
        !deliveries::release(fixture.pool(), id, stranger, Resume::After(retry))
            .await
            .unwrap()
    );
    assert_eq!(fixture.delivery(id).await, before, "a stranger changed it");

    assert!(
        deliveries::pin(fixture.pool(), id, lease.token, through)
            .await
            .unwrap()
    );
    assert_eq!(fixture.delivery(id).await.batch, Some(through));

    let earliest = shifted(fixture.clock().await, retry);
    let retry_at = deliveries::fail(
        fixture.pool(),
        id,
        lease.token,
        FailureCode::Status,
        "HTTP 503",
        retry,
    )
    .await
    .unwrap()
    .expect("the holder records its failure");
    let latest = shifted(fixture.clock().await, retry);
    assert!(
        retry_at >= earliest && retry_at <= latest,
        "the retry is due {retry:?} after the failure on the database's clock, not {retry_at}"
    );
    let failed = fixture.delivery(id).await;
    assert_eq!(failed.attempts, 1);
    assert_eq!(failed.batch, Some(through), "the pin survives a failure");
    assert_eq!(failed.delivered, ORIGIN);
    assert_eq!(failed.next_attempt_at, retry_at);
    assert_eq!(failed.last_error_code, Some(FailureCode::Status));
    assert_eq!(failed.last_error.as_deref(), Some("HTTP 503"));
    assert!(failed.last_failure_at.is_some());
    assert_eq!(failed.lease_owner, None);
    assert_eq!(failed.lease_expires_at, None);
    assert!(
        !deliveries::advance(fixture.pool(), id, lease.token, through, 3)
            .await
            .unwrap(),
        "a failure returns the lease"
    );
    assert_eq!(
        fixture.claim().await,
        None,
        "a failed delivery waits for its retry"
    );

    fixture.due(id).await;
    let retried = fixture
        .claim()
        .await
        .expect("the retry is claimed once due");
    let claimed = fixture.clock().await;
    assert!(
        deliveries::advance(fixture.pool(), id, retried.token, through, 3)
            .await
            .unwrap()
    );
    let advanced = fixture.delivery(id).await;
    assert_eq!(advanced.delivered, through);
    assert_eq!(advanced.batch, None);
    assert_eq!(advanced.attempts, 0);
    assert_eq!(advanced.delivered_count, 3);
    assert!(advanced.last_success_at.is_some());
    assert_eq!(advanced.lease_owner, None);
    assert_eq!(advanced.lease_expires_at, None);
    assert!(
        advanced.next_attempt_at >= claimed && advanced.next_attempt_at <= fixture.clock().await,
        "an advanced delivery is due again from the moment it advanced"
    );
    assert_eq!(
        advanced.last_error_code,
        Some(FailureCode::Status),
        "the last error stays on record"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_advance_queues_the_delivery_behind_every_other_due_one() {
    let fixture = Fixture::new().await;
    let first = fixture.destination(Scope::Server, true, ORIGIN).await;
    let second = fixture.destination(Scope::Server, true, ORIGIN).await;
    sqlx::query(
        "UPDATE audit_deliveries SET next_attempt_at = now() - interval '1 hour' \
         WHERE destination_id = $1",
    )
    .bind(first)
    .execute(fixture.pool())
    .await
    .unwrap();
    fixture.due(second).await;
    settle(fixture.pool()).await;

    let lease = fixture
        .claim()
        .await
        .expect("the earliest due delivery is claimed");
    assert_eq!(lease.delivery.destination_id, first);
    assert!(
        deliveries::advance(fixture.pool(), first, lease.token, position(1, 1), 1)
            .await
            .unwrap()
    );
    let next = fixture
        .claim()
        .await
        .expect("another due delivery is claimed");
    assert_eq!(
        next.delivery.destination_id, second,
        "the advanced delivery waits its turn"
    );
    let after = fixture
        .claim()
        .await
        .expect("the advanced delivery is still due");
    assert_eq!(after.delivery.destination_id, first);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_release_returns_the_lease_until_the_next_attempt() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let lease = fixture.claim().await.expect("the due delivery is claimed");

    let next = later(60);
    assert!(
        deliveries::release(fixture.pool(), id, lease.token, Resume::At(next))
            .await
            .unwrap()
    );
    let released = fixture.delivery(id).await;
    assert_eq!(released.next_attempt_at, next);
    assert_eq!(released.lease_owner, None);
    assert_eq!(released.attempts, 0);
    assert_eq!(fixture.claim().await, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_release_after_a_delay_is_timed_on_the_database_clock() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let delay = Duration::from_millis(5_250);
    let lease = fixture.claim().await.expect("the due delivery is claimed");

    let earliest = shifted(fixture.clock().await, delay);
    assert!(
        deliveries::release(fixture.pool(), id, lease.token, Resume::After(delay))
            .await
            .unwrap()
    );
    let latest = shifted(fixture.clock().await, delay);
    let released = fixture.delivery(id).await;
    assert!(
        released.next_attempt_at >= earliest && released.next_attempt_at <= latest,
        "due between {earliest} and {latest}, not {}",
        released.next_attempt_at
    );
    assert_eq!(fixture.claim().await, None, "not due before its delay");
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_claim_reports_the_database_clock() {
    let fixture = Fixture::new().await;
    fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let before = fixture.clock().await;
    let lease = fixture.claim().await.expect("the due delivery is claimed");
    let after = fixture.clock().await;
    assert!(lease.claimed_at >= before && lease.claimed_at <= after);
    assert_eq!(
        lease.delivery.lease_expires_at,
        Some(shifted(lease.claimed_at, LEASE)),
        "the lease runs from the claim"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn claims_skip_disabled_destinations_and_deliveries_not_yet_due() {
    let fixture = Fixture::new().await;
    fixture.destination(Scope::Server, false, ORIGIN).await;
    let waiting = fixture.destination(Scope::Server, true, ORIGIN).await;
    sqlx::query(
        "UPDATE audit_deliveries SET next_attempt_at = now() + interval '1 hour' \
         WHERE destination_id = $1",
    )
    .bind(waiting)
    .execute(fixture.pool())
    .await
    .unwrap();
    settle(fixture.pool()).await;

    assert_eq!(fixture.claim().await, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reset_drops_the_pin_and_failures_and_makes_the_delivery_due() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let lease = fixture.claim().await.expect("the due delivery is claimed");
    deliveries::pin(fixture.pool(), id, lease.token, position(3, 3))
        .await
        .unwrap();
    deliveries::fail(
        fixture.pool(),
        id,
        lease.token,
        FailureCode::Connect,
        "refused",
        Duration::from_secs(3600),
    )
    .await
    .unwrap();

    deliveries::reset(fixture.pool(), id).await.unwrap();
    let reset = fixture.delivery(id).await;
    assert_eq!(reset.batch, None);
    assert_eq!(reset.attempts, 0);
    assert_eq!(reset.delivered, ORIGIN);
    assert!(reset.next_attempt_at <= fixture.clock().await);
    assert!(
        fixture.claim().await.is_some(),
        "a reset delivery is due at once"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn deliveries_are_listed_and_read_within_their_scope() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let other = fixture.organization().await;
    let first = fixture
        .destination(Scope::Organization(organization), true, ORIGIN)
        .await;
    let second = fixture
        .destination(Scope::Organization(organization), false, position(4, 4))
        .await;
    let elsewhere = fixture
        .destination(Scope::Organization(other), true, ORIGIN)
        .await;
    let server = fixture.destination(Scope::Server, true, ORIGIN).await;

    let listed: BTreeSet<Uuid> =
        deliveries::list(fixture.pool(), Scope::Organization(organization))
            .await
            .unwrap()
            .into_iter()
            .map(|delivery| delivery.destination_id)
            .collect();
    assert_eq!(listed, BTreeSet::from([first, second]));
    let servers: Vec<Uuid> = deliveries::list(fixture.pool(), Scope::Server)
        .await
        .unwrap()
        .into_iter()
        .map(|delivery| delivery.destination_id)
        .collect();
    assert_eq!(servers, vec![server]);
    assert_eq!(fixture.delivery(second).await.delivered, position(4, 4));
    assert_eq!(fixture.delivery(elsewhere).await.destination_id, elsewhere);
    assert_eq!(
        deliveries::get(fixture.pool(), Uuid::new_v4())
            .await
            .unwrap(),
        None
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_floor_is_the_lowest_position_over_enabled_destinations_in_scope() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let other = fixture.organization().await;
    let empty = fixture.organization().await;
    let disabled_only = fixture.organization().await;
    let scope = Scope::Organization(organization);
    fixture.destination(scope, true, position(5, 10)).await;
    fixture.destination(scope, true, position(3, 20)).await;
    fixture.destination(scope, true, position(3, 30)).await;
    fixture.destination(scope, false, position(1, 1)).await;
    fixture
        .destination(Scope::Organization(other), true, position(0, 0))
        .await;
    fixture
        .destination(Scope::Organization(disabled_only), false, position(2, 2))
        .await;
    fixture
        .destination(Scope::Server, true, position(2, 2))
        .await;
    fixture
        .destination(Scope::Server, false, position(1, 1))
        .await;
    settle(fixture.pool()).await;

    assert_eq!(
        deliveries::floor(fixture.pool(), scope).await.unwrap(),
        Some(position(3, 20))
    );
    assert_eq!(
        deliveries::floor(fixture.pool(), Scope::Server)
            .await
            .unwrap(),
        Some(position(2, 2))
    );
    assert_eq!(
        deliveries::floor(fixture.pool(), Scope::Organization(empty))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        deliveries::floor(fixture.pool(), Scope::Organization(disabled_only))
            .await
            .unwrap(),
        None,
        "a disabled destination holds nothing back"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn holding_names_the_enabled_destinations_behind_an_expired_row() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let other = fixture.organization().await;
    let scope = Scope::Organization(organization);
    let old = Utc::now() - TimeDelta::days(10);
    let cutoff = Utc::now() - TimeDelta::days(1);

    let first_expired = fixture.event(scope, old).await;
    let last_expired = fixture.event(scope, old).await;
    let fresh = fixture.event(scope, Utc::now()).await;
    fixture.event(Scope::Server, old).await;

    let behind = fixture.destination(scope, true, ORIGIN).await;
    sqlx::query("UPDATE audit_deliveries SET attempts = 3 WHERE destination_id = $1")
        .bind(behind)
        .execute(fixture.pool())
        .await
        .unwrap();
    let partway = fixture.destination(scope, true, first_expired).await;
    fixture.destination(scope, true, last_expired).await;
    fixture.destination(scope, true, fresh).await;
    fixture.destination(scope, false, ORIGIN).await;
    fixture
        .destination(Scope::Organization(other), true, ORIGIN)
        .await;
    let server = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;

    let mut held = deliveries::holding(fixture.pool(), scope, cutoff)
        .await
        .unwrap();
    held.sort_by_key(|holding| holding.destination_id);
    let mut expected = vec![
        Holding {
            destination_id: behind,
            name: fixture.name(behind).await,
            attempts: 3,
        },
        Holding {
            destination_id: partway,
            name: fixture.name(partway).await,
            attempts: 0,
        },
    ];
    expected.sort_by_key(|holding| holding.destination_id);
    assert_eq!(held, expected);

    assert_eq!(
        deliveries::holding(fixture.pool(), Scope::Server, cutoff)
            .await
            .unwrap(),
        vec![Holding {
            destination_id: server,
            name: fixture.name(server).await,
            attempts: 0,
        }]
    );
    assert_eq!(
        deliveries::holding(fixture.pool(), Scope::Organization(other), cutoff)
            .await
            .unwrap(),
        Vec::new(),
        "a scope without rows holds nothing"
    );
    assert_eq!(
        deliveries::holding(fixture.pool(), scope, old - TimeDelta::days(1))
            .await
            .unwrap(),
        Vec::new(),
        "nothing has expired before the oldest row"
    );
    fixture.cleanup().await;
}
