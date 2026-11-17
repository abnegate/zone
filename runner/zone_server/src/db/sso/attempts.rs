use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;
use zone_enterprise::sso::Authorization;

use super::attempt::Attempt;
use crate::db::DbResult;
use crate::utils::crypto::hash_token;

pub const LIFETIME: TimeDelta = TimeDelta::minutes(10);
pub const CAPACITY: i64 = 10_000;

/// `None` when [`CAPACITY`] sign-ins are already waiting for their callback: starting one needs no
/// account, so the table would otherwise grow as fast as anyone can call.
pub async fn issue(
    executor: impl PgExecutor<'_>,
    provider_id: Uuid,
    authorization: &Authorization,
) -> DbResult<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "WITH pruned AS (DELETE FROM sso_attempts WHERE expires_at <= now()) \
         INSERT INTO sso_attempts (state_hash, provider_id, nonce, code_verifier, expires_at) \
         SELECT $1, $2, $3, $4, now() + make_interval(secs => $5) \
         WHERE (SELECT count(*) FROM sso_attempts WHERE expires_at > now()) < $6 \
         RETURNING expires_at",
    )
    .bind(hash_token(&authorization.state))
    .bind(provider_id)
    .bind(&authorization.nonce)
    .bind(&authorization.verifier)
    .bind(LIFETIME.as_seconds_f64())
    .bind(CAPACITY)
    .fetch_optional(executor)
    .await
}

pub async fn consume(executor: impl PgExecutor<'_>, state: &str) -> DbResult<Option<Attempt>> {
    sqlx::query_as(
        "DELETE FROM sso_attempts WHERE state_hash = $1 AND expires_at > now() \
         RETURNING id, provider_id, nonce, code_verifier AS verifier, expires_at, created_at",
    )
    .bind(hash_token(state))
    .fetch_optional(executor)
    .await
}
