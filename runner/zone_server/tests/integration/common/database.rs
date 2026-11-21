use std::future::Future;

use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, PgConnection, PgPool};
use tokio::sync::{Semaphore, SemaphorePermit};
use uuid::Uuid;

/// Every test migrates an empty database of its own; more at once
/// only queue on the same disk writes, and the tests' own time bounds with them.
static DATABASES: Semaphore = Semaphore::const_new(4);

/// The advisory lock, in `TEST_DATABASE_URL`'s database, that every test-database drop holds
/// shared. `DROP DATABASE` keeps its transaction ID through a forced checkpoint, which holds the
/// cluster-wide snapshot horizon back for every other test until the checkpoint finishes.
pub const DROPPING: i64 = 0x7A6F_6E65_4452_4F50;

const ENTER: &str = "SELECT pg_advisory_lock_shared($1)";
const QUIESCE: &str = "SELECT pg_advisory_xact_lock($1)";

/// An empty database of a test's own, for tests that migrate it themselves or
/// change rows every other test would see.
pub struct Database {
    pub(crate) admin: PgPool,
    pub(crate) pool: PgPool,
    pub(crate) name: String,
    pub(crate) _slot: SemaphorePermit<'static>,
}

impl Database {
    /// Creates `<prefix>_<uuid>` beside `TEST_DATABASE_URL`'s database.
    pub async fn new(prefix: &'static str) -> Self {
        let slot = DATABASES
            .acquire()
            .await
            .expect("the database slots are never closed");
        let admin = PgPool::connect(&url()).await.unwrap();
        // Identifiers contain only a fixed prefix and UUID hexadecimal.
        let name = format!("{prefix}_{}", Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .unwrap();
        let options = (*admin.connect_options()).clone().database(&name);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        Self {
            admin,
            pool,
            name,
            _slot: slot,
        }
    }

    pub async fn cleanup(self) {
        self.pool.close().await;
        dropping(
            sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {}", self.name)))
                .execute(&self.admin),
        )
        .await
        .unwrap();
        self.admin.close().await;
    }
}

pub fn url() -> String {
    std::env::var("TEST_DATABASE_URL").expect("disposable database")
}

/// Runs a `DROP DATABASE` while holding [`DROPPING`] shared, so [`quiesce`] can wait it out.
/// Drops still run alongside each other.
pub async fn dropping<T>(statement: impl Future<Output = T>) -> T {
    let mut gate = PgConnection::connect(&url())
        .await
        .expect("the test database server is reachable");
    sqlx::query(ENTER)
        .bind(DROPPING)
        .execute(&mut gate)
        .await
        .expect("the drop gate opens");
    let output = statement.await;
    gate.close().await.expect("the drop gate closes");
    output
}

/// Waits for every test-database drop running or queued when it is called. It takes no
/// transaction ID and holds the lock only for its one statement.
pub async fn quiesce() {
    let mut gate = PgConnection::connect(&url())
        .await
        .expect("the test database server is reachable");
    sqlx::query(QUIESCE)
        .bind(DROPPING)
        .execute(&mut gate)
        .await
        .expect("running drops finish");
    gate.close().await.expect("the drop gate closes");
}
