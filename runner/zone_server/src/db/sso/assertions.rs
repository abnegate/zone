use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::db::DbResult;

/// False when the provider already sent this assertion and it has not yet expired. An expired
/// entry is taken over rather than refused, since the assertion itself is no longer accepted.
pub async fn record(
    executor: impl PgExecutor<'_>,
    provider_id: Uuid,
    assertion_id: &str,
    expires_at: DateTime<Utc>,
) -> DbResult<bool> {
    let recorded: Option<bool> = sqlx::query_scalar(
        "WITH pruned AS ( \
           DELETE FROM sso_assertions WHERE expires_at <= now() \
             AND NOT (provider_id = $1 AND assertion_id = $2)) \
         INSERT INTO sso_assertions (provider_id, assertion_id, expires_at) VALUES ($1, $2, $3) \
         ON CONFLICT (provider_id, assertion_id) DO UPDATE SET expires_at = EXCLUDED.expires_at \
           WHERE sso_assertions.expires_at <= now() \
         RETURNING true",
    )
    .bind(provider_id)
    .bind(assertion_id)
    .bind(expires_at)
    .fetch_optional(executor)
    .await?;
    Ok(recorded.is_some())
}
