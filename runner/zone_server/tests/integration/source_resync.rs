//! A source is queued for one index however many resync passes reach it, including the passes
//! of a demoted leader and its successor that both run before either has an indexing permit.

use std::time::Duration;

use axum::http::StatusCode;
use chrono::NaiveDateTime;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;
use zone_server::db::context_gatherings::{
    self, ABANDONED, FAILED, ORPHANED, PENDING, RUNNING, SUPERSEDED,
};
use zone_server::db::migrations;
use zone_server::state::AppState;
use zone_server::workers::source_resync;

use crate::common::{self, Database, TestClient};

const PREFIX: &str = "source_resync";
const SETTLE: Duration = Duration::from_secs(1);
const CLAIMANTS: usize = 8;
/// How far past being orphaned a stranded gathering is.
const LATE: Duration = Duration::from_secs(60);
/// How long a released replica has to start what it queued.
const STARTING: Duration = Duration::from_secs(5);

async fn source(pool: &PgPool, workspace: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO sources (workspace_id, name, source_type, config) \
         VALUES ($1, $2, 'text', '{\"content\": \"The release notes.\"}') RETURNING id",
    )
    .bind(workspace)
    .bind(format!("Notes {}", Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .expect("a text source")
}

async fn gatherings(pool: &PgPool, source: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM context_gatherings WHERE source_ids @> ARRAY[$1]::uuid[]",
    )
    .bind(source)
    .fetch_one(pool)
    .await
    .expect("the source's gatherings")
}

/// The database's clock, which is what stamps a gathering when it begins running.
async fn now(pool: &PgPool) -> NaiveDateTime {
    sqlx::query_scalar("SELECT LOCALTIMESTAMP")
        .fetch_one(pool)
        .await
        .expect("the database clock")
}

async fn started_since(pool: &PgPool, source: Uuid, since: NaiveDateTime) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM context_gatherings \
         WHERE source_ids @> ARRAY[$1]::uuid[] AND started_at >= $2",
    )
    .bind(source)
    .bind(since)
    .fetch_one(pool)
    .await
    .expect("the source's started gatherings")
}

/// How many gatherings of `source` began running since `since`, once the first has and any
/// second has had as long again to follow it.
async fn started(pool: &PgPool, source: Uuid, since: NaiveDateTime) -> i64 {
    let deadline = tokio::time::Instant::now() + STARTING;
    while started_since(pool, source, since).await == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(SETTLE).await;
    started_since(pool, source, since).await
}

/// Ages every gathering of `source` by `age`, as if it had been queued that long ago.
async fn age(pool: &PgPool, source: Uuid, age: Duration) {
    sqlx::query(
        "UPDATE context_gatherings \
         SET created_at = created_at - make_interval(secs => $2), \
             started_at = started_at - make_interval(secs => $2) \
         WHERE source_ids @> ARRAY[$1]::uuid[]",
    )
    .bind(source)
    .bind(age.as_secs_f64())
    .execute(pool)
    .await
    .expect("the aged gatherings");
}

/// A member's token and the workspace they made through `client`.
async fn member(client: &TestClient) -> (String, Uuid) {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": common::test_email(), "password": common::test_password()}),
        )
        .await;
    registered.assert_status(StatusCode::CREATED);
    let token = registered.json_value()["access_token"]
        .as_str()
        .expect("an access token")
        .to_string();
    let suffix = Uuid::new_v4().simple();
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Resync", "slug": format!("resync-{suffix}")}),
            &token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("an organization")
        .to_string();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Resync", "slug": format!("resync-{suffix}")}),
            &token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .parse()
        .expect("a workspace id");
    (token, workspace)
}

async fn gathering(pool: &PgPool, source: Uuid, status: &str, age: Duration) {
    sqlx::query(
        "INSERT INTO context_gatherings (source_ids, status, started_at, created_at) \
         VALUES (ARRAY[$1]::uuid[], $2, LOCALTIMESTAMP - make_interval(secs => $3), \
         LOCALTIMESTAMP - make_interval(secs => $3))",
    )
    .bind(source)
    .bind(status)
    .bind(age.as_secs_f64())
    .execute(pool)
    .await
    .expect("a gathering");
}

