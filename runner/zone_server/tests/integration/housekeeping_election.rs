//! Housekeeping cluster sweeps run on one elected replica at a time, and the two sweeps that
//! notify claim in Postgres before they send, so a leader change sends nothing twice.

mod entered;
mod pass;
mod recorder;
mod regressing;
mod replica;
mod tally;

use std::future::Future;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use abnegate_notify::{Error, Fanout};
use chrono::{NaiveDateTime, TimeDelta, Utc};
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;
use tokio::time::Instant;
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::state::AppState;
use zone_server::workers::housekeeping;
use zone_server::workers::housekeeping::{
    Catchup, Failure, LEADER_KEY, Leadership, Registry, Schedule, Scope, Sweep, Timing, Warmup,
};
use zone_server::workers::regression::{self, RegressionChecker, RegressionSettings};
use zone_server::workers::reports::{self, Cadence, ReportSettings};

use crate::common::blackhole::Blackhole;
use crate::common::{self, Database};
use recorder::Recorder;
use regressing::Regressing;
use replica::Replica;
use tally::Tally;

const PREFIX: &str = "housekeeping_election";
const PERIOD: Duration = Duration::from_secs(1);
const SWEEP: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(20);
const SLACK: Duration = Duration::from_millis(150);

type Deliveries = Arc<Mutex<Vec<(usize, NaiveDateTime)>>>;

fn timing() -> Timing {
    Timing {
        acquire: Duration::from_millis(200),
        ping: Duration::from_millis(200),
        timeout: Duration::from_secs(1),
        freshness: Duration::from_secs(1),
        wait: Duration::from_secs(2),
        idle: Duration::from_secs(2),
    }
}

/// Tries for the lock far more often than [`timing`], so it wins any race with a replica
/// that was just demoted.
fn eager() -> Timing {
    Timing {
        acquire: Duration::from_millis(50),
        ..timing()
    }
}

fn options(database: &Database) -> PgConnectOptions {
    (*database.admin.connect_options())
        .clone()
        .database(&database.name)
}

fn every(period: Duration) -> Schedule {
    Schedule::new(period, Warmup::Period, Catchup::Skip)
}

fn state(database: &Database) -> AppState {
    common::create_test_state(common::test_config(), database.pool.clone())
}

fn counted(replica: usize, tally: &Arc<Tally>) -> Registry {
    let cluster = Arc::clone(tally);
    let local = Arc::clone(tally);
    Registry::new()
        .with(Sweep::new(
            "cluster",
            every(PERIOD),
            Scope::Cluster,
            move || {
                let tally = Arc::clone(&cluster);
                async move {
                    let _entered = tally.enter(replica);
                    tokio::time::sleep(SWEEP).await;
                    Ok(())
                }
            },
        ))
        .with(Sweep::new(
            "replica",
            every(PERIOD),
            Scope::Replica,
            move || {
                local.turn(replica);
                async { Ok(()) }
            },
        ))
}

async fn until(limit: Duration, mut settled: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if settled() {
            return true;
        }
        tokio::time::sleep(POLL).await;
    }
    settled()
}

async fn until_stored<Check, Checked>(limit: Duration, mut settled: Check) -> bool
where
    Check: FnMut() -> Checked,
    Checked: Future<Output = bool>,
{
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if settled().await {
            return true;
        }
        tokio::time::sleep(POLL).await;
    }
    settled().await
}

const LEADER_SESSIONS: &str = "
    FROM pg_locks
    WHERE locktype = 'advisory'
      AND granted
      AND database = (SELECT oid FROM pg_database WHERE datname = $1)
      AND classid::bigint = $2
      AND objid::bigint = $3
      AND objsubid = 1";

fn halves() -> (i64, i64) {
    (LEADER_KEY >> 32, LEADER_KEY & 0xFFFF_FFFF)
}

async fn holders(database: &Database) -> i64 {
    let (high, low) = halves();
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) {LEADER_SESSIONS}"
    )))
    .bind(&database.name)
    .bind(high)
    .bind(low)
    .fetch_one(&database.admin)
    .await
    .expect("the lock table is readable")
}

