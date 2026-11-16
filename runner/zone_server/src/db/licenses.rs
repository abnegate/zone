//! The server's one stored license key

use sqlx::{Executor, Postgres};

use super::DbResult;

const SINGLETON: i16 = 1;

pub async fn get<'e, E>(executor: E) -> DbResult<Option<String>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar("SELECT key FROM licenses WHERE id = $1")
        .bind(SINGLETON)
        .fetch_optional(executor)
        .await
}

pub async fn set<'e, E>(executor: E, key: &str) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO licenses (id, key) VALUES ($1, $2)
         ON CONFLICT (id) DO UPDATE SET key = EXCLUDED.key, updated_at = now()",
    )
    .bind(SINGLETON)
    .bind(key)
    .execute(executor)
    .await?;
    Ok(())
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