/// A replica whose indexing permits are all taken, so whatever it queues waits.
async fn busy(database: &Database) -> (AppState, OwnedSemaphorePermit) {
    let state = common::create_test_state(common::test_config(), database.pool.clone());
    let semaphore = state.index_semaphore().clone();
    let permits = u32::try_from(semaphore.available_permits()).expect("a permit count");
    let held = semaphore
        .acquire_many_owned(permits)
        .await
        .expect("the indexing semaphore is open");
    (state, held)
}

#[tokio::test]
async fn a_demoted_leaders_queued_index_and_its_successors_pass_index_a_source_once() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, _) = common::setup_workspace_member(&database.pool).await;
    let source = source(&database.pool, workspace).await;
    let (demoted, demoted_permits) = busy(&database).await;
    let (successor, successor_permits) = busy(&database).await;

    source_resync::poll_sources(&demoted).await.unwrap();
    source_resync::poll_sources(&successor).await.unwrap();
    let queued = gatherings(&database.pool, source).await;
    drop(demoted_permits);
    drop(successor_permits);
    tokio::time::sleep(SETTLE).await;
    let indexed = gatherings(&database.pool, source).await;
    database.cleanup().await;

    assert_eq!(
        queued, 1,
        "the first pass marks the source queued before it waits"
    );
    assert_eq!(
        indexed, 1,
        "the second pass skips a source the first already queued, so it is indexed once"
    );
}

#[tokio::test]
async fn concurrent_claims_on_one_source_queue_it_once() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, user) = common::setup_workspace_member(&database.pool).await;
    let source = source(&database.pool, workspace).await;

    let claims = futures::future::join_all(
        (0..CLAIMANTS)
            .map(|_| context_gatherings::claim_index(&database.pool, source, workspace, user)),
    )
    .await;
    let queued = gatherings(&database.pool, source).await;
    database.cleanup().await;

    let won = claims
        .into_iter()
        .map(|claim| claim.expect("the claim runs"))
        .filter(Option::is_some)
        .count();
    assert_eq!(
        won, 1,
        "exactly one of {CLAIMANTS} claims queues the source"
    );
    assert_eq!(queued, 1);
}

#[tokio::test]
async fn a_source_is_queued_again_once_its_index_ended_or_was_abandoned() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, user) = common::setup_workspace_member(&database.pool).await;
    let claim =
        |source: Uuid| context_gatherings::claim_index(&database.pool, source, workspace, user);
    let running = source(&database.pool, workspace).await;
    gathering(&database.pool, running, RUNNING, ORPHANED / 2).await;
    let orphaned = source(&database.pool, workspace).await;
    gathering(&database.pool, orphaned, RUNNING, ORPHANED + LATE).await;
    let pending = source(&database.pool, workspace).await;
    gathering(&database.pool, pending, PENDING, ABANDONED / 2).await;
    let abandoned = source(&database.pool, workspace).await;
    gathering(&database.pool, abandoned, PENDING, ABANDONED * 2).await;
    let finished = source(&database.pool, workspace).await;
    gathering(
        &database.pool,
        finished,
        "completed",
        Duration::from_secs(60),
    )
    .await;

    let outcomes = [
        claim(running).await.unwrap().is_some(),
        claim(orphaned).await.unwrap().is_some(),
        claim(pending).await.unwrap().is_some(),
        claim(abandoned).await.unwrap().is_some(),
        claim(finished).await.unwrap().is_some(),
        claim(Uuid::new_v4()).await.unwrap().is_some(),
    ];
    database.cleanup().await;

    assert_eq!(
        outcomes,
        [false, true, false, true, true, false],
        "running and recently queued indexes hold the source, one left running past its \
         timeout by a replica that stopped, an abandoned or a finished one does not, and a \
         missing source is never queued"
    );
}