async fn terminate_leader(database: &Database) -> usize {
    let (high, low) = halves();
    let terminated: Vec<bool> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT pg_terminate_backend(pid) {LEADER_SESSIONS}"
    )))
    .bind(&database.name)
    .bind(high)
    .bind(low)
    .fetch_all(&database.admin)
    .await
    .expect("the leader's session can be terminated");
    terminated.into_iter().filter(|done| *done).count()
}

fn assert_never_overlapped(tally: &Tally) {
    assert_eq!(tally.peak(), 1, "two cluster sweeps ran at once");
    let passes = tally.passes();
    for (index, pass) in passes.iter().enumerate() {
        for other in &passes[index + 1..] {
            assert!(
                !pass.overlaps(other),
                "replica {} and replica {} ran a cluster sweep at the same time",
                pass.replica,
                other.replica
            );
        }
    }
}

#[tokio::test]
async fn cluster_sweeps_run_on_one_replica_and_replica_sweeps_on_both() {
    let database = Database::new(PREFIX).await;
    let tally = Arc::new(Tally::default());
    let first = Replica::start(
        counted(0, &tally),
        Leadership::start(options(&database), timing()),
    );
    let second = Replica::start(
        counted(1, &tally),
        Leadership::start(options(&database), timing()),
    );

    assert!(until(Duration::from_secs(5), || first.leads() || second.leads()).await);
    tokio::time::sleep(Duration::from_secs(6)).await;
    first.stop().await;
    second.stop().await;

    assert_never_overlapped(&tally);
    let passes = tally.passes();
    assert!(
        passes.len() >= 5,
        "a leader swept {} times in six seconds",
        passes.len()
    );
    assert!(
        passes.iter().all(|pass| pass.replica == passes[0].replica),
        "with no leader change only the leader sweeps"
    );
    for pair in passes.windows(2) {
        assert!(
            pair[1].started - pair[0].started >= PERIOD / 2,
            "two cluster passes started {:?} apart, inside one half-period window",
            pair[1].started - pair[0].started
        );
    }
    let span = passes[passes.len() - 1].started - passes[0].started;
    let periods = (span.as_secs_f64() / PERIOD.as_secs_f64()).round() as usize + 1;
    assert!(
        passes.len().abs_diff(periods) <= 1,
        "{} passes across {periods} periods",
        passes.len()
    );
    assert!(
        tally.turns(0) >= 6,
        "replica 0 swept {} times",
        tally.turns(0)
    );
    assert!(
        tally.turns(1) >= 6,
        "replica 1 swept {} times",
        tally.turns(1)
    );
    database.cleanup().await;
}

#[tokio::test]
async fn terminating_the_leaders_session_moves_leadership_without_overlap() {
    let database = Database::new(PREFIX).await;
    let tally = Arc::new(Tally::default());
    let old = Replica::start(
        counted(0, &tally),
        Leadership::start(options(&database), timing()),
    );
    assert!(until(Duration::from_secs(5), || old.leads()).await);
    let new = Replica::start(
        counted(1, &tally),
        Leadership::start(options(&database), eager()),
    );
    assert!(until(Duration::from_secs(5), || tally.passes_by(0).len() >= 2).await);

    assert_eq!(terminate_leader(&database).await, 1);
    assert!(
        until(Duration::from_secs(10), || new.leads()).await,
        "the follower takes over once the leader's session is gone"
    );
    let acquired = new
        .leadership
        .tenure()
        .expect("the new leader holds the lock")
        .acquired();
    assert!(until(Duration::from_secs(5), || tally.passes_by(1).len() >= 2).await);
    assert!(!old.leads(), "the old leader stays demoted");
    old.stop().await;
    new.stop().await;

    assert_never_overlapped(&tally);
    let before = tally.passes_by(0);
    let after = tally.passes_by(1);
    assert!(before[before.len() - 1].finished <= after[0].started);
    assert!(after[0].started >= acquired + timing().wait);
    database.cleanup().await;
}

