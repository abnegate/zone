//! Refresh token database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Refresh token row from database
#[derive(Debug, Clone)]
pub struct RefreshTokenRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
    pub created_at: Option<NaiveDateTime>,
    pub revoked_at: Option<NaiveDateTime>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

/// Create a new refresh token
pub async fn create_refresh_token(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &str,
    expires_at: NaiveDateTime,
    user_agent: Option<&str>,
    ip_address: Option<&str>,
) -> DbResult<RefreshTokenRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at, user_agent, ip_address)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id, token_hash, expires_at, created_at, revoked_at, user_agent, ip_address
        "#,
        user_id,
        token_hash,
        expires_at,
        user_agent,
        ip_address
    )
    .fetch_one(pool)
    .await?;

    Ok(RefreshTokenRow {
        id: row.id,
        user_id: row.user_id,
        token_hash: row.token_hash,
        expires_at: row.expires_at,
        created_at: row.created_at,
        revoked_at: row.revoked_at,
        user_agent: row.user_agent,
        ip_address: row.ip_address,
    })
}

/// Validate a refresh token (returns user_id if valid)
pub async fn validate_refresh_token(pool: &PgPool, token_hash: &str) -> DbResult<Option<Uuid>> {
    let result = sqlx::query_scalar!(
        r#"
        SELECT user_id
        FROM refresh_tokens
        WHERE token_hash = $1
          AND expires_at > NOW()
          AND revoked_at IS NULL
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await?;

    Ok(result)
}

/// Revoke a refresh token
pub async fn revoke_refresh_token(pool: &PgPool, token_hash: &str) -> DbResult<bool> {
    let result = sqlx::query!(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = NOW()
        WHERE token_hash = $1 AND revoked_at IS NULL
        "#,
        token_hash
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Revoke all refresh tokens for a user
pub async fn revoke_all_user_tokens(pool: &PgPool, user_id: Uuid) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = NOW()
        WHERE user_id = $1 AND revoked_at IS NULL
        "#,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Count active tokens for a user
pub async fn count_user_tokens(pool: &PgPool, user_id: Uuid) -> DbResult<i64> {
    let result = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)
        FROM refresh_tokens
        WHERE user_id = $1 AND expires_at > NOW() AND revoked_at IS NULL
        "#,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(result.unwrap_or(0))
}

/// Cleanup expired tokens
pub async fn cleanup_expired_tokens(pool: &PgPool) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"
        DELETE FROM refresh_tokens
        WHERE expires_at < NOW() OR revoked_at IS NOT NULL
        "#
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