/// A gathering is timed from when it began running, so one that waited long for a permit is
/// not taken to be orphaned the moment it starts.
#[tokio::test]
async fn a_gathering_that_waited_long_to_run_still_holds_its_source() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, user) = common::setup_workspace_member(&database.pool).await;
    let source = source(&database.pool, workspace).await;
    gathering(&database.pool, source, PENDING, ORPHANED + LATE).await;
    let queued: Uuid = sqlx::query_scalar(
        "SELECT id FROM context_gatherings WHERE source_ids @> ARRAY[$1]::uuid[]",
    )
    .bind(source)
    .fetch_one(&database.pool)
    .await
    .expect("the queued gathering");

    let started = context_gatherings::start(&database.pool, queued)
        .await
        .expect("the gathering begins running");
    let claimed = context_gatherings::claim_index(&database.pool, source, workspace, user)
        .await
        .expect("the claim runs");
    database.cleanup().await;

    assert!(started, "a pending gathering can start");
    assert_eq!(
        claimed, None,
        "a gathering that has only just begun running holds its source"
    );
}

/// A claim that takes over an abandoned one supersedes it, so the one it took over cannot start
/// once its replica gets a permit, and nothing starts twice.
#[tokio::test]
async fn only_the_newest_claim_of_a_source_can_start() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, user) = common::setup_workspace_member(&database.pool).await;
    let source = source(&database.pool, workspace).await;
    let claim = || context_gatherings::claim_index(&database.pool, source, workspace, user);
    let stale = claim().await.unwrap().expect("the first claim");
    age(&database.pool, source, ABANDONED * 2).await;
    let newest = claim()
        .await
        .unwrap()
        .expect("a claim past the abandoned one");

    let outcomes = [
        context_gatherings::start(&database.pool, stale)
            .await
            .unwrap(),
        context_gatherings::start(&database.pool, newest)
            .await
            .unwrap(),
        context_gatherings::start(&database.pool, newest)
            .await
            .unwrap(),
    ];
    let superseded = context_gatherings::get_gathering(&database.pool, stale)
        .await
        .unwrap()
        .expect("the superseded gathering");
    database.cleanup().await;

    assert_eq!(
        outcomes,
        [false, true, false],
        "the superseded claim cannot start, and the newest starts once"
    );
    assert_eq!(superseded.status, FAILED);
    assert_eq!(superseded.error_message.as_deref(), Some(SUPERSEDED));
}

/// A changed source queues a re-index beside a queued one, which it supersedes because that one
/// has not read the source yet; a context gathering that reads the same source is left alone.
#[tokio::test]
async fn a_changed_source_supersedes_only_its_queued_index() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, user) = common::setup_workspace_member(&database.pool).await;
    let source = source(&database.pool, workspace).await;
    let queued = context_gatherings::claim_index(&database.pool, source, workspace, user)
        .await
        .unwrap()
        .expect("the first claim");
    let context = context_gatherings::create_gathering(&database.pool, user, workspace, &[source])
        .await
        .unwrap();
    let changed = context_gatherings::queue_index(&database.pool, source, workspace, user)
        .await
        .unwrap()
        .expect("a changed source is always queued");
    let missing = context_gatherings::queue_index(&database.pool, Uuid::new_v4(), workspace, user)
        .await
        .unwrap();

    let outcomes = [
        context_gatherings::start(&database.pool, queued)
            .await
            .unwrap(),
        context_gatherings::start(&database.pool, changed)
            .await
            .unwrap(),
        context_gatherings::start(&database.pool, context)
            .await
            .unwrap(),
    ];
    database.cleanup().await;

    assert_eq!(
        outcomes,
        [false, true, true],
        "the queued index is superseded, while the re-index and the context gathering start"
    );
    assert_eq!(missing, None, "a missing source is never queued");
}