#[tokio::test]
async fn a_partitioned_leader_demotes_before_its_successor_starts() {
    let database = Database::new(PREFIX).await;
    let direct = options(&database);
    let proxy = Blackhole::start(direct.get_host().to_string(), direct.get_port()).await;
    let routed = options(&database).host("127.0.0.1").port(proxy.port());
    let tally = Arc::new(Tally::default());
    let old = Replica::start(counted(0, &tally), Leadership::start(routed, timing()));
    assert!(until(Duration::from_secs(5), || old.leads()).await);
    let new = Replica::start(counted(1, &tally), Leadership::start(direct, eager()));
    assert!(until(Duration::from_secs(5), || tally.passes_by(0).len() >= 2).await);

    let silenced = Instant::now();
    proxy.silence();

    assert!(until(Duration::from_secs(3), || !old.leads()).await);
    let demoted = Instant::now();
    assert!(
        demoted <= silenced + timing().freshness + SLACK,
        "the partitioned leader kept leading {:?} after its last ping could be answered",
        demoted - silenced
    );
    assert!(
        until(Duration::from_secs(10), || new.leads()).await,
        "the follower leads once the partitioned session times out"
    );
    let acquired = new
        .leadership
        .tenure()
        .expect("the new leader holds the lock")
        .acquired();
    assert!(
        acquired + timing().ping + SLACK >= silenced + timing().idle,
        "the lock was free {:?} after the partition, before the idle session timeout",
        acquired - silenced
    );
    assert!(until(Duration::from_secs(5), || !tally.passes_by(1).is_empty()).await);
    old.stop().await;
    new.stop().await;

    assert_never_overlapped(&tally);
    let before = tally.passes_by(0);
    let after = tally.passes_by(1);
    let stopped = before[before.len() - 1].finished;
    assert!(
        stopped <= silenced + timing().freshness + SLACK,
        "the old leader's sweep ran {:?} past the partition",
        stopped - silenced
    );
    assert!(after[0].started > stopped);
    assert!(after[0].started >= acquired + timing().wait);
    database.cleanup().await;
}

#[tokio::test]
async fn a_paused_leader_stops_leading_while_it_still_holds_the_lock() {
    let database = Database::new(PREFIX).await;
    let (sender, receiver) = mpsc::channel();
    let (stall, stalled) = tokio::sync::oneshot::channel::<()>();
    let paused = options(&database);
    let process = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("the paused process has a runtime");
        runtime.block_on(async move {
            let leadership = Leadership::start(paused, timing());
            sender
                .send(leadership.clone())
                .expect("the test is listening");
            let _ = stalled.await;
            std::thread::sleep(Duration::from_millis(1_500));
            tokio::time::sleep(timing().ping * 2).await;
            leadership.release().await;
        });
    });
    let leadership = receiver
        .recv()
        .expect("the paused process started an election");
    assert!(until(Duration::from_secs(5), || leadership.leads()).await);

    stall.send(()).expect("the paused process is waiting");
    tokio::time::sleep(POLL).await;
    let confirmed = leadership.tenure().expect("the lock is held").confirmed();
    tokio::time::sleep_until(confirmed + timing().freshness + POLL).await;

    assert!(
        leadership.tenure().is_some(),
        "the stalled process has not noticed anything, so it still records the lock as held"
    );
    assert_eq!(
        holders(&database).await,
        1,
        "the session still holds the lock"
    );
    assert!(
        !leadership.leads(),
        "a leader whose last answered ping is older than freshness must not dispatch"
    );

    tokio::task::spawn_blocking(move || process.join())
        .await
        .expect("the join does not panic")
        .expect("the paused process does not panic");
    assert_eq!(holders(&database).await, 0, "release gives the lock back");
    database.cleanup().await;
}

#[tokio::test]
async fn a_draining_leader_hands_leadership_over_at_once() {
    let database = Database::new(PREFIX).await;
    let state = state(&database);
    let leadership = Leadership::start(options(&database), timing());
    let worker = housekeeping::spawn(state.clone(), leadership.clone());
    assert!(until(Duration::from_secs(5), || leadership.leads()).await);
    let successor = Leadership::start(options(&database), eager());

    state.drain().begin();

    assert!(
        until(SLACK, || !leadership.leads()).await,
        "a draining replica kept dispatching cluster sweeps"
    );
    assert!(
        until(Duration::from_secs(5), || successor.tenure().is_some()).await,
        "no other replica could take the lock while this one drained"
    );
    worker.abort();
    let _ = worker.await;
    leadership.release().await;
    successor.release().await;
    database.cleanup().await;
}

