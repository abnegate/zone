use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use futures::future::join_all;
use sqlx::{Executor, PgPool};
use tokio::sync::Notify;
use uuid::Uuid;
use zone_enterprise::Tier;
use zone_enterprise::audit::delivery::{RETRY_BASE, S3_BATCH, S3_FLUSH_AGE};
use zone_enterprise::audit::{FailureCode, Kind, Pause};
use zone_server::db::audit::deliveries::{self, Delivery};
use zone_server::db::audit::destinations::DestinationRecord;
use zone_server::db::audit::events;
use zone_server::db::audit::position::Position;
use zone_server::db::audit::scope::Scope;
use zone_server::db::audit::{actions, resources};
use zone_server::db::licenses;
use zone_server::services::audit::delivery::{Cadence, Open, Outcome, deliver};
use zone_server::services::audit::transport::{Batch, Failure, Transport};
use zone_server::state::AppState;
use zone_server::workers::audit::drain;

use crate::common::audit::{
    Destination, UNVERIFIABLE, created_at, database, organization, position, settle, write,
};
use crate::common::sso::{active, keyring, lapsed};
use crate::common::{Database, test_config};

const PREFIX: &str = "audit_delivery";
const CADENCE: Cadence = Cadence {
    tick: Duration::from_millis(50),
    lease: Duration::from_secs(30),
    concurrency: 4,
};
const RESTORED: i64 = 1 << 40;
const UNAVAILABLE: &str = "HTTP 503";
const JITTER: f64 = 0.75;

#[derive(Default)]
struct Receiver {
    batches: Mutex<Vec<Batch>>,
    failures: Mutex<VecDeque<Failure>>,
    sending: Mutex<HashSet<Uuid>>,
    overlaps: AtomicUsize,
    hang: AtomicBool,
    hung: Notify,
    resume: Notify,
    pause: AtomicU64,
}

impl Receiver {
    fn fail(&self, failure: Failure) {
        self.failures.lock().unwrap().push_back(failure);
    }

    fn sent(&self, destination: Uuid) -> Vec<Vec<Uuid>> {
        self.batches
            .lock()
            .unwrap()
            .iter()
            .filter(|batch| batch.destination_id == destination)
            .map(|batch| batch.events.iter().map(|event| event.id).collect())
            .collect()
    }

    fn received(&self, destination: Uuid) -> Vec<Uuid> {
        self.sent(destination).concat()
    }

    fn order(&self) -> Vec<Uuid> {
        self.batches
            .lock()
            .unwrap()
            .iter()
            .map(|batch| batch.destination_id)
            .collect()
    }
}

struct Sending(Arc<Receiver>);

