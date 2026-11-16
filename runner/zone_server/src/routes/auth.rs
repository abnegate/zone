//! Authentication endpoints

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::auth::{AuthUser, account, hash_password, verify_password};
use crate::db::DbResult;
use crate::db::users::UserRow;
use crate::db::{refresh_tokens, sessions, users};
use crate::services::account::{AuthResponse, Refusal, provision, session};
use crate::services::mail::Template;
use crate::state::AppState;
use crate::utils::crypto::hash_token;

use super::common::ErrorResponse;

const LOGIN_OK: &str = "ok";
const LOGIN_DISABLED: &str = "disabled";
const LOGIN_ERROR: &str = "error";
const LOGIN_INVALID_CREDENTIALS: &str = "invalid_credentials";

/// Register request
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    email: String,
    password: String,
    display_name: Option<String>,
}

/// Login request
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

/// Refresh request
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    refresh_token: String,
}

/// POST /api/auth/register
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    // RFC 5322 simplified: local@domain with basic constraints
    let email_regex = regex::Regex::new(
        r"^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$"
    ).unwrap();

    if !email_regex.is_match(&req.email) || req.email.len() > 254 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid email format")),
        )
            .into_response();
    }

    if req.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Password must be at least 8 characters")),
        )
            .into_response();
    }

    let has_uppercase = req.password.chars().any(|c| c.is_uppercase());
    let has_lowercase = req.password.chars().any(|c| c.is_lowercase());
    let has_digit = req.password.chars().any(|c| c.is_ascii_digit());

    if !has_uppercase || !has_lowercase || !has_digit {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "Password must contain at least one uppercase letter, one lowercase letter, and one number"
            )),
        )
            .into_response();
    }

    match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(ErrorResponse::new("Email already registered")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
        Ok(None) => {}
    }

    let password_hash = match hash_password(&req.password) {
        Ok(hash) => hash,
        Err(e) => {
            tracing::error!("Password hashing error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    let is_admin = match users::count_users(state.db()).await {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    let user = match insert(&state, &req, &password_hash, is_admin).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    provision::furnish(state.db(), &user).await;

    match session::issue(&state, user.id, &headers).await {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn insert(
    state: &AppState,
    request: &RegisterRequest,
    password_hash: &str,
    is_admin: bool,
) -> DbResult<UserRow> {
    let mut transaction = state.db().begin().await?;
    let user = provision::insert(
        &mut transaction,
        &request.email,
        password_hash,
        request.display_name.as_deref(),
        is_admin,
        false,
    )
    .await?;
    transaction.commit().await?;
    Ok(user)
}

/// POST /api/auth/login
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let user = match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            crate::metrics::record_login(LOGIN_INVALID_CREDENTIALS);
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid email or password")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            crate::metrics::record_login(LOGIN_ERROR);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    match verify_password(&req.password, &user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            crate::metrics::record_login(LOGIN_INVALID_CREDENTIALS);
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid email or password")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Password verification error: {}", e);
            crate::metrics::record_login(LOGIN_ERROR);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    if !user.is_active.unwrap_or(true) {
        crate::metrics::record_login(LOGIN_DISABLED);
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::with_code(
                "Account is disabled",
                account::DISABLED,
            )),
        )
            .into_response();
    }

    if let Err(e) = users::update_last_login(state.db(), user.id).await {
        tracing::warn!("Failed to update last login: {}", e);
    }

    match session::issue(&state, user.id, &headers).await {
        Ok(response) => {
            crate::metrics::record_login(LOGIN_OK);
            Json(response).into_response()
        }
        Err(refusal) => {
            crate::metrics::record_login(login_outcome(refusal));
            refusal.into_response()
        }
    }
}

fn login_outcome(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::DevicePending | Refusal::DeviceBlocked => LOGIN_DISABLED,
        Refusal::Missing | Refusal::Unavailable => LOGIN_ERROR,
    }
}

