//! Password reset database queries
//!
//! Handles creation, validation, and cleanup of password reset tokens.
//! Tokens are hashed before storage for security.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;
use crate::utils::crypto::{generate_token, hash_token};

const TOKEN_EXPIRY_HOURS: i64 = 1; // Password reset tokens expire in 1 hour

/// Create a new password reset token for a user
///
/// This function:
/// 1. Generates a unique random token
/// 2. Hashes the token for secure storage
/// 3. Stores the hash with 1-hour expiry
///
/// Note: Unlike email verification, we don't delete old tokens.
/// Multiple reset tokens can be active simultaneously (e.g., if user requests multiple times).
/// Old tokens are cleaned up by the delete_expired_tokens function.
///
/// Returns the plain token (to send in email) and expiration timestamp
pub async fn create_reset_token(pool: &PgPool, user_id: Uuid) -> DbResult<(String, DateTime<Utc>)> {
    let token = generate_token();
    let token_hash = hash_token(&token);
    let expires_at = Utc::now() + Duration::hours(TOKEN_EXPIRY_HOURS);

    // Verify user exists
    let user_exists =
        sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)", user_id)
            .fetch_one(pool)
            .await?;

    if !user_exists.unwrap_or(false) {
        return Err(sqlx::Error::RowNotFound);
    }

    // Insert new token (allow multiple active tokens)
    sqlx::query!(
        r#"
        INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
        VALUES ($1, $2, $3)
        "#,
        user_id,
        token_hash,
        expires_at
    )
    .execute(pool)
    .await?;

    Ok((token, expires_at))
}

/// Verify and consume a password reset token atomically
///
/// This function:
/// 1. Hashes the provided token
/// 2. Checks if a matching hash exists in the database
/// 3. Verifies the token is not expired
/// 4. Verifies the token has not been used
/// 5. Marks the token as used (atomically)
/// 6. Returns the user_id
///
/// This is an atomic operation that prevents race conditions where
/// the same token could be used multiple times.
///
/// Returns an error if token is invalid, expired, or already used
pub async fn verify_and_consume_reset_token(pool: &PgPool, token: &str) -> DbResult<Uuid> {
    let token_hash = hash_token(token);

    let row = sqlx::query!(
        r#"
        UPDATE password_reset_tokens
        SET used_at = NOW()
        WHERE token_hash = $1
          AND expires_at > NOW()
          AND used_at IS NULL
        RETURNING user_id
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await?;

    let user_id = row.ok_or(sqlx::Error::RowNotFound)?.user_id;

    Ok(user_id)
}

/// Verify a password reset token
///
/// This function:
/// 1. Hashes the provided token
/// 2. Checks if a matching hash exists in the database
/// 3. Verifies the token is not expired
/// 4. Verifies the token has not been used
/// 5. Returns the user_id
///
/// Note: This does NOT consume the token. The token must be explicitly
/// marked as used via mark_token_used() after password is updated.
/// For atomic verification and consumption, use verify_and_consume_reset_token().
///
/// Returns an error if token is invalid, expired, or already used
pub async fn verify_reset_token(pool: &PgPool, token: &str) -> DbResult<Uuid> {
    let token_hash = hash_token(token);

    let row = sqlx::query!(
        r#"
        SELECT user_id
        FROM password_reset_tokens
        WHERE token_hash = $1
          AND expires_at > NOW()
          AND used_at IS NULL
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await?;

    let user_id = row.ok_or(sqlx::Error::RowNotFound)?.user_id;

    Ok(user_id)
}

/// Mark a password reset token as used
///
/// This should be called after the password has been successfully updated.
/// Once marked as used, the token cannot be used again.
/// This operation is idempotent - marking an already-used token succeeds.
pub async fn mark_token_used(pool: &PgPool, token: &str) -> DbResult<()> {
    let token_hash = hash_token(token);

    // Check if token exists first
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM password_reset_tokens WHERE token_hash = $1)",
        token_hash
    )
    .fetch_one(pool)
    .await?;

    if !exists.unwrap_or(false) {
        return Err(sqlx::Error::RowNotFound);
    }

    // Mark as used (idempotent - doesn't fail if already used)
    sqlx::query!(
        r#"
        UPDATE password_reset_tokens
        SET used_at = COALESCE(used_at, NOW())
        WHERE token_hash = $1
        "#,
        token_hash
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Delete expired and old used password reset tokens
///
/// This function removes:
/// 1. All tokens that have expired (past expires_at)
/// 2. Used tokens older than 24 hours
///
/// This should be called periodically (e.g., via a cron job) to clean up old tokens.
/// Returns the number of deleted tokens.
pub async fn delete_expired_tokens(pool: &PgPool) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"
        DELETE FROM password_reset_tokens
        WHERE expires_at < NOW()
           OR (used_at IS NOT NULL AND used_at < NOW() - INTERVAL '24 hours')
        "#
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
