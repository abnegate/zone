//! How far LiteLLM's spend logs have been ingested.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::db::DbResult;

pub async fn read(pool: &PgPool) -> DbResult<Option<DateTime<Utc>>> {
    sqlx::query_scalar("SELECT through FROM spend_cursor WHERE id")
        .fetch_optional(pool)
        .await
}

pub async fn advance(pool: &PgPool, through: DateTime<Utc>) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO spend_cursor (id, through) VALUES (true, $1)
         ON CONFLICT (id) DO UPDATE SET through = EXCLUDED.through, updated_at = now()",
    )
    .bind(through)
    .execute(pool)
    .await?;
    Ok(())
}