/// POST /api/auth/refresh
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RefreshRequest>,
) -> impl IntoResponse {
    let token_hash = hash_token(&req.refresh_token);

    let user_id = match refresh_tokens::validate_refresh_token(state.db(), &token_hash).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid or expired refresh token")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    let current = match sessions::get_session_by_token(state.db(), &token_hash).await {
        Ok(Some(current)) if current.user_id == user_id => current,
        Ok(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Session expired or revoked")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    if let Err(e) = refresh_tokens::revoke_refresh_token(state.db(), &token_hash).await {
        tracing::warn!("Failed to revoke old refresh token: {}", e);
    }

    let user_perms = match users::get_user_with_permissions(state.db(), user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("User not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    let device = match session::admit_device(&state, user_id, &headers, current.device_id).await {
        Ok(device) => device,
        Err(refusal) => return refusal.into_response(),
    };

    match session::generate_tokens(&state, &user_perms, &headers, &device, Some(current.id)).await {
        Ok(tokens) => Json(AuthResponse::new(
            &user_perms,
            tokens,
            state.config().jwt_access_lifetime,
        ))
        .into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

/// POST /api/auth/logout
pub async fn logout(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    let user_id = match auth.0.user_id() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Invalid user ID")),
            )
                .into_response();
        }
    };

    if let Err(e) = refresh_tokens::revoke_all_user_tokens(state.db(), user_id).await {
        tracing::warn!("Failed to revoke refresh tokens: {}", e);
    }

    if let Err(e) = sessions::revoke_all_user_sessions(state.db(), user_id).await {
        tracing::warn!("Failed to revoke sessions: {}", e);
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Request to verify an email address
#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    token: String,
}

/// Request to resend verification email
#[derive(Debug, Deserialize)]
pub struct ResendVerificationRequest {
    email: String,
}

/// POST /api/auth/verify-email
pub async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> impl IntoResponse {
    use crate::db::email_verification;

    let verified = serde_json::json!({
        "success": true,
        "message": "Email verified successfully"
    });

    let user_id = match email_verification::verify_token(state.db(), &req.token).await {
        Ok(id) => id,
        Err(_) => {
            return match email_verification::recently_verified_user(state.db(), &req.token).await {
                Ok(Some(_)) => (StatusCode::OK, Json(verified)).into_response(),
                Ok(None) => (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new("Invalid or expired verification token")),
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!("Failed to look up verification token: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Internal server error")),
                    )
                        .into_response()
                }
            };
        }
    };

    if let Err(e) = email_verification::mark_email_verified(state.db(), user_id).await {
        tracing::error!("Failed to mark email as verified: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    }

    (StatusCode::OK, Json(verified)).into_response()
}

/// The link a verification mail carries: the console's `/verify-email` page.
fn verification_url(base_url: &str, token: &str) -> String {
    format!("{}/verify-email?token={}", base_url, token)
}

/// POST /api/auth/resend-verification
pub async fn resend_verification(
    State(state): State<AppState>,
    Json(req): Json<ResendVerificationRequest>,
) -> impl IntoResponse {
    use crate::db::email_verification;

    let user = match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // Don't reveal whether email exists
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "message": "If the email exists, a verification email has been sent"
                })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Check if already verified - if so, don't send email but return success
    // to avoid leaking verification status
    if user.email_verified {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                    "message": "If the email exists, a verification email has been sent"
            })),
        )
            .into_response();
    }

    let (token, _expires_at) =
        match email_verification::create_verification_token(state.db(), user.id).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Failed to create verification token: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        };

    if let Some(mail) = state.mail() {
        let link = verification_url(&state.config().app_base_url, &token);
        let name = user.display_name.as_deref().unwrap_or(&user.email);

        if mail
            .dispatch(&user.email, &Template::Verification { name, link: &link })
            .is_some()
        {
            tracing::info!("Verification email queued for user_id: {}", user.id);
        }
    } else {
        tracing::debug!(
            "Account mail not configured, verification token created for user_id: {}",
            user.id
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
                    "message": "If the email exists, a verification email has been sent"
        })),
    )
        .into_response()
}

/// Request to initiate password reset
#[derive(Debug, Deserialize)]
pub struct ForgotPasswordRequest {
    email: String,
}

/// Request to reset password with token
#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    token: String,
    new_password: String,
}

/// POST /api/auth/forgot-password
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(req): Json<ForgotPasswordRequest>,
) -> impl IntoResponse {
    use crate::db::password_reset;

    let user = match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // Don't reveal whether email exists for security
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "message": "If the email exists, a password reset email has been sent"
                })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    let (token, _expires_at) = match password_reset::create_reset_token(state.db(), user.id).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to create reset token: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    if let Some(mail) = state.mail() {
        let link = format!(
            "{}/reset-password?token={}",
            state.config().app_base_url,
            token
        );
        let name = user.display_name.as_deref().unwrap_or(&user.email);

        if mail
            .dispatch(&user.email, &Template::PasswordReset { name, link: &link })
            .is_some()
        {
            tracing::info!("Password reset email queued for user_id: {}", user.id);
        }
    } else {
        tracing::debug!(
            "Account mail not configured, password reset token created for user_id: {}",
            user.id
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
                    "message": "If the email exists, a password reset email has been sent"
        })),
    )
        .into_response()
}

/// POST /api/auth/reset-password
pub async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>,
) -> impl IntoResponse {
    use crate::db::password_reset;

    if req.new_password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Password must be at least 8 characters")),
        )
            .into_response();
    }

    // Verify and consume the reset token atomically to prevent race conditions
    let user_id = match password_reset::verify_and_consume_reset_token(state.db(), &req.token).await
    {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Invalid or expired reset token")),
            )
                .into_response();
        }
    };

    let password_hash = match hash_password(&req.new_password) {
        Ok(hash) => hash,
        Err(e) => {
            tracing::error!("Password hashing error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    if let Err(e) = sqlx::query!(
        "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2",
        password_hash,
        user_id
    )
    .execute(state.db())
    .await
    {
        tracing::error!("Failed to update password: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    }

    if let Err(e) = refresh_tokens::revoke_all_user_tokens(state.db(), user_id).await {
        tracing::warn!(
            "Failed to revoke refresh tokens after password reset: {}",
            e
        );
    }

    if let Err(e) = sessions::revoke_all_user_sessions(state.db(), user_id).await {
        tracing::warn!("Failed to revoke sessions after password reset: {}", e);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "message": "Password reset successfully"
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{LOGIN_DISABLED, LOGIN_ERROR, Refusal, login_outcome, verification_url};

    #[test]
    fn a_refused_session_keeps_the_login_metric_labels() {
        assert_eq!(login_outcome(Refusal::DevicePending), LOGIN_DISABLED);
        assert_eq!(login_outcome(Refusal::DeviceBlocked), LOGIN_DISABLED);
        assert_eq!(login_outcome(Refusal::Missing), LOGIN_ERROR);
        assert_eq!(login_outcome(Refusal::Unavailable), LOGIN_ERROR);
        assert_eq!(LOGIN_DISABLED, "disabled");
        assert_eq!(LOGIN_ERROR, "error");
    }

    /// The console routes the verification page at /verify-email, so the mail
    /// must point there.
    #[test]
    fn the_verification_mail_links_to_the_console_verify_email_page() {
        assert_eq!(
            verification_url("http://localhost:4199", "abc123"),
            "http://localhost:4199/verify-email?token=abc123"
        );
    }
}
