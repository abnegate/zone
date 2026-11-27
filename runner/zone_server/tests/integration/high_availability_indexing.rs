//! What a real `zone-server` process leaves behind of the indexes it queued but had not started
//! when it stops: whether it stopped on SIGTERM or crashed, the next process to reach their
//! sources indexes them at once, while a live replica's queued index keeps holding its source.

use std::time::Duration;

use chrono::NaiveDateTime;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::ConnectOptions;
use sqlx::PgPool;
use tokio::time::Instant;
use uuid::Uuid;
use zone_server::db::context_gatherings::{self, FAILED, PENDING, STOPPED};
use zone_server::services::fleet::identity::NAME;

use crate::common::Database;
use crate::common::replica::{Account, Environment, Replica};
use crate::common::stub_litellm::StubLitellm;

const PREFIX: &str = "high_availability_indexing";
const DELAY: &str = "ZONE_SHUTDOWN_DELAY_SECONDS";
const GRACE: &str = "ZONE_SHUTDOWN_GRACE_SECONDS";
/// How long a replica with no shutdown delay or grace may take to stop.
const STOPS: Duration = Duration::from_secs(20);
/// How long a replica has to start the index it queued.
const STARTING: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(50);
const SINGLE_NODE: &str = "single-node";

/// One test's database, LiteLLM stand-in and the environment its replicas share.
struct Deployment {
    database: Database,
    _litellm: StubLitellm,
    environment: Environment,
}

impl Deployment {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        let litellm = StubLitellm::start().await;
        let environment = Environment::new(
            database.pool.connect_options().to_url_lossy().to_string(),
            litellm.url(),
        )
        .with_variable(DELAY, "0")
        .with_variable(GRACE, "1");
        Self {
            database,
            _litellm: litellm,
            environment,
        }
    }

    async fn replica(&self) -> Replica {
        Replica::start(&self.environment).await
    }

    /// Stops every replica, waits for their sessions to leave the database, and drops it.
    async fn retire(self, replicas: Vec<Replica>) {
        for mut replica in replicas {
            replica.kill().await;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let connected: i64 =
                sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname = $1")
                    .bind(&self.database.name)
                    .fetch_one(&self.database.admin)
                    .await
                    .expect("the sessions can be counted");
            if connected == 0 {
                break;
            }
            tokio::time::sleep(POLL).await;
        }
        self.database.cleanup().await;
    }
}

