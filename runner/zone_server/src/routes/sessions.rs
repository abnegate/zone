//! Session management endpoints
//!
//! Provides API routes for users to view and manage their active sessions.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{AuthSession, AuthUser};
use crate::db::sessions;
use crate::error::ServerError;
use crate::state::AppState;

use super::common::Timestamps;

/// Session information response
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub device_info: Option<String>,
    pub location: Option<String>,
    pub last_active_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub timestamps: Timestamps,
    pub is_current: bool,
}

impl SessionResponse {
    fn from_session(session: sessions::Session, current_session_id: Uuid) -> Self {
        Self {
            id: session.id,
            user_id: session.user_id,
            ip_address: session.ip_address,
            user_agent: session.user_agent,
            device_info: session.device_info.as_ref().map(describe_device),
            location: None,
            last_active_at: session.last_active_at,
            expires_at: session.expires_at,
            revoked_at: session.revoked_at,
            timestamps: Timestamps::from_utc(session.created_at, session.created_at),
            is_current: current_session_id == session.id,
        }
    }
}

fn describe_device(info: &serde_json::Value) -> String {
    match info {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(fields) => fields
            .values()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>()
            .join(" · "),
        other => other.to_string(),
    }
}

/// List sessions response
#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionResponse>,
}

/// Revoke sessions response
#[derive(Debug, Serialize)]
pub struct RevokeSessionsResponse {
    pub message: String,
    pub revoked_count: i64,
}

/// GET /api/auth/sessions
///
/// List all active sessions for the current user.
pub async fn list_sessions(
    State(state): State<AppState>,
    auth: AuthSession,
) -> Result<impl IntoResponse, ServerError> {
    let user_id = auth.claims.user_id().map_err(|e| {
        tracing::error!("Invalid user ID in JWT: {}", e);
        ServerError::BadRequest("Invalid user ID".to_string())
    })?;

    let sessions = sessions::list_active_user_sessions(state.db(), user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list sessions for user {}: {}", user_id, e);
            ServerError::Internal("Failed to retrieve sessions".to_string())
        })?;

    let session_responses: Vec<SessionResponse> = sessions
        .into_iter()
        .map(|s| SessionResponse::from_session(s, auth.session_id))
        .collect();

    Ok(Json(ListSessionsResponse {
        sessions: session_responses,
    }))
}

/// DELETE /api/auth/sessions/:session_id
///
/// Revoke a specific session for the current user.
pub async fn revoke_session(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(session_id): Path<Uuid>,
) -> Result<impl IntoResponse, ServerError> {
    let user_id = user.user_id().map_err(|e| {
        tracing::error!("Invalid user ID in JWT: {}", e);
        ServerError::BadRequest("Invalid user ID".to_string())
    })?;

    let session_belongs_to_user = sessions::is_user_session(state.db(), session_id, user_id)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to verify session ownership for user {}: {}",
                user_id,
                e
            );
            ServerError::Internal("Failed to verify session ownership".to_string())
        })?;

    if !session_belongs_to_user {
        return Err(ServerError::NotFound(
            "Session not found or does not belong to you".to_string(),
        ));
    }

    sessions::revoke_session(state.db(), session_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to revoke session {}: {}", session_id, e);
            ServerError::Internal("Failed to revoke session".to_string())
        })?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Session revoked successfully"
        })),
    ))
}

/// DELETE /api/auth/sessions
///
/// Revoke every session of the current user except the one making the call.
pub async fn revoke_all_sessions(
    State(state): State<AppState>,
    auth: AuthSession,
) -> Result<impl IntoResponse, ServerError> {
    let user_id = auth.claims.user_id().map_err(|e| {
        tracing::error!("Invalid user ID in JWT: {}", e);
        ServerError::BadRequest("Invalid user ID".to_string())
    })?;

    let revoked_count = sessions::revoke_other_user_sessions(state.db(), user_id, auth.session_id)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to revoke other sessions for user {}: {}",
                user_id,
                e
            );
            ServerError::Internal("Failed to revoke sessions".to_string())
        })?;

    Ok(Json(RevokeSessionsResponse {
        message: "All other sessions revoked successfully".to_string(),
        revoked_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_response_serialization() {
        use chrono::Utc;

        let session = sessions::Session {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            refresh_token_hash: "test".to_string(),
            ip_address: Some("192.168.1.1".to_string()),
            user_agent: Some("Test Browser".to_string()),
            device_info: Some(serde_json::json!({"device": "Desktop"})),
            last_active_at: Utc::now(),
            expires_at: Utc::now(),
            revoked_at: None,
            created_at: Utc::now(),
        };

        let response = SessionResponse::from_session(session.clone(), session.id);
        assert!(response.is_current);
        assert_eq!(response.user_id, session.user_id);
        assert_eq!(response.device_info.as_deref(), Some("Desktop"));

        let json: serde_json::Value = serde_json::to_value(&response).expect("Failed to serialize");
        assert!(json["ip_address"].is_string());
        assert!(json["user_id"].is_string());
        assert!(json["location"].is_null());
    }
}
