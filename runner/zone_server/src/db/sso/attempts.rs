use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;
use zone_enterprise::sso::Authorization;

use super::asserted::Asserted;
use super::attempt::Attempt;
use crate::db::DbResult;
use crate::utils::crypto::hash_token;

pub const LIFETIME: TimeDelta = TimeDelta::minutes(10);
pub const HANDOFF_LIFETIME: TimeDelta = TimeDelta::minutes(2);
pub const CAPACITY: i64 = 10_000;

const COLUMNS: &str = "id, provider_id, nonce, code_verifier AS verifier, request_id, code_hash, subject, email, display_name, expires_at, created_at";

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    provider_id: Uuid,
    nonce: Option<String>,
    verifier: Option<String>,
    request_id: Option<String>,
    code_hash: Option<String>,
    subject: Option<String>,
    email: Option<String>,
    display_name: Option<String>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl From<Row> for Attempt {
    fn from(row: Row) -> Self {
        let asserted = match (row.code_hash, row.subject, row.email) {
            (Some(code_hash), Some(subject), Some(email)) => Some(Asserted {
                code_hash,
                subject,
                email,
                display_name: row.display_name,
            }),
            _ => None,
        };
        Self {
            id: row.id,
            provider_id: row.provider_id,
            nonce: row.nonce,
            verifier: row.verifier,
            request_id: row.request_id,
            asserted,
            expires_at: row.expires_at,
            created_at: row.created_at,
        }
    }
}

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

/// The SAML counterpart of [`issue`], bounded by the same [`CAPACITY`].
pub async fn issue_request(
    executor: impl PgExecutor<'_>,
    provider_id: Uuid,
    state: &str,
    request_id: &str,
) -> DbResult<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "WITH pruned AS (DELETE FROM sso_attempts WHERE expires_at <= now()) \
         INSERT INTO sso_attempts (state_hash, provider_id, request_id, expires_at) \
         SELECT $1, $2, $3, now() + make_interval(secs => $4) \
         WHERE (SELECT count(*) FROM sso_attempts WHERE expires_at > now()) < $5 \
         RETURNING expires_at",
    )
    .bind(hash_token(state))
    .bind(provider_id)
    .bind(request_id)
    .bind(LIFETIME.as_seconds_f64())
    .bind(CAPACITY)
    .fetch_optional(executor)
    .await
}

/// `None` unless the request is live and no post has claimed it yet: the first post for a state
/// takes it, so posts racing with the same state can't each have their response checked.
pub async fn claim(executor: impl PgExecutor<'_>, state: &str) -> DbResult<Option<Attempt>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE sso_attempts SET claimed_at = now() \
         WHERE state_hash = $1 AND expires_at > now() \
           AND request_id IS NOT NULL AND claimed_at IS NULL \
         RETURNING {COLUMNS}"
    )))
    .bind(hash_token(state))
    .fetch_optional(executor)
    .await?;
    Ok(row.map(Attempt::from))
}

/// False when the attempt is gone, expired, unclaimed or already asserted: a response is accepted
/// once, and only from the post that claimed the attempt.
pub async fn assert(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    code: &str,
    subject: &str,
    email: &str,
    display_name: Option<&str>,
) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE sso_attempts \
         SET code_hash = $2, subject = $3, email = $4, display_name = $5, \
             expires_at = least(expires_at, now() + make_interval(secs => $6)) \
         WHERE id = $1 AND claimed_at IS NOT NULL AND code_hash IS NULL AND expires_at > now()",
    )
    .bind(id)
    .bind(hash_token(code))
    .bind(subject)
    .bind(email)
    .bind(display_name)
    .bind(HANDOFF_LIFETIME.as_seconds_f64())
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Leaves an asserted attempt alone: its handoff is the console's to redeem.
pub async fn discard(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM sso_attempts WHERE id = $1 AND code_hash IS NULL")
        .bind(id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Every attempt still open for a provider, asserted or not, checked against settings the
/// provider no longer has.
pub async fn clear(executor: impl PgExecutor<'_>, provider_id: Uuid) -> DbResult<u64> {
    let result = sqlx::query("DELETE FROM sso_attempts WHERE provider_id = $1")
        .bind(provider_id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected())
}

pub async fn consume(executor: impl PgExecutor<'_>, state: &str) -> DbResult<Option<Attempt>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "DELETE FROM sso_attempts WHERE state_hash = $1 AND expires_at > now() RETURNING {COLUMNS}"
    )))
    .bind(hash_token(state))
    .fetch_optional(executor)
    .await?;
    Ok(row.map(Attempt::from))
}
