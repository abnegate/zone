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

/// A password sign-in passes the hash it verified, so a reclaim that replaced the password since
/// refuses the session instead of leaving the old owner signed in.
pub async fn issue(
    state: &AppState,
    user_id: Uuid,
    headers: &HeaderMap,
    password_hash: Option<&str>,
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
    let tokens = generate_tokens(state, &user, headers, &device, None, password_hash).await?;
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
    password_hash: Option<&str>,
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

    let mut transaction = state
        .db()
        .begin()
        .await
        .map_err(unavailable("Failed to begin a session"))?;
    if let Some(verified) = password_hash {
        let current = users::lock_password_hash(&mut transaction, user.user.id)
            .await
            .map_err(unavailable("Failed to recheck the password"))?;
        if current.as_deref() != Some(verified) {
            return Err(Refusal::Superseded);
        }
    }
    refresh_tokens::create_refresh_token(
        &mut *transaction,
        user.user.id,
        &token_hash,
        expires_at.naive_utc(),
        user_agent,
        ip_address,
    )
    .await
    .map_err(unavailable("Failed to store refresh token"))?;
    let session = match session {
        Some(session_id) => sessions::rotate_session(
            &mut *transaction,
            session_id,
            &token_hash,
            expires_at.naive_utc(),
        )
        .await
        .and_then(|rotated| rotated.ok_or(sqlx::Error::RowNotFound)),
        None => {
            sessions::create_session(
                &mut *transaction,
                user.user.id,
                &token_hash,
                ip_address,
                user_agent,
                Some(device_info),
                expires_at.naive_utc(),
            )
            .await
        }
    }
    .map_err(unavailable("Failed to bind session"))?;
    sessions::attach_device(&mut *transaction, session.id, device.id)
        .await
        .map_err(unavailable("Failed to bind the session to its device"))?;
    transaction
        .commit()
        .await
        .map_err(unavailable("Failed to commit the session"))?;

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

fn unavailable(context: &'static str) -> impl FnOnce(sqlx::Error) -> Refusal {
    move |error| {
        tracing::error!(%error, "{context}");
        Refusal::Unavailable
    }
}