#[async_trait]
impl Transport for Sending {
    async fn deliver(&self, batch: &Batch) -> Result<(), Failure> {
        let receiver = &self.0;
        if !receiver
            .sending
            .lock()
            .unwrap()
            .insert(batch.destination_id)
        {
            receiver.overlaps.fetch_add(1, Ordering::SeqCst);
        }
        receiver.batches.lock().unwrap().push(batch.clone());
        if receiver.hang.load(Ordering::SeqCst) {
            receiver.hung.notify_one();
            receiver.resume.notified().await;
        }
        tokio::time::sleep(Duration::from_millis(receiver.pause.load(Ordering::SeqCst))).await;
        receiver
            .sending
            .lock()
            .unwrap()
            .remove(&batch.destination_id);
        match receiver.failures.lock().unwrap().pop_front() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

struct Fake(Arc<Receiver>);

#[async_trait]
impl Open for Fake {
    async fn open(&self, _destination: &DestinationRecord) -> Result<Box<dyn Transport>, Failure> {
        Ok(Box::new(Sending(self.0.clone())))
    }
}

struct Engine {
    database: Database,
    state: AppState,
    receiver: Arc<Receiver>,
    open: Fake,
}

impl Engine {
    async fn new() -> Self {
        let database = database(PREFIX).await;
        let state = AppState::new(test_config(), database.pool.clone(), None);
        state.disable_mcp();
        state.install_keyring(keyring());
        let receiver = Arc::new(Receiver::default());
        Self {
            database,
            state,
            open: Fake(receiver.clone()),
            receiver,
        }
    }

    async fn licensed(tier: Tier) -> Self {
        let engine = Self::new().await;
        engine.license(&active(tier)).await;
        engine
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn license(&self, key: &str) {
        licenses::replace(self.pool(), key).await.unwrap();
        self.state.licenses().load(self.pool()).await.unwrap();
    }

    async fn organization(&self) -> Scope {
        Scope::Organization(organization(self.pool()).await)
    }

    async fn destination(&self, scope: Scope, kind: Kind) -> Uuid {
        let start = events::start(self.pool()).await.unwrap();
        Destination {
            kind,
            ..Destination::new(scope.organization_id())
        }
        .create(self.pool(), start)
        .await
    }

    async fn write(&self, scope: Scope) -> Uuid {
        write(self.pool(), scope.organization_id()).await
    }

    async fn written(&self, scope: Scope, count: usize) -> Vec<Uuid> {
        sqlx::query_scalar(
            "WITH written AS ( \
                 INSERT INTO audit_logs (organization_id, action, resource_type) \
                 SELECT $1, $2, $3 FROM generate_series(1, $4) \
                 RETURNING id, sequence) \
             SELECT id FROM written ORDER BY sequence",
        )
        .bind(scope.organization_id())
        .bind(actions::SETTINGS_UPDATED)
        .bind(resources::ORGANIZATION)
        .bind(i32::try_from(count).unwrap())
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    async fn position(&self, id: Uuid) -> Position {
        position(self.pool(), id).await
    }

    async fn created_at(&self, id: Uuid) -> DateTime<Utc> {
        created_at(self.pool(), id).await
    }

    async fn due(&self, destination: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET next_attempt_at = now() - interval '1 second' \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn expire(&self, destination: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET lease_expires_at = now() - interval '1 second' \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn hold(&self, destination: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET lease_expires_at = now() + interval '1 hour' \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn restore(&self) {
        self.pool()
            .execute(sqlx::AssertSqlSafe(format!(
                "UPDATE audit_logs SET transaction_id = transaction_id + {RESTORED}"
            )))
            .await
            .unwrap();
    }

    async fn clock(&self) -> DateTime<Utc> {
        sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn schedule(&self, destination: Uuid, seconds_ago: i32) {
        sqlx::query(
            "UPDATE audit_deliveries SET next_attempt_at = now() - make_interval(secs => $2) \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .bind(f64::from(seconds_ago))
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn shift(&self, destination: Uuid) {
        sqlx::query(
            "UPDATE audit_deliveries SET transaction_id = transaction_id + $2 \
             WHERE destination_id = $1",
        )
        .bind(destination)
        .bind(RESTORED)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn hung(&self) -> impl Future<Output = usize> + '_ {
        self.receiver.hang.store(true, Ordering::SeqCst);
        let mut stale = Box::pin(self.drain());
        tokio::select! {
            _ = &mut stale => panic!("a hung send never finishes its drain"),
            () = self.receiver.hung.notified() => {}
        }
        stale
    }

    async fn delivery(&self, destination: Uuid) -> Delivery {
        deliveries::get(self.pool(), destination)
            .await
            .unwrap()
            .expect("the delivery exists")
    }

    async fn drain(&self) -> usize {
        self.drain_with(CADENCE).await
    }

    async fn drain_with(&self, cadence: Cadence) -> usize {
        drain(&self.state, cadence, &self.open)
            .await
            .expect("the tick drains")
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn unavailable(retry_after: Option<Duration>) -> Failure {
    Failure {
        retry_after,
        ..Failure::new(FailureCode::Status, UNAVAILABLE)
    }
}

fn backoff(attempt: u32) -> Duration {
    RETRY_BASE * 2u32.pow(attempt)
}

#[tokio::test]
async fn a_failing_destination_is_retried_with_backoff_until_every_event_is_delivered() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let ids = engine.written(scope, 3).await;
    settle(engine.pool()).await;
    let retry_after = Duration::from_secs(60);
    let waits = [backoff(0), retry_after, backoff(2)];
    engine.receiver.fail(unavailable(None));
    engine.receiver.fail(unavailable(Some(retry_after)));
    engine.receiver.fail(unavailable(None));

    for (attempt, wait) in (1..).zip(waits) {
        let before = engine.clock().await;
        assert_eq!(engine.drain().await, 0, "attempt {attempt} failed");
        let after = engine.clock().await;
        let delivery = engine.delivery(destination).await;
        assert_eq!(delivery.attempts, attempt);
        assert_eq!(delivery.last_error_code, Some(FailureCode::Status));
        assert_eq!(delivery.last_error.as_deref(), Some(UNAVAILABLE));
        assert_eq!(
            delivery.batch,
            Some(engine.position(ids[2]).await),
            "the batch stays pinned across retries"
        );
        assert_eq!(delivery.lease_owner, None, "a failure releases the lease");
        let earliest = before + TimeDelta::from_std(wait.mul_f64(JITTER)).unwrap();
        let latest = after + TimeDelta::from_std(wait).unwrap();
        assert!(
            delivery.next_attempt_at >= earliest && delivery.next_attempt_at <= latest,
            "attempt {attempt} retries between {earliest} and {latest}, not {}",
            delivery.next_attempt_at
        );
        engine.due(destination).await;
    }

    assert_eq!(engine.drain().await, 3);
    let delivery = engine.delivery(destination).await;
    assert_eq!(delivery.attempts, 0);
    assert_eq!(delivery.batch, None);
    assert_eq!(delivery.delivered, engine.position(ids[2]).await);
    assert_eq!(delivery.delivered_count, 3);
    assert!(delivery.last_success_at.is_some());
    assert_eq!(
        delivery.last_error_code,
        Some(FailureCode::Status),
        "the last failure stays in the history"
    );
    assert_eq!(
        engine.receiver.sent(destination),
        vec![ids.clone(); 4],
        "every retry resends the identical pinned batch"
    );
    engine.cleanup().await;
}

#[tokio::test]
async fn a_new_owner_resends_the_identical_pinned_batch_after_the_lease_expires() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let first = engine.written(scope, 3).await;
    settle(engine.pool()).await;

    drop(engine.hung().await);
    engine.receiver.hang.store(false, Ordering::SeqCst);
    let stranded = engine.delivery(destination).await;
    let crashed = stranded
        .lease_owner
        .expect("the crashed send holds the lease");
    assert_eq!(stranded.batch, Some(engine.position(first[2]).await));

    let second = engine.written(scope, 2).await;
    settle(engine.pool()).await;
    engine.hold(destination).await;
    assert_eq!(
        engine.drain().await,
        0,
        "a live lease keeps every other owner away"
    );
    engine.expire(destination).await;
    assert_eq!(engine.drain().await, 5);

    assert_eq!(
        engine.receiver.sent(destination),
        vec![first.clone(), first, second.clone()],
        "the restarted delivery resends the pinned batch before anything new"
    );
    let delivery = engine.delivery(destination).await;
    let head = engine.position(second[1]).await;
    assert_eq!(delivery.delivered, head);
    assert!(
        !deliveries::advance(engine.pool(), destination, crashed, head, 0)
            .await
            .unwrap(),
        "the crashed owner can no longer advance"
    );
    engine.cleanup().await;
}

#[tokio::test]
async fn streaming_pauses_without_claiming_until_an_enterprise_license_is_renewed() {
    let engine = Engine::new().await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let ids = engine.written(scope, 2).await;
    settle(engine.pool()).await;
    let untouched = engine.delivery(destination).await;
    let standings = [
        (None, Pause::Unlicensed),
        (Some(active(Tier::Team)), Pause::Edition),
        (Some(lapsed(Tier::Enterprise)), Pause::Lapsed),
        (Some(UNVERIFIABLE.to_owned()), Pause::Unverified),
    ];

    for (key, pause) in standings {
        if let Some(key) = key {
            engine.license(&key).await;
        }
        assert_eq!(
            deliver(&engine.state, CADENCE, &engine.open).await.unwrap(),
            Outcome::Paused(pause)
        );
        assert_eq!(engine.drain().await, 0, "{pause} streams nothing");
        assert_eq!(
            engine.delivery(destination).await,
            untouched,
            "{pause} claims nothing"
        );
        let backlog = events::backlog(engine.pool(), scope, untouched.delivered)
            .await
            .unwrap();
        assert_eq!(backlog.count, 2, "{pause} keeps the backlog");
    }
    assert!(engine.receiver.received(destination).is_empty());

    engine.license(&active(Tier::Enterprise)).await;
    assert_eq!(engine.drain().await, 2, "renewal resumes from the cursor");
    assert_eq!(engine.receiver.received(destination), ids);
    engine.cleanup().await;
}

#[tokio::test]
async fn an_open_transaction_delays_its_row_but_never_lets_the_cursor_skip_it() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let first = engine.write(scope).await;
    settle(engine.pool()).await;

    let mut open = engine.pool().begin().await.unwrap();
    let held = write(&mut *open, scope.organization_id()).await;
    let later = engine.write(scope).await;
    engine.drain().await;
    let early = engine.receiver.received(destination);
    open.commit().await.unwrap();

    assert!(
        !early.contains(&held) && !early.contains(&later),
        "nothing at or after an open transaction is sent: {early:?}"
    );
    settle(engine.pool()).await;
    engine.due(destination).await;
    engine.drain().await;
    assert_eq!(
        engine.receiver.received(destination),
        vec![first, held, later],
        "the held row is delivered in order once it commits"
    );
    engine.cleanup().await;
}

#[tokio::test]
async fn s3_waits_for_a_full_batch_or_its_oldest_event_to_age() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::S3).await;
    let single = engine.write(scope).await;
    settle(engine.pool()).await;

    assert_eq!(engine.drain().await, 0, "one fresh event is not an object");
    let waiting = engine.delivery(destination).await;
    assert_eq!(waiting.batch, None, "a waiting batch is not pinned");
    assert_eq!(waiting.attempts, 0);
    assert_eq!(
        waiting.next_attempt_at,
        engine.created_at(single).await + S3_FLUSH_AGE,
        "it waits until the oldest event is due to flush"
    );

    let rest = engine.written(scope, S3_BATCH - 1).await;
    settle(engine.pool()).await;
    engine.due(destination).await;
    assert_eq!(
        engine.drain().await,
        S3_BATCH,
        "a full batch is sent at once"
    );
    assert_eq!(
        engine.receiver.sent(destination),
        vec![[vec![single], rest].concat()]
    );

    let late = engine.write(scope).await;
    settle(engine.pool()).await;
    engine.due(destination).await;
    assert_eq!(engine.drain().await, 0);
    sqlx::query(
        "UPDATE audit_logs SET created_at = now() - make_interval(secs => $2) WHERE id = $1",
    )
    .bind(late)
    .bind((S3_FLUSH_AGE + TimeDelta::seconds(1)).as_seconds_f64())
    .execute(engine.pool())
    .await
    .unwrap();
    engine.due(destination).await;
    assert_eq!(engine.drain().await, 1, "an aged event flushes alone");
    assert_eq!(engine.receiver.sent(destination).len(), 2);
    engine.cleanup().await;
}

#[tokio::test]
async fn concurrent_drains_never_send_one_destination_two_batches_at_once() {
    const DESTINATIONS: usize = 4;
    const EVENTS: usize = 250;
    const DRAINS: usize = 4;
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let mut destinations = Vec::with_capacity(DESTINATIONS);
    for _ in 0..DESTINATIONS {
        destinations.push(engine.destination(scope, Kind::Webhook).await);
    }
    let ids = engine.written(scope, EVENTS).await;
    settle(engine.pool()).await;
    engine.receiver.pause.store(5, Ordering::SeqCst);

    let delivered: usize = join_all((0..DRAINS).map(|_| engine.drain()))
        .await
        .into_iter()
        .sum();

    assert_eq!(delivered, DESTINATIONS * EVENTS);
    assert_eq!(
        engine.receiver.overlaps.load(Ordering::SeqCst),
        0,
        "two drains sent to one destination at once"
    );
    for destination in destinations {
        assert_eq!(
            engine.receiver.received(destination),
            ids,
            "each destination gets every event once, in order"
        );
    }
    engine.cleanup().await;
}

#[tokio::test]
async fn a_tick_rebases_a_restore_before_delivering() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let delivered = engine.written(scope, 3).await;
    settle(engine.pool()).await;
    assert_eq!(engine.drain().await, 3);
    let pending = engine.written(scope, 2).await;
    settle(engine.pool()).await;

    engine.restore().await;
    engine.shift(destination).await;
    let new = engine.write(scope).await;

    engine.drain().await;
    settle(engine.pool()).await;
    engine.due(destination).await;
    engine.drain().await;

    assert_eq!(
        engine.receiver.received(destination),
        [delivered, pending, vec![new]].concat(),
        "every undelivered row is sent after the rebase, and nothing delivered is sent again"
    );
    let unmoved: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE transaction_id <> 0")
            .fetch_one(engine.pool())
            .await
            .unwrap();
    assert_eq!(unmoved, 0, "the tick rebased every row");
    assert_eq!(
        engine.delivery(destination).await.delivered,
        engine.position(new).await
    );
    engine.cleanup().await;
}

#[tokio::test]
async fn a_failed_rebase_aborts_the_tick() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let ids = engine.written(scope, 1).await;
    settle(engine.pool()).await;
    let untouched = engine.delivery(destination).await;
    engine.restore().await;
    engine
        .pool()
        .execute(
            "CREATE FUNCTION refuse_rebase() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN RAISE EXCEPTION 'refused'; END $$; \
             CREATE TRIGGER refuse_rebase BEFORE UPDATE ON audit_logs \
             FOR EACH ROW EXECUTE FUNCTION refuse_rebase();",
        )
        .await
        .unwrap();

    assert!(
        drain(&engine.state, CADENCE, &engine.open).await.is_err(),
        "a rebase error aborts the tick"
    );
    assert_eq!(
        engine.delivery(destination).await,
        untouched,
        "an aborted tick claims nothing"
    );
    assert!(engine.receiver.received(destination).is_empty());

    engine
        .pool()
        .execute("DROP TRIGGER refuse_rebase ON audit_logs")
        .await
        .unwrap();
    engine.drain().await;
    settle(engine.pool()).await;
    engine.due(destination).await;
    engine.drain().await;
    assert_eq!(
        engine.receiver.received(destination),
        ids,
        "the next tick rebases and delivers"
    );
    engine.cleanup().await;
}

#[tokio::test]
async fn a_send_that_outlived_its_lease_cannot_record_over_the_same_process_reclaiming_it() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let first = engine.written(scope, 3).await;
    settle(engine.pool()).await;

    let stale = engine.hung().await;
    let second = engine.written(scope, 2).await;
    settle(engine.pool()).await;
    engine.expire(destination).await;
    let reclaimed = engine.hung().await;
    engine.receiver.hang.store(false, Ordering::SeqCst);
    let reclaim = engine.delivery(destination).await;
    engine.receiver.resume.notify_waiters();

    assert_eq!(
        stale.await,
        0,
        "the stale send's result is discarded while the reclaim still holds the lease"
    );
    let held = engine.delivery(destination).await;
    assert_eq!(held.lease_owner, reclaim.lease_owner);
    assert_eq!(
        held.delivered, reclaim.delivered,
        "the stale send moved the cursor"
    );
    assert_eq!(held.delivered_count, 0);

    assert_eq!(reclaimed.await, 5);
    let delivery = engine.delivery(destination).await;
    assert_eq!(delivery.delivered, engine.position(second[1]).await);
    assert_eq!(delivery.delivered_count, 5);
    assert_eq!(
        engine.receiver.sent(destination),
        vec![first.clone(), first, second],
        "the reclaim resends the pinned batch, then the rest"
    );
    engine.cleanup().await;
}

#[tokio::test]
async fn a_send_in_flight_across_a_rebase_cannot_move_the_cursor() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let destination = engine.destination(scope, Kind::Webhook).await;
    let ids = engine.written(scope, 3).await;
    settle(engine.pool()).await;

    let stale = engine.hung().await;
    engine.restore().await;
    engine.shift(destination).await;
    let rebased = engine.hung().await;
    engine.receiver.hang.store(false, Ordering::SeqCst);
    let reclaim = engine.delivery(destination).await;
    assert_eq!(
        reclaim.delivered.transaction_id, 0,
        "the reclaim follows the rebase"
    );
    engine.receiver.resume.notify_waiters();

    assert_eq!(stale.await, 0, "a pre-rebase send's result is discarded");
    let held = engine.delivery(destination).await;
    assert_eq!(
        held.delivered, reclaim.delivered,
        "a pre-rebase advance moved the cursor"
    );
    assert_eq!(held.lease_owner, reclaim.lease_owner);

    assert_eq!(rebased.await, 3);
    let delivery = engine.delivery(destination).await;
    assert_eq!(delivery.delivered, engine.position(ids[2]).await);
    assert_eq!(delivery.delivered_count, 3);
    engine.cleanup().await;
}

#[tokio::test]
async fn a_tick_ends_even_when_an_idle_destination_is_released_already_due() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let s3 = engine.destination(scope, Kind::S3).await;
    let busy = engine.destination(scope, Kind::Webhook).await;
    engine.written(scope, 2).await;
    settle(engine.pool()).await;
    let idle = engine.destination(scope, Kind::Webhook).await;
    let skewed = Cadence {
        tick: Duration::ZERO,
        ..CADENCE
    };

    let delivered = tokio::time::timeout(Duration::from_secs(30), engine.drain_with(skewed))
        .await
        .expect("a destination due again at once does not spin the tick");
    assert_eq!(delivered, 2, "the busy destination is still drained");
    assert_eq!(engine.receiver.received(busy).len(), 2);
    assert!(engine.receiver.received(idle).is_empty());
    assert!(engine.receiver.received(s3).is_empty());
    engine.cleanup().await;
}

#[tokio::test]
async fn idle_and_failed_deliveries_are_due_again_on_the_database_clock() {
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let idle = engine.destination(scope, Kind::Webhook).await;
    settle(engine.pool()).await;

    let before = engine.clock().await;
    assert_eq!(engine.drain().await, 0);
    let after = engine.clock().await;
    let tick = TimeDelta::from_std(CADENCE.tick).unwrap();
    let released = engine.delivery(idle).await;
    assert!(
        released.next_attempt_at >= before + tick && released.next_attempt_at <= after + tick,
        "an idle destination is due a tick after its release, between {} and {}, not {}",
        before + tick,
        after + tick,
        released.next_attempt_at
    );
    assert_eq!(released.lease_owner, None);
    engine.cleanup().await;
}

#[tokio::test]
async fn a_deep_backlog_takes_turns_with_other_destinations() {
    const EVENTS: usize = 250;
    let engine = Engine::licensed(Tier::Enterprise).await;
    let scope = engine.organization().await;
    let mut destinations = Vec::new();
    for _ in 0..3 {
        destinations.push(engine.destination(scope, Kind::Webhook).await);
    }
    for (destination, seconds_ago) in destinations.iter().zip([3, 2, 1]) {
        engine.schedule(*destination, seconds_ago).await;
    }
    engine.written(scope, EVENTS).await;
    settle(engine.pool()).await;
    let single = Cadence {
        concurrency: 1,
        ..CADENCE
    };

    assert_eq!(engine.drain_with(single).await, 3 * EVENTS);
    let order = engine.receiver.order();
    assert_eq!(order.len(), 9, "each backlog is three batches");
    assert_eq!(
        order,
        [destinations.as_slice(); 3].concat(),
        "one slot serves the destinations in turn, not one backlog to the end"
    );
    engine.cleanup().await;
}
