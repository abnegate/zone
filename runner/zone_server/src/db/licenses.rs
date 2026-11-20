use sqlx::{Executor, PgPool, Postgres};

use super::DbResult;

pub const SINGLETON: i16 = 1;

pub async fn get<'e, E>(executor: E) -> DbResult<Option<String>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar("SELECT key FROM licenses WHERE id = $1")
        .bind(SINGLETON)
        .fetch_optional(executor)
        .await
}

/// Stores the key and returns the one it replaced. The table lock makes the
/// read and the write one step even when no row exists yet, which a row lock
/// cannot cover.
pub async fn replace(pool: &PgPool, key: &str) -> DbResult<Option<String>> {
    let mut transaction = pool.begin().await?;
    sqlx::query("LOCK TABLE licenses IN EXCLUSIVE MODE")
        .execute(&mut *transaction)
        .await?;
    let previous = get(&mut *transaction).await?;
    sqlx::query(
        "INSERT INTO licenses (id, key) VALUES ($1, $2)
         ON CONFLICT (id) DO UPDATE SET key = EXCLUDED.key, updated_at = now(),
             renewal_attempted_at = NULL, renewal_outcome = NULL",
    )
    .bind(SINGLETON)
    .bind(key)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(previous)
}

/// Stores `key` only while `expected` is still the stored key, so a key an
/// admin installs meanwhile is never overwritten.
pub async fn replace_if(pool: &PgPool, expected: &str, key: &str) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE licenses SET key = $3, updated_at = now(),
             renewal_attempted_at = NULL, renewal_outcome = NULL
         WHERE id = $1 AND key = $2",
    )
    .bind(SINGLETON)
    .bind(expected)
    .bind(key)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn remove<'e, E>(executor: E) -> DbResult<Option<String>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar("DELETE FROM licenses WHERE id = $1 RETURNING key")
        .bind(SINGLETON)
        .fetch_optional(executor)
        .await
}