/// A resync pass skips a source whose index is running, but not one whose replica stopped
/// mid-index, which every rolling restart does to whatever it was indexing.
#[tokio::test]
async fn a_resync_pass_queues_a_source_left_running_by_a_stopped_replica() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, _) = common::setup_workspace_member(&database.pool).await;
    let running = source(&database.pool, workspace).await;
    gathering(&database.pool, running, RUNNING, ORPHANED / 2).await;
    let orphaned = source(&database.pool, workspace).await;
    gathering(&database.pool, orphaned, RUNNING, ORPHANED + LATE).await;
    let (state, _permits) = busy(&database).await;

    source_resync::poll_sources(&state).await.unwrap();
    let outcomes = [
        gatherings(&database.pool, running).await,
        gatherings(&database.pool, orphaned).await,
    ];
    database.cleanup().await;

    assert_eq!(
        outcomes,
        [1, 2],
        "a running index holds its source and an orphaned one is queued again"
    );
}

/// A pass that finds a claim older than [`ABANDONED`] queues the source again, and the claim
/// it took over, whose replica was only waiting for a permit, never starts once it gets one.
#[tokio::test]
async fn a_claim_taken_over_after_waiting_too_long_never_starts() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (_, workspace, _) = common::setup_workspace_member(&database.pool).await;
    let source = source(&database.pool, workspace).await;
    let (stalled, stalled_permits) = busy(&database).await;
    let (leader, leader_permits) = busy(&database).await;

    source_resync::poll_sources(&stalled).await.unwrap();
    age(&database.pool, source, ABANDONED * 2).await;
    source_resync::poll_sources(&leader).await.unwrap();
    let queued = gatherings(&database.pool, source).await;
    let since = now(&database.pool).await;
    drop(stalled_permits);
    drop(leader_permits);
    let started = started(&database.pool, source, since).await;
    database.cleanup().await;

    assert_eq!(
        queued, 2,
        "the leader queues a source whose claim was abandoned"
    );
    assert_eq!(
        started, 1,
        "only the newest claim starts, so the source is indexed once"
    );
}

/// A source created on a replica whose permits are all taken is claimed before it waits, so
/// the leader's pass on another replica does not take it for one that never synced.
#[tokio::test]
async fn a_source_created_on_a_busy_replica_is_indexed_once_across_a_leader_pass() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (creator, creator_permits) = busy(&database).await;
    let (leader, leader_permits) = busy(&database).await;
    let client = TestClient::with_state(creator);
    let (token, workspace) = member(&client).await;

    let created = client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/sources"),
            &json!({
                "name": "Notes",
                "source_type": "text",
                "config": {"content": "The release notes."}
            }),
            &token,
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let source: Uuid = created.json_value()["source"]["id"]
        .as_str()
        .expect("a source")
        .parse()
        .expect("a source id");
    source_resync::poll_sources(&leader).await.unwrap();
    let since = now(&database.pool).await;
    drop(creator_permits);
    drop(leader_permits);
    let started = started(&database.pool, source, since).await;
    let queued = gatherings(&database.pool, source).await;
    database.cleanup().await;

    assert_eq!(
        queued, 1,
        "the leader skips a source its creator already queued"
    );
    assert_eq!(started, 1, "the source is indexed once");
}

/// A reindex asked for while a pass's claim waits for a permit is refused, rather than queued
/// beside it to index the source a second time.
#[tokio::test]
async fn a_reindex_while_a_claimed_index_waits_is_refused() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let (replica, permits) = busy(&database).await;
    let client = TestClient::with_state(replica.clone());
    let (token, workspace) = member(&client).await;
    let source = source(&database.pool, workspace).await;

    source_resync::poll_sources(&replica).await.unwrap();
    let reindex = client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/sources/{source}/reindex"),
            &json!({}),
            &token,
        )
        .await;
    let since = now(&database.pool).await;
    drop(permits);
    let started = started(&database.pool, source, since).await;
    database.cleanup().await;

    assert_eq!(
        reindex.status,
        StatusCode::CONFLICT,
        "a queued index refuses another: {}",
        reindex.text()
    );
    assert_eq!(started, 1, "the source is indexed once");
}
