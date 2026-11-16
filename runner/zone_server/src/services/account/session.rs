use axum::http::HeaderMap;
use chrono::Utc;
use uuid::Uuid;

use super::auth_response::AuthResponse;
use super::refusal::Refusal;
use super::tokens::Tokens;
use crate::auth::{create_refresh_token, create_session_access_token};
use crate::db::devices::{self, Device};
use crate::db::{refresh_tokens, sessions, users};
use crate::state::AppState;
use crate::utils::crypto::hash_token;

pub async fn issue(
    state: &AppState,
    user_id: Uuid,
    headers: &HeaderMap,
) -> Result<AuthResponse, Refusal> {
    let user = match users::get_user_with_permissions(state.db(), user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err(Refusal::Missing),
        Err(error) => {
            tracing::error!(%error, "Failed to load the signed-in user");
            return Err(Refusal::Unavailable);
        }
    };
    let device = admit_device(state, user_id, headers, None).await?;
    let tokens = generate_tokens(state, &user, headers, &device, None).await?;
    Ok(AuthResponse::new(
        &user,
        tokens,
        state.config().jwt_access_lifetime,
    ))
}

pub async fn admit_device(
    state: &AppState,
    user_id: Uuid,
    headers: &HeaderMap,
    existing_device_id: Option<Uuid>,
) -> Result<Device, Refusal> {
    let mut claim = crate::auth::device::claim(headers);
    if claim.public_id.is_none()
        && let Some(device_id) = existing_device_id
    {
        match devices::get(state.db(), device_id).await {
            Ok(Some(device)) => {
                claim.public_id = Some(device.public_id);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::error!(%error, "Failed to load the session device");
                return Err(Refusal::Unavailable);
            }
        }
    }

    match devices::admit(state.db(), user_id, &claim).await {
        Ok(devices::Admission::Allowed(device)) => Ok(device),
        Ok(devices::Admission::Pending(_)) => Err(Refusal::DevicePending),
        Ok(devices::Admission::Blocked(_)) => Err(Refusal::DeviceBlocked),
        Err(error) => {
            tracing::error!(%error, "Failed to admit a device");
            Err(Refusal::Unavailable)
        }
    }
}

/// Issue a fresh refresh token and an access token bound to a session: a new
/// session for a sign-in, or the caller's existing one when `session` names it.
pub async fn generate_tokens(
    state: &AppState,
    user: &users::UserWithPermissions,
    headers: &HeaderMap,
    device: &Device,
    session: Option<Uuid>,
) -> Result<Tokens, Refusal> {
    let config = state.config();
    if let Some(zone) = crate::auth::time_zone::claim(headers)
        && let Err(error) = users::set_time_zone(state.db(), user.user.id, zone).await
    {
        tracing::warn!(user_id = %user.user.id, %error, "Failed to record the console's time zone");
    }
    let claim = crate::auth::device::claim(headers);
    let ip_address = claim.ip_address.as_deref();
    let user_agent = claim.user_agent.as_deref();
    let device_info = crate::auth::device::info(&claim, device);

    let refresh_token = create_refresh_token(
        user.user.id,
        config.jwt_secret(),
        config.refresh_token_lifetime(),
    )
    .map_err(|error| {
        tracing::error!(%error, "Token creation error");
        Refusal::Unavailable
    })?;

    let token_hash = hash_token(&refresh_token);
    let expires_at = Utc::now() + config.refresh_token_lifetime();

    if let Err(error) = refresh_tokens::create_refresh_token(
        state.db(),
        user.user.id,
        &token_hash,
        expires_at.naive_utc(),
        user_agent,
        ip_address,
    )
    .await
    {
        tracing::error!(%error, "Failed to store refresh token");
        return Err(Refusal::Unavailable);
    }

    let session = match session {
        Some(session_id) => {
            sessions::rotate_session(state.db(), session_id, &token_hash, expires_at.naive_utc())
                .await
                .and_then(|rotated| rotated.ok_or(sqlx::Error::RowNotFound))
        }
        None => {
            sessions::create_session(
                state.db(),
                user.user.id,
                &token_hash,
                ip_address,
                user_agent,
                Some(device_info),
                expires_at.naive_utc(),
            )
            .await
        }
    };
    let session = match session {
        Ok(session) => session,
        Err(error) => {
            tracing::error!(%error, "Failed to bind session");
            let _ = refresh_tokens::revoke_refresh_token(state.db(), &token_hash).await;
            return Err(Refusal::Unavailable);
        }
    };
    if let Err(error) = sessions::attach_device(state.db(), session.id, device.id).await {
        tracing::error!(%error, "Failed to bind the session to its device");
        let _ = refresh_tokens::revoke_refresh_token(state.db(), &token_hash).await;
        return Err(Refusal::Unavailable);
    }

    let access_token = create_session_access_token(
        user.user.id,
        &user.user.email,
        user.roles.clone(),
        user.permissions.clone(),
        user.user.is_admin.unwrap_or(false),
        session.id,
        config.jwt_secret(),
        config.access_token_lifetime(),
    )
    .map_err(|error| {
        tracing::error!(%error, "Token creation error");
        Refusal::Unavailable
    })?;

    Ok(Tokens {
        access: access_token,
        refresh: refresh_token,
    })
}
