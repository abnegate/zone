//! Session database queries
//!
//! Provides functions for managing user sessions, including creation, validation,
//! revocation, and cleanup of expired sessions.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Session row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token_hash: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_info: Option<JsonValue>,
    pub last_active_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Create a new session
///
/// Creates a session record for tracking user authentication across devices.
/// Sessions are tied to refresh tokens and include metadata about the client.
pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    refresh_token_hash: &str,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
    device_info: Option<JsonValue>,
    expires_at: NaiveDateTime,
) -> DbResult<Session> {
    let session: Session = sqlx::query_as(
        r#"
        INSERT INTO sessions (
            user_id,
            refresh_token_hash,
            ip_address,
            user_agent,
            device_info,
            expires_at
        )
        VALUES ($1, $2, $3::inet, $4, $5, $6)
        RETURNING
            id,
            user_id,
            refresh_token_hash,
            CAST(ip_address AS text) as "ip_address",
            user_agent,
            device_info,
            last_active_at,
            expires_at,
            revoked_at,
            created_at
        "#,
    )
    .bind(user_id)
    .bind(refresh_token_hash)
    .bind(ip_address)
    .bind(user_agent)
    .bind(device_info)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(session)
}

/// Get session by refresh token hash
///
/// Finds an active session by its refresh token hash. Returns None if the
/// session doesn't exist, has expired, or has been revoked.
pub async fn get_session_by_token(
    pool: &PgPool,
    refresh_token_hash: &str,
) -> DbResult<Option<Session>> {
    let session: Option<Session> = sqlx::query_as(
        r#"
        SELECT
            id,
            user_id,
            refresh_token_hash,
            CAST(ip_address AS text) as "ip_address",
            user_agent,
            device_info,
            last_active_at,
            expires_at,
            revoked_at,
            created_at
        FROM sessions
        WHERE refresh_token_hash = $1
          AND revoked_at IS NULL
          AND expires_at > NOW()
        "#,
    )
    .bind(refresh_token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(session)
}

/// Update last active timestamp for a session
///
/// Updates the last_active_at field to track session activity.
pub async fn update_last_active(pool: &PgPool, session_id: Uuid) -> DbResult<()> {
    sqlx::query(
        r#"
        UPDATE sessions
        SET last_active_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(session_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Revoke a specific session
///
/// Marks a session as revoked, preventing further use of its refresh token.
pub async fn revoke_session(pool: &PgPool, session_id: Uuid) -> DbResult<()> {
    sqlx::query(
        r#"
        UPDATE sessions
        SET revoked_at = NOW()
        WHERE id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(session_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Revoke all sessions for a user
///
/// Revokes all active sessions for a user (logout everywhere).
/// Returns the count of sessions that were revoked.
pub async fn revoke_all_user_sessions(pool: &PgPool, user_id: Uuid) -> DbResult<i64> {
    let result = sqlx::query(
        r#"
        UPDATE sessions
        SET revoked_at = NOW()
        WHERE user_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() as i64)
}

/// List all sessions for a user
///
/// Returns all sessions (active and revoked) for a user, ordered by creation date.
pub async fn list_user_sessions(pool: &PgPool, user_id: Uuid) -> DbResult<Vec<Session>> {
    let sessions: Vec<Session> = sqlx::query_as(
        r#"
        SELECT
            id,
            user_id,
            refresh_token_hash,
            CAST(ip_address AS text) as "ip_address",
            user_agent,
            device_info,
            last_active_at,
            expires_at,
            revoked_at,
            created_at
        FROM sessions
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

/// List active (non-revoked) sessions for a user
///
/// Returns only active sessions that have not expired or been revoked.
pub async fn list_active_user_sessions(pool: &PgPool, user_id: Uuid) -> DbResult<Vec<Session>> {
    let sessions: Vec<Session> = sqlx::query_as(
        r#"
        SELECT
            id,
            user_id,
            refresh_token_hash,
            CAST(ip_address AS text) as "ip_address",
            user_agent,
            device_info,
            last_active_at,
            expires_at,
            revoked_at,
            created_at
        FROM sessions
        WHERE user_id = $1
            AND revoked_at IS NULL
            AND expires_at > NOW()
        ORDER BY last_active_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

/// Check if a session belongs to a user
///
/// Returns true if the session exists and belongs to the specified user.
pub async fn is_user_session(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let result: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM sessions WHERE id = $1 AND user_id = $2")
            .bind(session_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    Ok(result.is_some())
}

/// Cleanup expired sessions
///
/// Deletes sessions that have expired or been revoked.
/// Returns the count of sessions that were deleted.
pub async fn cleanup_expired_sessions(pool: &PgPool) -> DbResult<i64> {
    let result = sqlx::query(
        r#"
        DELETE FROM sessions
        WHERE expires_at < NOW() OR revoked_at IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_struct() {
        // Ensure Session struct can be instantiated (compile-time check)
        let _session = Session {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            refresh_token_hash: "test".to_string(),
            ip_address: Some("127.0.0.1".to_string()),
            user_agent: Some("test".to_string()),
            device_info: None,
            last_active_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now(),
            revoked_at: None,
            created_at: chrono::Utc::now(),
        };
    }
}
