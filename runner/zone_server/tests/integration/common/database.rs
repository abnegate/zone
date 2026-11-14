use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::sync::{Semaphore, SemaphorePermit};
use uuid::Uuid;

/// Every test migrates an empty database of its own; more at once
/// only queue on the same disk writes, and the tests' own time bounds with them.
static DATABASES: Semaphore = Semaphore::const_new(4);

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
        let admin =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("disposable database"))
                .await
                .unwrap();
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
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {}", self.name)))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}