/// A source in `account`'s workspace that nothing has indexed yet, and the member who owns it.
async fn source(pool: &PgPool, account: &Account) -> (Uuid, Uuid) {
    let source = sqlx::query_scalar(
        "INSERT INTO sources (workspace_id, name, source_type, config) \
         VALUES ($1, $2, 'text', '{\"content\": \"The release notes.\"}') RETURNING id",
    )
    .bind(account.workspace)
    .bind(format!("Notes {}", Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .expect("a text source");
    let member =
        sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id = $1")
            .bind(account.workspace)
            .fetch_one(pool)
            .await
            .expect("the workspace's member");
    (source, member)
}

/// The process `replica` registered its runner as.
async fn instance(pool: &PgPool, replica: &Replica) -> Uuid {
    sqlx::query_scalar("SELECT instance_id FROM fleet_runners WHERE name = $1")
        .bind(replica.name())
        .fetch_one(pool)
        .await
        .expect("the replica registered as a runner")
}

/// Queues an index of `source` for `replica`, as one still waiting there for an indexing permit.
async fn queued(pool: &PgPool, replica: &Replica, account: &Account) -> (Uuid, Uuid) {
    let (source, member) = source(pool, account).await;
    let owner = instance(pool, replica).await;
    let gathering = context_gatherings::claim_index(pool, source, account.workspace, member, owner)
        .await
        .expect("the claim runs")
        .expect("the source is queued");
    (source, gathering)
}

async fn reindex(replica: &Replica, account: &Account, source: Uuid) -> (StatusCode, Value) {
    replica
        .post_status(
            &format!(
                "/api/workspaces/{}/sources/{source}/reindex",
                account.workspace
            ),
            &json!({}),
            Some(&account.token),
        )
        .await
}

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
/// second has had a moment to follow it.
async fn started(pool: &PgPool, source: Uuid, since: NaiveDateTime) -> i64 {
    let deadline = Instant::now() + STARTING;
    while started_since(pool, source, since).await == 0 && Instant::now() < deadline {
        tokio::time::sleep(POLL).await;
    }
    tokio::time::sleep(SETTLE).await;
    started_since(pool, source, since).await
}

async fn status(pool: &PgPool, gathering: Uuid) -> (String, Option<String>) {
    let row = context_gatherings::get_gathering(pool, gathering)
        .await
        .expect("the gathering can be read")
        .expect("the gathering");
    (row.status, row.error_message)
}

#[tokio::test]
async fn a_terminated_replica_releases_its_queued_indexes_for_its_successor_to_index_at_once() {
    let deployment = Deployment::new().await;
    let pool = deployment.database.pool.clone();
    let mut stopping = deployment.replica().await;
    let account = Account::register(&stopping).await;
    let (source, gathering) = queued(&pool, &stopping, &account).await;

    stopping.terminate();
    let exited = stopping
        .exited(STOPS)
        .await
        .expect("the replica stops after SIGTERM");
    let released = status(&pool, gathering).await;
    let successor = deployment.replica().await;
    let since = now(&pool).await;
    let (code, body) = reindex(&successor, &account, source).await;
    let started = started(&pool, source, since).await;
    deployment.retire(vec![stopping, successor]).await;

    assert!(exited.success(), "{exited}");
    assert_eq!(
        code,
        StatusCode::ACCEPTED,
        "the successor reindexes what the stopped replica had queued at once: {body}"
    );
    assert_eq!(started, 1, "the source is indexed once");
    assert_eq!(
        released,
        (FAILED.to_string(), Some(STOPPED.to_string())),
        "a stopping replica fails the indexes still waiting for its permits"
    );
}

#[tokio::test]
async fn a_single_node_restarted_after_a_crash_reindexes_what_it_had_queued_at_once() {
    let deployment = Deployment::new().await;
    let pool = deployment.database.pool.clone();
    let environment = deployment
        .environment
        .clone()
        .with_variable(NAME, SINGLE_NODE);
    let mut crashed = Replica::start(&environment).await;
    let account = Account::register(&crashed).await;
    let (source, _) = queued(&pool, &crashed, &account).await;

    crashed.kill().await;
    let restarted = Replica::start(&environment).await;
    let since = now(&pool).await;
    let (code, body) = reindex(&restarted, &account, source).await;
    let started = started(&pool, source, since).await;
    deployment.retire(vec![crashed, restarted]).await;

    assert_eq!(
        code,
        StatusCode::ACCEPTED,
        "the process restarted under the crashed one's runner name takes over its queue: {body}"
    );
    assert_eq!(started, 1, "the source is indexed once");
}

#[tokio::test]
async fn a_starting_replica_leaves_a_live_replicas_queued_index_holding_its_source() {
    let deployment = Deployment::new().await;
    let pool = deployment.database.pool.clone();
    let live = deployment.replica().await;
    let account = Account::register(&live).await;
    let (source, gathering) = queued(&pool, &live, &account).await;

    let starting = deployment.replica().await;
    let (code, body) = reindex(&starting, &account, source).await;
    let (queued_status, _) = status(&pool, gathering).await;
    deployment.retire(vec![live, starting]).await;

    assert_eq!(
        code,
        StatusCode::CONFLICT,
        "a live replica's queued index refuses another: {body}"
    );
    assert_eq!(
        queued_status, PENDING,
        "the live replica's index stays queued"
    );
}
