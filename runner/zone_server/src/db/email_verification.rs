//! Email verification database queries
//!
//! Handles creation, validation, and cleanup of email verification tokens.
//! Tokens are hashed before storage for security.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;
use crate::utils::crypto::{generate_token, hash_token};

const TOKEN_EXPIRY_HOURS: i64 = 24;

/// Create a new email verification token for a user
///
/// This function:
/// 1. Generates a unique random token
/// 2. Hashes the token for secure storage
/// 3. Deletes any existing tokens for the user (invalidates old tokens)
/// 4. Creates a new token with 24-hour expiry
///
/// Returns the plain token (to send in email) and expiration timestamp
pub async fn create_verification_token(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<(String, DateTime<Utc>)> {
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

    // Delete any existing tokens for this user (invalidate old tokens)
    sqlx::query!(
        "DELETE FROM email_verification_tokens WHERE user_id = $1",
        user_id
    )
    .execute(pool)
    .await?;

    // Insert new token hash
    sqlx::query!(
        r#"
        INSERT INTO email_verification_tokens (user_id, token_hash, expires_at)
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

/// Verify an email verification token
///
/// This function:
/// 1. Hashes the provided token
/// 2. Checks if a matching hash exists in the database
/// 3. Verifies the token is not expired
/// 4. Deletes the token (single-use)
/// 5. Returns the user_id
///
/// Note: DELETE...RETURNING is atomic, so no transaction is needed.
///
/// Returns an error if token is invalid, expired, or already used
pub async fn verify_token(pool: &PgPool, token: &str) -> DbResult<Uuid> {
    let token_hash = hash_token(token);

    // Get and delete the token in one atomic query
    let row = sqlx::query!(
        r#"
        DELETE FROM email_verification_tokens
        WHERE token_hash = $1 AND expires_at > NOW()
        RETURNING user_id
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await?;

    let user_id = row.ok_or(sqlx::Error::RowNotFound)?.user_id;

    Ok(user_id)
}

/// Mark a user's email as verified
///
/// Sets the email_verified flag to true for the given user
pub async fn mark_email_verified(pool: &PgPool, user_id: Uuid) -> DbResult<()> {
    let result = sqlx::query!(
        r#"
        UPDATE users
        SET email_verified = TRUE, updated_at = NOW()
        WHERE id = $1
        "#,
        user_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Delete all expired verification tokens
///
/// This should be called periodically (e.g., via a cron job) to clean up old tokens.
/// Returns the number of deleted tokens.
pub async fn delete_expired_tokens(pool: &PgPool) -> DbResult<u64> {
    let result = sqlx::query!("DELETE FROM email_verification_tokens WHERE expires_at < NOW()")
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}
