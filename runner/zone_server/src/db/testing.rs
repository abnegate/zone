//! A migrated database of a test's own, beside `TEST_DATABASE_URL`'s, for a test whose rows are
//! server-wide, such as the license, or that must not see another test's rows.

use sqlx::PgPool;
use uuid::Uuid;

pub struct Database {
    pub pool: PgPool,
    admin: PgPool,
    name: String,
}

impl Database {
    /// Creates and migrates a database named `prefix` and a fresh UUID. `prefix` is put into SQL
    /// unquoted, so it must be lowercase letters and underscores.
    pub async fn create(prefix: &'static str) -> Self {
        let admin = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("a disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let name = format!("{prefix}_{}", Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .expect("a database of the test's own");
        let options = (*admin.connect_options()).clone().database(&name);
        let pool = PgPool::connect_with(options)
            .await
            .expect("the test's own database");
        crate::db::migrations::run(&pool).await.expect("the schema");
        Self { pool, admin, name }
    }

    pub async fn remove(self) {
        self.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await
        .expect("the test's database to be dropped");
    }
}
