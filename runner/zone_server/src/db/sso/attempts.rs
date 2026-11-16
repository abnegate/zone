use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;
use zone_enterprise::sso::Authorization;

use super::attempt::Attempt;
use crate::db::DbResult;
use crate::utils::crypto::hash_token;

pub const LIFETIME: TimeDelta = TimeDelta::minutes(10);

/// Only the SHA-256 of the state is stored. Expired attempts are pruned in the same statement.
pub async fn issue(
    executor: impl PgExecutor<'_>,
    provider_id: Uuid,
    authorization: &Authorization,
) -> DbResult<DateTime<Utc>> {
    sqlx::query_scalar(
        "WITH pruned AS (DELETE FROM sso_attempts WHERE expires_at <= now()) \
         INSERT INTO sso_attempts (state_hash, provider_id, nonce, code_verifier, expires_at) \
         VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5)) \
         RETURNING expires_at",
    )
    .bind(hash_token(&authorization.state))
    .bind(provider_id)
    .bind(&authorization.nonce)
    .bind(&authorization.verifier)
    .bind(LIFETIME.as_seconds_f64())
    .fetch_one(executor)
    .await
}

/// Deletes the matching live attempt, so a second callback with the same state finds nothing.
pub async fn consume(executor: impl PgExecutor<'_>, state: &str) -> DbResult<Option<Attempt>> {
    sqlx::query_as(
        "DELETE FROM sso_attempts WHERE state_hash = $1 AND expires_at > now() \
         RETURNING id, provider_id, nonce, code_verifier AS verifier, expires_at, created_at",
    )
    .bind(hash_token(state))
    .fetch_optional(executor)
    .await
}
