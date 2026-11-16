use sqlx::{PgExecutor, Postgres, Transaction};

use crate::db::DbResult;

pub async fn required(executor: impl PgExecutor<'_>) -> DbResult<bool> {
    let required: Option<bool> = sqlx::query_scalar("SELECT required FROM sso_settings WHERE id")
        .fetch_optional(executor)
        .await?;
    Ok(required.unwrap_or(false))
}

/// Every change that could leave enforcement on with no enabled provider takes this lock before
/// counting providers, so two such changes cannot both pass the count.
pub async fn lock(transaction: &mut Transaction<'_, Postgres>) -> DbResult<()> {
    sqlx::query("LOCK TABLE sso_settings IN EXCLUSIVE MODE")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

/// Returns the previous value.
pub async fn set_required(
    transaction: &mut Transaction<'_, Postgres>,
    required: bool,
) -> DbResult<bool> {
    let previous: Option<bool> = sqlx::query_scalar(
        "WITH previous AS (SELECT required FROM sso_settings WHERE id) \
         INSERT INTO sso_settings (id, required, updated_at) VALUES (true, $1, now()) \
         ON CONFLICT (id) DO UPDATE SET required = EXCLUDED.required, updated_at = now() \
         RETURNING (SELECT required FROM previous)",
    )
    .bind(required)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(previous.unwrap_or(false))
}