fn reportable(
    replica: usize,
    state: AppState,
    settings: &ReportSettings,
    clock: &Arc<Mutex<NaiveDateTime>>,
    deliveries: &Deliveries,
) -> Registry {
    let fanout = Arc::new(Fanout::new().with(Recorder::new(replica, clock, deliveries)));
    let checkers = Arc::new(reports::worker::checkers(settings));
    let settings = Arc::new(settings.clone());
    let clock = Arc::clone(clock);
    Registry::new().with(Sweep::new(
        "scheduled-reports",
        every(PERIOD),
        Scope::Cluster,
        move || {
            let state = state.clone();
            let settings = Arc::clone(&settings);
            let checkers = Arc::clone(&checkers);
            let fanout = Arc::clone(&fanout);
            let now = *clock.lock().unwrap_or_else(PoisonError::into_inner);
            async move {
                reports::worker::run_cycle(&state, &settings, &checkers, &fanout, now)
                    .await
                    .map_err(Failure::new)
            }
        },
    ))
}

fn daily() -> ReportSettings {
    ReportSettings {
        enabled: true,
        schedule: reports::Schedule {
            cadence: Cadence::Daily,
            hour: 8,
            ..reports::Schedule::default()
        },
        ..ReportSettings::default()
    }
}

/// Half past eight on the `offset`th day from today, half an hour after that day's slot.
fn day(offset: i64) -> NaiveDateTime {
    let today = Utc::now().date_naive();
    today.and_hms_opt(8, 30, 0).expect("a valid time") + TimeDelta::days(offset)
}

fn slot(offset: i64) -> NaiveDateTime {
    day(offset) - TimeDelta::minutes(30)
}

/// A workspace whose finished run keeps it reportable on every simulated day.
async fn reporting_workspace(pool: &PgPool) -> Uuid {
    let (_, workspace_id, _) = common::setup_test_data(pool).await;
    let task_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (workspace_id, title, description) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(workspace_id)
    .bind("digest")
    .bind("a run for the digest to describe")
    .fetch_one(pool)
    .await
    .expect("the task is created");
    let finished = day(30);
    sqlx::query(
        "INSERT INTO task_runs (task_id, status, started_at, completed_at)
         VALUES ($1, 'completed', $2, $2)",
    )
    .bind(task_id)
    .bind(finished)
    .execute(pool)
    .await
    .expect("the run is created");
    workspace_id
}

async fn stored_slot(pool: &PgPool, workspace_id: Uuid) -> Option<NaiveDateTime> {
    sqlx::query_scalar("SELECT slot FROM report_deliveries WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .expect("the ledger is readable")
}

fn delivered(deliveries: &Deliveries) -> Vec<(usize, NaiveDateTime)> {
    deliveries
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[tokio::test]
async fn a_digest_goes_out_once_per_slot_while_leadership_changes() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let workspace_id = reporting_workspace(&database.pool).await;
    let settings = daily();
    let clock = Arc::new(Mutex::new(day(0)));
    let deliveries: Deliveries = Arc::default();
    let start = |replica: usize, timing: Timing| {
        Replica::start(
            reportable(replica, state(&database), &settings, &clock, &deliveries),
            Leadership::start(options(&database), timing),
        )
    };
    let mut replicas = vec![start(0, timing()), start(1, timing())];
    assert!(
        until_stored(Duration::from_secs(10), || async {
            stored_slot(&database.pool, workspace_id).await == Some(slot(0))
        })
        .await,
        "the leader seeds the workspace with the slot that already passed"
    );

    for offset in 1..=3 {
        *clock.lock().unwrap_or_else(PoisonError::into_inner) = day(offset);
        assert!(
            until(Duration::from_secs(10), || {
                delivered(&deliveries)
                    .iter()
                    .any(|(_, at)| *at == day(offset))
            })
            .await,
            "the digest for day {offset} goes out"
        );
        if offset < 3 {
            let leader = replicas
                .iter()
                .position(Replica::leads)
                .expect("the replica that delivered leads");
            replicas.remove(leader).stop().await;
            assert!(until(Duration::from_secs(5), || replicas[0].leads()).await);
            replicas.push(start(offset as usize + 1, eager()));
        }
    }
    tokio::time::sleep(PERIOD * 3).await;
    for replica in replicas {
        replica.stop().await;
    }

    let deliveries = delivered(&deliveries);
    assert_eq!(
        deliveries.iter().map(|(_, at)| *at).collect::<Vec<_>>(),
        [day(1), day(2), day(3)],
        "each slot's digest goes out exactly once across the leader changes"
    );
    for pair in deliveries.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "each digest came from a different leader"
        );
    }
    assert_eq!(
        stored_slot(&database.pool, workspace_id).await,
        Some(slot(3))
    );
    database.cleanup().await;
}

