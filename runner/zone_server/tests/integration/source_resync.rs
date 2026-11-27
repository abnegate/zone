//! A source is queued for one index however many resync passes reach it, including the passes
//! of a demoted leader and its successor that both run before either has an indexing permit.

use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;
use zone_server::db::context_gatherings;
use zone_server::db::migrations;
use zone_server::state::AppState;
use zone_server::workers::source_resync::{self, ABANDONED};

use crate::common::{self, Database};

const PREFIX: &str = "source_resync";
const SETTLE: Duration = Duration::from_secs(1);
const CLAIMANTS: usize = 8;

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
    sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
        .bind(source)
        .fetch_one(pool)
        .await
        .expect("the source's gatherings")
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

    let claims = futures::future::join_all((0..CLAIMANTS).map(|_| {
        context_gatherings::claim_index(&database.pool, source, workspace, user, ABANDONED)
    }))
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
    let claim = |source: Uuid| {
        context_gatherings::claim_index(&database.pool, source, workspace, user, ABANDONED)
    };
    let running = source(&database.pool, workspace).await;
    gathering(&database.pool, running, "running", ABANDONED * 2).await;
    let pending = source(&database.pool, workspace).await;
    gathering(&database.pool, pending, "pending", ABANDONED / 2).await;
    let abandoned = source(&database.pool, workspace).await;
    gathering(&database.pool, abandoned, "pending", ABANDONED * 2).await;
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
        claim(pending).await.unwrap().is_some(),
        claim(abandoned).await.unwrap().is_some(),
        claim(finished).await.unwrap().is_some(),
        claim(Uuid::new_v4()).await.unwrap().is_some(),
    ];
    database.cleanup().await;

    assert_eq!(
        outcomes,
        [false, false, true, true, false],
        "running and recently queued indexes hold the source, an abandoned or finished one \
         does not, and a missing source is never queued"
    );
}
