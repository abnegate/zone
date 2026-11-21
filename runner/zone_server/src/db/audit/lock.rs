use sqlx::pool::PoolConnection;
use sqlx::{PgConnection, PgPool, Postgres};

use crate::db::DbResult;

/// A session-level advisory lock held on a connection of its own. The connection is closed
/// rather than returned to the pool, on release or on drop, so the lock never outlives its
/// holder on a pooled connection, even when the holder is cancelled.
pub struct Lock {
    key: i64,
    connection: PoolConnection<Postgres>,
}

impl Lock {
    /// Waits until no other session holds `key`.
    pub async fn acquire(pool: &PgPool, key: i64) -> DbResult<Self> {
        let mut connection = open(pool).await?;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(key)
            .execute(&mut *connection)
            .await?;
        Ok(Self { key, connection })
    }

    /// None when another session holds `key`.
    pub async fn attempt(pool: &PgPool, key: i64) -> DbResult<Option<Self>> {
        let mut connection = open(pool).await?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *connection)
            .await?;
        Ok(acquired.then_some(Self { key, connection }))
    }

    pub fn connection(&mut self) -> &mut PgConnection {
        &mut self.connection
    }

    /// Unlocks before closing: a closed session's locks are freed only once its backend exits,
    /// after this would return.
    pub async fn release(mut self) -> DbResult<()> {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(self.key)
            .execute(&mut *self.connection)
            .await?;
        self.connection.close().await
    }
}

async fn open(pool: &PgPool) -> DbResult<PoolConnection<Postgres>> {
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    Ok(connection)
}