#[tokio::test]
async fn two_replicas_owing_the_same_digest_send_it_once() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    reporting_workspace(&database.pool).await;
    let settings = daily();
    let checkers = reports::worker::checkers(&settings);
    let clock = Arc::new(Mutex::new(day(1)));
    let deliveries: Deliveries = Arc::default();
    let first = Fanout::new().with(Recorder::new(0, &clock, &deliveries));
    let second = Fanout::new().with(Recorder::new(1, &clock, &deliveries));
    let (one, two) = (state(&database), state(&database));
    reports::worker::run_cycle(&one, &settings, &checkers, &first, day(0))
        .await
        .unwrap();

    let (left, right) = tokio::join!(
        reports::worker::run_cycle(&one, &settings, &checkers, &first, day(1)),
        reports::worker::run_cycle(&two, &settings, &checkers, &second, day(1)),
    );
    left.unwrap();
    right.unwrap();
    reports::worker::run_cycle(
        &two,
        &settings,
        &checkers,
        &second,
        day(1) + TimeDelta::hours(1),
    )
    .await
    .unwrap();

    assert_eq!(
        delivered(&deliveries).len(),
        1,
        "the claim lets exactly one replica send"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_delivery_that_may_succeed_later_gives_its_slot_back() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let workspace_id = reporting_workspace(&database.pool).await;
    let settings = daily();
    let checkers = reports::worker::checkers(&settings);
    let clock = Arc::new(Mutex::new(day(1)));
    let deliveries: Deliveries = Arc::default();
    let fanout = Fanout::new().with(
        Recorder::new(0, &clock, &deliveries)
            .failing(Error::unreachable("hooks.zone.test", "connection refused")),
    );
    let state = state(&database);
    reports::worker::run_cycle(&state, &settings, &checkers, &fanout, day(0))
        .await
        .unwrap();
    assert_eq!(
        stored_slot(&database.pool, workspace_id).await,
        Some(slot(0))
    );

    reports::worker::run_cycle(&state, &settings, &checkers, &fanout, day(1))
        .await
        .unwrap();
    assert!(delivered(&deliveries).is_empty());
    assert_eq!(
        stored_slot(&database.pool, workspace_id).await,
        Some(slot(0)),
        "a retryable failure rolls the claim back"
    );

    reports::worker::run_cycle(&state, &settings, &checkers, &fanout, day(1))
        .await
        .unwrap();
    assert_eq!(
        delivered(&deliveries),
        [(0, day(1))],
        "the next tick delivers it"
    );
    assert_eq!(
        stored_slot(&database.pool, workspace_id).await,
        Some(slot(1))
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_delivery_every_channel_refuses_for_good_keeps_its_slot_closed() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let workspace_id = reporting_workspace(&database.pool).await;
    let settings = daily();
    let checkers = reports::worker::checkers(&settings);
    let clock = Arc::new(Mutex::new(day(1)));
    let deliveries: Deliveries = Arc::default();
    let fanout = Fanout::new().with(
        Recorder::new(0, &clock, &deliveries).failing(Error::rejected(
            "hooks.zone.test",
            404,
            "Unknown Webhook",
        )),
    );
    let state = state(&database);
    reports::worker::run_cycle(&state, &settings, &checkers, &fanout, day(0))
        .await
        .unwrap();

    reports::worker::run_cycle(&state, &settings, &checkers, &fanout, day(1))
        .await
        .unwrap();
    reports::worker::run_cycle(&state, &settings, &checkers, &fanout, day(1))
        .await
        .unwrap();

    assert!(delivered(&deliveries).is_empty());
    assert_eq!(
        stored_slot(&database.pool, workspace_id).await,
        Some(slot(1))
    );
    database.cleanup().await;
}

fn watching(
    replica: usize,
    state: AppState,
    clock: &Arc<Mutex<NaiveDateTime>>,
    deliveries: &Deliveries,
    tally: &Arc<Tally>,
) -> Registry {
    let fanout = Arc::new(Fanout::new().with(Recorder::new(replica, clock, deliveries)));
    let checkers: Arc<Vec<Arc<dyn RegressionChecker>>> = Arc::new(vec![Arc::new(Regressing)]);
    let settings = Arc::new(RegressionSettings::default());
    let tally = Arc::clone(tally);
    Registry::new().with(Sweep::new(
        "regression-watch",
        every(PERIOD),
        Scope::Cluster,
        move || {
            let state = state.clone();
            let checkers = Arc::clone(&checkers);
            let settings = Arc::clone(&settings);
            let fanout = Arc::clone(&fanout);
            let tally = Arc::clone(&tally);
            async move {
                let now = Utc::now().naive_utc();
                regression::worker::run_cycle(&state, &checkers, &settings, &fanout, now)
                    .await
                    .map_err(Failure::new)?;
                tally.turn(replica);
                Ok(())
            }
        },
    ))
}

async fn task(pool: &PgPool, workspace_id: Uuid, shipped: Option<NaiveDateTime>) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO tasks (workspace_id, title, description, status, shipped_at)
         VALUES ($1, 'Stop the importer dropping rows', 'a shipped fix', 'complete', $2)
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(shipped)
    .fetch_one(pool)
    .await
    .expect("the task is created")
}

