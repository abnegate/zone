//! Authentication endpoints

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;
use zone_enterprise::sso::{self, PasswordAccess};

use crate::auth::{AuthUser, account, admin_guard, hash_password, verify_password};
use crate::db::DbResult;
use crate::db::audit::{actions, resources};
use crate::db::users::UserRow;
use crate::db::{password_reset, refresh_tokens, sessions, users};
use crate::services::account::{AuthResponse, Credential, Grant, Refusal, provision, session};
use crate::services::mail::Template;
use crate::services::sso::enforcement;
use crate::state::AppState;
use crate::utils::crypto::hash_token;

use super::common::{AuditEvent, ErrorResponse, audit};
use super::sso::refusal::{self, Stage};

const LOGIN_OK: &str = "ok";
const LOGIN_DISABLED: &str = "disabled";
const LOGIN_ERROR: &str = "error";
const LOGIN_INVALID_CREDENTIALS: &str = "invalid_credentials";
const EMAIL_REGISTERED: &str = "Email already registered";

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
    Json(request): Json<RegisterRequest>,
) -> impl IntoResponse {
    match enforcement::access(state.db(), false).await {
        Ok(PasswordAccess::Refused) => return sso_required(),
        Ok(PasswordAccess::Allowed | PasswordAccess::BreakGlass) => {}
        Err(error) => return unavailable(&error),
    }

    let email_regex = regex::Regex::new(
        r"^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$"
    ).unwrap();

    if !email_regex.is_match(&request.email) || request.email.len() > 254 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid email format")),
        )
            .into_response();
    }

    if request.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Password must be at least 8 characters")),
        )
            .into_response();
    }

    let has_uppercase = request.password.chars().any(char::is_uppercase);
    let has_lowercase = request.password.chars().any(char::is_lowercase);
    let has_digit = request
        .password
        .chars()
        .any(|character| character.is_ascii_digit());

    if !has_uppercase || !has_lowercase || !has_digit {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "Password must contain at least one uppercase letter, one lowercase letter, and one number"
            )),
        )
            .into_response();
    }

    match users::get_user_by_email(state.db(), &request.email.to_lowercase()).await {
        Ok(Some(_)) => return already_registered(),
        Err(error) => return unavailable(&error),
        Ok(None) => {}
    }

    let password_hash = match hash_password(&request.password) {
        Ok(hash) => hash,
        Err(error) => {
            tracing::error!("Password hashing error: {}", error);
            return internal_error();
        }
    };

    let is_admin = match users::count_users(state.db()).await {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => return unavailable(&error),
    };

    let user = match insert(&state, &request, &password_hash, is_admin).await {
        Ok(user) => user,
        Err(error) if registered(&error) => return already_registered(),
        Err(error) => return unavailable(&error),
    };

    provision::furnish(state.db(), &user).await;

    match session::issue(&state, user.id, &headers, Credential::Registration).await {
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
    Json(request): Json<LoginRequest>,
) -> impl IntoResponse {
    let user = match users::get_user_by_email(state.db(), &request.email.to_lowercase()).await {
        Ok(user) => user,
        Err(error) => {
            crate::metrics::record_login(LOGIN_ERROR);
            return unavailable(&error);
        }
    };

    let administers = user
        .as_ref()
        .is_some_and(|user| admin_guard::administers(user.is_admin, user.is_active));
    let access = match enforcement::access(state.db(), administers).await {
        Ok(access) => access,
        Err(error) => {
            crate::metrics::record_login(LOGIN_ERROR);
            return unavailable(&error);
        }
    };
    if access == PasswordAccess::Refused {
        crate::metrics::record_login(sso::SSO_REQUIRED);
        return sso_required();
    }

    let Some(user) = user else {
        crate::metrics::record_login(LOGIN_INVALID_CREDENTIALS);
        return invalid_credentials();
    };

    match verify_password(&request.password, &user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            crate::metrics::record_login(LOGIN_INVALID_CREDENTIALS);
            return invalid_credentials();
        }
        Err(error) => {
            tracing::error!(%error, "Password verification error");
            crate::metrics::record_login(LOGIN_ERROR);
            return internal_error();
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

    if let Err(error) = users::update_last_login(state.db(), user.id).await {
        tracing::warn!(%error, "Failed to update last login");
    }

    match session::issue(
        &state,
        user.id,
        &headers,
        Credential::Password(&user.password_hash),
    )
    .await
    {
        Ok(response) => {
            crate::metrics::record_login(LOGIN_OK);
            if access == PasswordAccess::BreakGlass {
                record_break_glass(&state, user.id, &user.email).await;
            }
            Json(response).into_response()
        }
        Err(refusal) => {
            crate::metrics::record_login(login_outcome(refusal));
            refusal.into_response()
        }
    }
}

/// A server admin signing in with a password while single sign-on is required is the way back in
/// when the identity provider is down, so every such sign-in is on the record.
async fn record_break_glass(state: &AppState, user_id: Uuid, email: &str) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: None,
            actor_id: user_id,
            actor_email: email,
            action: actions::USER_BREAK_GLASS_SIGNED_IN,
            resource_type: resources::USER,
            resource_id: Some(user_id),
            old_values: None,
            new_values: None,
        },
    )
    .await;
}

