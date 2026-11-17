use std::collections::BTreeSet;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use tokio::task::JoinSet;
use uuid::Uuid;
use zone_server::db::audit::deliveries::{self, Delivery, Holding};
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
        let owner = Uuid::new_v4();
        contenders.spawn(async move {
            let claimed = deliveries::claim(&pool, owner, LEASE).await.unwrap();
            (owner, claimed)
        });
    }
    let mut claimed = Vec::new();
    while let Some(result) = contenders.join_next().await {
        let (owner, delivery) = result.unwrap();
        if let Some(delivery) = delivery {
            assert_eq!(delivery.lease_owner, Some(owner));
            claimed.push((owner, delivery.destination_id));
        }
    }

    let distinct: BTreeSet<Uuid> = claimed.iter().map(|(_, id)| *id).collect();
    assert_eq!(
        distinct.len(),
        claimed.len(),
        "two claims took one destination"
    );
    assert_eq!(distinct, created, "every destination was claimed once");
    for (owner, id) in &claimed {
        let delivery = fixture.delivery(*id).await;
        assert_eq!(delivery.lease_owner, Some(*owner));
        assert!(delivery.lease_expires_at.unwrap() > Utc::now() + TimeDelta::seconds(60));
    }
    assert_eq!(
        deliveries::claim(fixture.pool(), Uuid::new_v4(), LEASE)
            .await
            .unwrap(),
        None,
        "a leased delivery is not claimed again"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_expired_lease_can_be_claimed_again() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();

    let claimed = deliveries::claim(fixture.pool(), first, LEASE)
        .await
        .unwrap()
        .expect("the due delivery is claimed");
    assert_eq!(claimed.destination_id, id);
    assert_eq!(
        deliveries::claim(fixture.pool(), second, LEASE)
            .await
            .unwrap(),
        None
    );

    fixture.expire(id).await;
    let reclaimed = deliveries::claim(fixture.pool(), second, LEASE)
        .await
        .unwrap()
        .expect("a lapsed lease is claimed again");
    assert_eq!(reclaimed.destination_id, id);
    assert_eq!(reclaimed.lease_owner, Some(second));
    assert!(
        !deliveries::advance(fixture.pool(), id, first, position(9, 9), 1)
            .await
            .unwrap(),
        "the first owner lost its lease"
    );
    assert_eq!(fixture.delivery(id).await.delivered, ORIGIN);
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_the_lease_owner_pins_advances_fails_or_releases() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let owner = Uuid::new_v4();
    let stranger = Uuid::new_v4();
    let through = position(7, 42);

    deliveries::claim(fixture.pool(), owner, LEASE)
        .await
        .unwrap()
        .expect("the due delivery is claimed");
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
    assert!(
        !deliveries::fail(
            fixture.pool(),
            id,
            stranger,
            "status",
            "HTTP 503",
            later(60)
        )
        .await
        .unwrap()
    );
    assert!(
        !deliveries::release(fixture.pool(), id, stranger, later(60))
            .await
            .unwrap()
    );
    assert_eq!(fixture.delivery(id).await, before, "a stranger changed it");

    assert!(
        deliveries::pin(fixture.pool(), id, owner, through)
            .await
            .unwrap()
    );
    assert_eq!(fixture.delivery(id).await.batch, Some(through));

    let retry_at = later(300);
    assert!(
        deliveries::fail(fixture.pool(), id, owner, "status", "HTTP 503", retry_at)
            .await
            .unwrap()
    );
    let failed = fixture.delivery(id).await;
    assert_eq!(failed.attempts, 1);
    assert_eq!(failed.batch, Some(through), "the pin survives a failure");
    assert_eq!(failed.delivered, ORIGIN);
    assert_eq!(failed.next_attempt_at, retry_at);
    assert_eq!(failed.last_error_code.as_deref(), Some("status"));
    assert_eq!(failed.last_error.as_deref(), Some("HTTP 503"));
    assert!(failed.last_failure_at.is_some());
    assert_eq!(failed.lease_owner, None);
    assert_eq!(failed.lease_expires_at, None);
    assert!(
        !deliveries::advance(fixture.pool(), id, owner, through, 3)
            .await
            .unwrap(),
        "a failure returns the lease"
    );
    assert_eq!(
        deliveries::claim(fixture.pool(), owner, LEASE)
            .await
            .unwrap(),
        None,
        "a failed delivery waits for its retry"
    );

    fixture.due(id).await;
    deliveries::claim(fixture.pool(), owner, LEASE)
        .await
        .unwrap()
        .expect("the retry is claimed once due");
    assert!(
        deliveries::advance(fixture.pool(), id, owner, through, 3)
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
    assert_eq!(
        advanced.last_error_code.as_deref(),
        Some("status"),
        "the last error stays on record"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_release_returns_the_lease_until_the_next_attempt() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let owner = Uuid::new_v4();
    deliveries::claim(fixture.pool(), owner, LEASE)
        .await
        .unwrap()
        .expect("the due delivery is claimed");

    let next = later(60);
    assert!(
        deliveries::release(fixture.pool(), id, owner, next)
            .await
            .unwrap()
    );
    let released = fixture.delivery(id).await;
    assert_eq!(released.next_attempt_at, next);
    assert_eq!(released.lease_owner, None);
    assert_eq!(released.attempts, 0);
    assert_eq!(
        deliveries::claim(fixture.pool(), owner, LEASE)
            .await
            .unwrap(),
        None
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

    assert_eq!(
        deliveries::claim(fixture.pool(), Uuid::new_v4(), LEASE)
            .await
            .unwrap(),
        None
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reset_drops_the_pin_and_failures_and_makes_the_delivery_due() {
    let fixture = Fixture::new().await;
    let id = fixture.destination(Scope::Server, true, ORIGIN).await;
    settle(fixture.pool()).await;
    let owner = Uuid::new_v4();
    deliveries::claim(fixture.pool(), owner, LEASE)
        .await
        .unwrap()
        .expect("the due delivery is claimed");
    deliveries::pin(fixture.pool(), id, owner, position(3, 3))
        .await
        .unwrap();
    deliveries::fail(fixture.pool(), id, owner, "connect", "refused", later(3600))
        .await
        .unwrap();

    deliveries::reset(fixture.pool(), id).await.unwrap();
    let reset = fixture.delivery(id).await;
    assert_eq!(reset.batch, None);
    assert_eq!(reset.attempts, 0);
    assert_eq!(reset.delivered, ORIGIN);
    assert!(reset.next_attempt_at <= Utc::now());
    assert!(
        deliveries::claim(fixture.pool(), owner, LEASE)
            .await
            .unwrap()
            .is_some(),
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