async fn alerted(pool: &PgPool, task_id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM regression_alerts WHERE task_id = $1)")
        .bind(task_id)
        .fetch_one(pool)
        .await
        .expect("the ledger is readable")
}

#[tokio::test]
async fn a_regression_is_alerted_once_across_a_leader_change_and_old_alerts_are_pruned() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let pool = &database.pool;
    let now = Utc::now().naive_utc();
    let monitoring = RegressionSettings::default().policy.monitoring;
    let (_, workspace_id, _) = common::setup_test_data(pool).await;
    let watched = task(pool, workspace_id, Some(now - TimeDelta::days(1))).await;
    sqlx::query(
        "INSERT INTO task_runs (task_id, status, started_at, completed_at)
         VALUES ($1, 'completed', $2, $3)",
    )
    .bind(watched)
    .bind(now - TimeDelta::hours(2))
    .bind(now - TimeDelta::hours(1))
    .execute(pool)
    .await
    .expect("the run is created");
    let forgotten = task(pool, workspace_id, None).await;
    sqlx::query("INSERT INTO regression_alerts (task_id, alerted_at) VALUES ($1, $2)")
        .bind(forgotten)
        .bind(now - monitoring - TimeDelta::days(1))
        .execute(pool)
        .await
        .expect("the old alert is recorded");

    let clock = Arc::new(Mutex::new(now));
    let deliveries: Deliveries = Arc::default();
    let tally = Arc::new(Tally::default());
    let mut replicas = vec![
        Replica::start(
            watching(0, state(&database), &clock, &deliveries, &tally),
            Leadership::start(options(&database), timing()),
        ),
        Replica::start(
            watching(1, state(&database), &clock, &deliveries, &tally),
            Leadership::start(options(&database), timing()),
        ),
    ];
    assert!(
        until(Duration::from_secs(10), || !delivered(&deliveries)
            .is_empty())
        .await
    );

    let leader = replicas
        .iter()
        .position(Replica::leads)
        .expect("the replica that alerted leads");
    let survivor = 1 - leader;
    replicas.remove(leader).stop().await;
    assert!(until(Duration::from_secs(5), || replicas[0].leads()).await);
    let before = tally.turns(survivor);
    assert!(
        until(Duration::from_secs(10), || tally.turns(survivor)
            >= before + 3)
        .await,
        "the new leader scans the same regression several times"
    );
    for replica in replicas {
        replica.stop().await;
    }

    assert_eq!(
        delivered(&deliveries).len(),
        1,
        "a regression is alerted once, whichever replica leads"
    );
    assert!(alerted(pool, watched).await);
    assert!(
        !alerted(pool, forgotten).await,
        "an alert older than the watch window is pruned"
    );
    database.cleanup().await;
}