fn registered(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|database| {
        database.is_unique_violation()
            && database.constraint().is_some_and(|constraint| {
                [users::EMAIL_KEY, users::EMAIL_LOWER_KEY].contains(&constraint)
            })
    })
}

fn already_registered() -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse::new(EMAIL_REGISTERED)),
    )
        .into_response()
}

fn sso_required() -> Response {
    refusal::respond(&sso::Error::Required, Stage::SignIn)
}

fn invalid_credentials() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse::new("Invalid email or password")),
    )
        .into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse::new("Internal server error")),
    )
        .into_response()
}

fn unavailable(error: &sqlx::Error) -> Response {
    tracing::error!("Database error: {}", error);
    internal_error()
}

fn login_outcome(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::DevicePending | Refusal::DeviceBlocked => LOGIN_DISABLED,
        Refusal::Superseded => LOGIN_INVALID_CREDENTIALS,
        Refusal::Missing | Refusal::Unavailable => LOGIN_ERROR,
    }
}

/// POST /api/auth/refresh
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RefreshRequest>,
) -> impl IntoResponse {
    let token_hash = hash_token(&request.refresh_token);

    let user_id = match refresh_tokens::validate_refresh_token(state.db(), &token_hash).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid or expired refresh token")),
            )
                .into_response();
        }
        Err(error) => return unavailable(&error),
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
        Err(error) => return unavailable(&error),
    };

    let user = match users::get_user_with_permissions(state.db(), user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("User not found")),
            )
                .into_response();
        }
        Err(error) => return unavailable(&error),
    };

    if current.sso_provider_id.is_none() {
        let administers = admin_guard::administers(user.user.is_admin, user.user.is_active);
        match enforcement::access(state.db(), administers).await {
            Ok(PasswordAccess::Refused) => {
                if let Err(error) = sessions::revoke_session(state.db(), current.id).await {
                    return unavailable(&error);
                }
                return sso_required();
            }
            Ok(PasswordAccess::Allowed | PasswordAccess::BreakGlass) => {}
            Err(error) => return unavailable(&error),
        }
    }

    if let Err(error) = refresh_tokens::revoke_refresh_token(state.db(), &token_hash).await {
        tracing::warn!(%error, "Failed to revoke old refresh token");
    }

    let device = match session::admit_device(&state, user_id, &headers, current.device_id).await {
        Ok(device) => device,
        Err(refusal) => return refusal.into_response(),
    };

    match session::generate_tokens(&state, &user, &headers, &device, Grant::Refresh(current.id))
        .await
    {
        Ok(tokens) => Json(AuthResponse::new(
            &user,
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

    if let Err(error) = refresh_tokens::revoke_all_user_tokens(state.db(), user_id).await {
        tracing::warn!("Failed to revoke refresh tokens: {}", error);
    }

    if let Err(error) = sessions::revoke_all_user_sessions(state.db(), user_id).await {
        tracing::warn!("Failed to revoke sessions: {}", error);
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
    Json(request): Json<VerifyEmailRequest>,
) -> impl IntoResponse {
    use crate::db::email_verification;

    let verified = serde_json::json!({
        "success": true,
        "message": "Email verified successfully"
    });

    let user_id = match email_verification::verify_token(state.db(), &request.token).await {
        Ok(id) => id,
        Err(_) => {
            return match email_verification::recently_verified_user(state.db(), &request.token)
                .await
            {
                Ok(Some(_)) => (StatusCode::OK, Json(verified)).into_response(),
                Ok(None) => (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new("Invalid or expired verification token")),
                )
                    .into_response(),
                Err(error) => {
                    tracing::error!("Failed to look up verification token: {}", error);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Internal server error")),
                    )
                        .into_response()
                }
            };
        }
    };

    if let Err(error) = email_verification::mark_email_verified(state.db(), user_id).await {
        tracing::error!("Failed to mark email as verified: {}", error);
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
    Json(request): Json<ResendVerificationRequest>,
) -> impl IntoResponse {
    use crate::db::email_verification;

    let user = match users::get_user_by_email(state.db(), &request.email).await {
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
        Err(error) => {
            tracing::error!("Database error: {}", error);
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
            Err(error) => {
                tracing::error!("Failed to create verification token: {}", error);
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
    Json(request): Json<ForgotPasswordRequest>,
) -> impl IntoResponse {
    let user = match users::get_user_by_email(state.db(), &request.email).await {
        Ok(Some(user)) => user,
        Ok(None) => return reset_requested(),
        Err(error) => return unavailable(&error),
    };

    match enforcement::access(
        state.db(),
        admin_guard::administers(user.is_admin, user.is_active),
    )
    .await
    {
        Ok(PasswordAccess::Refused) => return reset_requested(),
        Ok(PasswordAccess::Allowed | PasswordAccess::BreakGlass) => {}
        Err(error) => return unavailable(&error),
    }

    let (token, _expires_at) = match password_reset::create_reset_token(state.db(), user.id).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!("Failed to create reset token: {}", error);
            return internal_error();
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

    reset_requested()
}

/// The same answer whether or not the email has an account, or may reset its password, so the
/// endpoint can't be used to find out either.
fn reset_requested() -> Response {
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
    Json(request): Json<ResetPasswordRequest>,
) -> impl IntoResponse {
    if request.new_password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Password must be at least 8 characters")),
        )
            .into_response();
    }

    if let Err(refusal) = reset_allowed(&state, &request.token).await {
        return refusal.into_response();
    }

    let user_id =
        match password_reset::verify_and_consume_reset_token(state.db(), &request.token).await {
            Ok(id) => id,
            Err(_) => return invalid_reset_token(),
        };

    let password_hash = match hash_password(&request.new_password) {
        Ok(hash) => hash,
        Err(error) => {
            tracing::error!("Password hashing error: {}", error);
            return internal_error();
        }
    };

    if let Err(error) = sqlx::query!(
        "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2",
        password_hash,
        user_id
    )
    .execute(state.db())
    .await
    {
        tracing::error!("Failed to update password: {}", error);
        return internal_error();
    }

    if let Err(error) = refresh_tokens::revoke_all_user_tokens(state.db(), user_id).await {
        tracing::warn!(
            "Failed to revoke refresh tokens after password reset: {}",
            error
        );
    }

    if let Err(error) = sessions::revoke_all_user_sessions(state.db(), user_id).await {
        tracing::warn!("Failed to revoke sessions after password reset: {}", error);
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

/// Looks at the token without using it, so a refused reset leaves the token and the password as
/// they were.
async fn reset_allowed(state: &AppState, token: &str) -> Result<(), ResetRefusal> {
    let user_id = password_reset::verify_reset_token(state.db(), token)
        .await
        .map_err(|_| ResetRefusal::InvalidToken)?;
    let user = users::get_user_by_id(state.db(), user_id)
        .await?
        .ok_or(ResetRefusal::InvalidToken)?;
    match enforcement::access(
        state.db(),
        admin_guard::administers(user.is_admin, user.is_active),
    )
    .await?
    {
        PasswordAccess::Refused => Err(ResetRefusal::Required),
        PasswordAccess::Allowed | PasswordAccess::BreakGlass => Ok(()),
    }
}

enum ResetRefusal {
    InvalidToken,
    Required,
    Unavailable(sqlx::Error),
}

impl From<sqlx::Error> for ResetRefusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Unavailable(error)
    }
}

impl IntoResponse for ResetRefusal {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidToken => invalid_reset_token(),
            Self::Required => sso_required(),
            Self::Unavailable(error) => unavailable(&error),
        }
    }
}

fn invalid_reset_token() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::new("Invalid or expired reset token")),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use zone_enterprise::sso;

    use super::{
        LOGIN_DISABLED, LOGIN_ERROR, LOGIN_INVALID_CREDENTIALS, Refusal, login_outcome,
        verification_url,
    };

    #[test]
    fn a_refused_session_keeps_the_login_metric_labels() {
        assert_eq!(login_outcome(Refusal::DevicePending), LOGIN_DISABLED);
        assert_eq!(login_outcome(Refusal::DeviceBlocked), LOGIN_DISABLED);
        assert_eq!(login_outcome(Refusal::Missing), LOGIN_ERROR);
        assert_eq!(login_outcome(Refusal::Unavailable), LOGIN_ERROR);
        assert_eq!(
            login_outcome(Refusal::Superseded),
            LOGIN_INVALID_CREDENTIALS
        );
        assert_eq!(LOGIN_DISABLED, "disabled");
        assert_eq!(LOGIN_ERROR, "error");
        assert_eq!(LOGIN_INVALID_CREDENTIALS, "invalid_credentials");
    }

    #[test]
    fn a_password_refused_for_single_sign_on_has_its_own_login_metric_label() {
        assert_eq!(sso::SSO_REQUIRED, "sso_required");
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
