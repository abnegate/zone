//! Authentication endpoints

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{
    AuthUser, create_access_token, create_refresh_token, hash_password, verify_password,
};
use crate::db::{
    organization_members::{self, OrgRole},
    organizations, refresh_tokens, sessions, users,
    workspace_members::{self, WorkspaceRole},
    workspaces,
};
use crate::state::AppState;
use crate::utils::crypto::hash_token;

use super::common::{ErrorResponse, Timestamps};

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

/// Auth response
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    access_token: String,
    refresh_token: String,
    token_type: &'static str,
    expires_in: u64,
    user: UserResponse,
    roles: Vec<String>,
    permissions: Vec<String>,
}

/// User response
#[derive(Debug, Serialize)]
pub struct UserResponse {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    is_admin: bool,
    is_active: bool,
    email_verified: bool,
    #[serde(flatten)]
    timestamps: Timestamps,
    last_login_at: Option<String>,
}

impl UserResponse {
    /// Create a UserResponse from UserWithPermissions
    pub fn from_user(user: &users::UserWithPermissions) -> Self {
        Self {
            id: user.user.id,
            email: user.user.email.clone(),
            display_name: user.user.display_name.clone(),
            is_admin: user.user.is_admin.unwrap_or(false),
            is_active: user.user.is_active.unwrap_or(true),
            email_verified: user.user.email_verified,
            timestamps: Timestamps::from_naive(user.user.created_at, user.user.updated_at),
            last_login_at: user.user.last_login_at.map(|dt| dt.and_utc().to_rfc3339()),
        }
    }
}

/// POST /api/auth/register
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    // Validate email with proper regex
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

    // Validate password - require length, uppercase, lowercase, and number
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

    // Check if user already exists
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

    // Hash password
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

    // Check if this is the first user (make admin)
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

    // Create user
    let user = match users::create_user(
        state.db(),
        &req.email,
        &password_hash,
        req.display_name.as_deref(),
        is_admin,
    )
    .await
    {
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

    // Assign the default "user" role to give basic permissions
    if let Err(e) = users::assign_user_role(state.db(), user.id, "user").await {
        tracing::error!(
            "Failed to assign user role: {}. User may have limited permissions.",
            e
        );
    }

    // Create a default organization and workspace for every new user
    {
        // Generate organization name based on user's display name or email
        let org_name = req
            .display_name
            .as_ref()
            .map(|name| format!("{}'s Organization", name))
            .unwrap_or_else(|| "My Organization".to_string());

        // Generate slug from organization name with user id suffix for uniqueness
        let base_slug = org_name
            .to_lowercase()
            .replace(' ', "-")
            .replace('\'', "")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();

        // Add a short unique suffix to ensure slug uniqueness across users
        let org_slug = format!("{}-{}", base_slug, &user.id.to_string()[..8]);

        // Create the default organization
        match organizations::create_organization(state.db(), &org_name, &org_slug, None).await {
            Ok(org) => {
                tracing::info!(
                    "Created default organization '{}' for user {}",
                    org.name,
                    user.id
                );

                // Add user as owner of the organization
                if let Err(e) = organization_members::add_member(
                    state.db(),
                    org.id,
                    user.id,
                    OrgRole::Owner,
                    None,
                )
                .await
                {
                    tracing::error!(
                        "Failed to add user as organization owner: {}. User can manually create organization.",
                        e
                    );
                } else {
                    tracing::info!("Added user {} as owner of organization {}", user.id, org.id);

                    // Create a default workspace within the organization
                    match workspaces::create_workspace(
                        state.db(),
                        org.id,
                        "Default Workspace",
                        "default",
                        Some("Your default workspace"),
                    )
                    .await
                    {
                        Ok(workspace) => {
                            tracing::info!(
                                "Created default workspace '{}' in organization {}",
                                workspace.name,
                                org.id
                            );

                            // Add user as owner of the workspace
                            if let Err(e) = workspace_members::add_member(
                                state.db(),
                                workspace.id,
                                user.id,
                                WorkspaceRole::Owner,
                                None,
                            )
                            .await
                            {
                                tracing::error!(
                                    "Failed to add user as workspace owner: {}. User can manually join workspace.",
                                    e
                                );
                            } else {
                                tracing::info!(
                                    "Added user {} as owner of workspace {}",
                                    user.id,
                                    workspace.id
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to create default workspace: {}. User can manually create workspace.",
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to create default organization: {}. User can manually create organization.",
                    e
                );
            }
        }
    }

    // Get user with permissions
    let user_perms = match users::get_user_with_permissions(state.db(), user.id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("User not found after creation")),
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

    // Generate tokens
    let (access_token, refresh_token) = match generate_tokens(&state, &user_perms, None, None).await
    {
        Ok(tokens) => tokens,
        Err(response) => return response.into_response(),
    };

    (
        StatusCode::CREATED,
        Json(AuthResponse {
            access_token,
            refresh_token,
            token_type: "Bearer",
            expires_in: state.config().jwt_access_lifetime,
            user: UserResponse::from_user(&user_perms),
            roles: user_perms.roles.clone(),
            permissions: user_perms.permissions.clone(),
        }),
    )
        .into_response()
}

/// POST /api/auth/login
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    // Get user by email
    let user = match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid email or password")),
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

    // Verify password
    match verify_password(&req.password, &user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid email or password")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Password verification error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    // Check if user is active
    if !user.is_active.unwrap_or(true) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Account is disabled")),
        )
            .into_response();
    }

    // Update last login
    if let Err(e) = users::update_last_login(state.db(), user.id).await {
        tracing::warn!("Failed to update last login: {}", e);
    }

    // Get user with permissions
    let user_perms = match users::get_user_with_permissions(state.db(), user.id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
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

    // Generate tokens
    let (access_token, refresh_token) = match generate_tokens(&state, &user_perms, None, None).await
    {
        Ok(tokens) => tokens,
        Err(response) => return response.into_response(),
    };

    Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: state.config().jwt_access_lifetime,
        user: UserResponse::from_user(&user_perms),
        roles: user_perms.roles.clone(),
        permissions: user_perms.permissions.clone(),
    })
    .into_response()
}

/// POST /api/auth/refresh
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> impl IntoResponse {
    // Hash the refresh token to look it up
    let token_hash = hash_token(&req.refresh_token);

    // Validate the refresh token in the database
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

    // Revoke the old refresh token
    if let Err(e) = refresh_tokens::revoke_refresh_token(state.db(), &token_hash).await {
        tracing::warn!("Failed to revoke old refresh token: {}", e);
    }

    // Get user with permissions
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

    // Generate new tokens
    let (access_token, refresh_token) = match generate_tokens(&state, &user_perms, None, None).await
    {
        Ok(tokens) => tokens,
        Err(response) => return response.into_response(),
    };

    Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: state.config().jwt_access_lifetime,
        user: UserResponse::from_user(&user_perms),
        roles: user_perms.roles.clone(),
        permissions: user_perms.permissions.clone(),
    })
    .into_response()
}

/// POST /api/auth/logout
pub async fn logout(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    // Revoke all refresh tokens and sessions for this user
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

/// Generate access and refresh tokens
async fn generate_tokens(
    state: &AppState,
    user: &users::UserWithPermissions,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(String, String), (StatusCode, Json<ErrorResponse>)> {
    let config = state.config();

    // Create access token
    let access_token = create_access_token(
        user.user.id,
        &user.user.email,
        user.roles.clone(),
        user.permissions.clone(),
        user.user.is_admin.unwrap_or(false),
        config.jwt_secret(),
        config.access_token_lifetime(),
    )
    .map_err(|e| {
        tracing::error!("Token creation error: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
    })?;

    // Create refresh token
    let refresh_token = create_refresh_token(
        user.user.id,
        config.jwt_secret(),
        config.refresh_token_lifetime(),
    )
    .map_err(|e| {
        tracing::error!("Token creation error: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
    })?;

    // Store refresh token hash in database
    let token_hash = hash_token(&refresh_token);
    let expires_at = Utc::now() + config.refresh_token_lifetime();

    if let Err(e) = refresh_tokens::create_refresh_token(
        state.db(),
        user.user.id,
        &token_hash,
        expires_at.naive_utc(),
        user_agent,
        ip_address,
    )
    .await
    {
        tracing::error!("Failed to store refresh token: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        ));
    }

    // Create session record to track this authentication
    if let Err(e) = sessions::create_session(
        state.db(),
        user.user.id,
        &token_hash,
        ip_address,
        user_agent,
        None, // device_info can be parsed from user_agent if needed
        expires_at.naive_utc(),
    )
    .await
    {
        tracing::error!("Failed to create session: {}", e);
        // Don't fail auth if session creation fails, just log it
    }

    Ok((access_token, refresh_token))
}

// =============================================================================
// Email Verification Endpoints
// =============================================================================

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

    // Verify the token and get user_id
    let user_id = match email_verification::verify_token(state.db(), &req.token).await {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Invalid or expired verification token")),
            )
                .into_response();
        }
    };

    // Mark email as verified
    if let Err(e) = email_verification::mark_email_verified(state.db(), user_id).await {
        tracing::error!("Failed to mark email as verified: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Email verified successfully"
        })),
    )
        .into_response()
}

/// POST /api/auth/resend-verification
pub async fn resend_verification(
    State(state): State<AppState>,
    Json(req): Json<ResendVerificationRequest>,
) -> impl IntoResponse {
    use crate::db::email_verification;

    // Get user by email
    let user = match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // Don't reveal whether email exists
            return (
                StatusCode::OK,
                Json(serde_json::json!({
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
                "message": "If the email exists, a verification email has been sent"
            })),
        )
            .into_response();
    }

    // Create new verification token
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

    // Send verification email if email service is configured
    if let Some(email_service) = state.email_service() {
        // Build verification URL using configured base URL
        let verification_url = format!("{}/verify?token={}", state.config().app_base_url, token);
        let display_name = user.display_name.as_deref().unwrap_or(&user.email);

        if let Err(e) = email_service
            .send_verification_email(&user.email, display_name, &verification_url)
            .await
        {
            tracing::error!("Failed to send verification email: {}", e);
            // Don't fail the request - token is created, user can retry
        } else {
            tracing::info!("Verification email sent to user_id: {}", user.id);
        }
    } else {
        tracing::debug!(
            "Email service not configured, verification token created for user_id: {}",
            user.id
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "If the email exists, a verification email has been sent"
        })),
    )
        .into_response()
}

// =============================================================================
// Password Reset Endpoints
// =============================================================================

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

    // Get user by email
    let user = match users::get_user_by_email(state.db(), &req.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            // Don't reveal whether email exists for security
            return (
                StatusCode::OK,
                Json(serde_json::json!({
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

    // Create reset token
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

    // Send password reset email if email service is configured
    if let Some(email_service) = state.email_service() {
        // Build reset URL using configured base URL
        let reset_url = format!(
            "{}/reset-password?token={}",
            state.config().app_base_url,
            token
        );
        let display_name = user.display_name.as_deref().unwrap_or(&user.email);

        if let Err(e) = email_service
            .send_password_reset_email(&user.email, display_name, &reset_url)
            .await
        {
            tracing::error!("Failed to send password reset email: {}", e);
            // Don't fail the request - token is created, user can retry
        } else {
            tracing::info!("Password reset email sent to user_id: {}", user.id);
        }
    } else {
        tracing::debug!(
            "Email service not configured, password reset token created for user_id: {}",
            user.id
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
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

    // Validate new password
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

    // Hash the new password
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

    // Update the password
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

    // Revoke all existing refresh tokens and sessions for security
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
            "message": "Password reset successfully"
        })),
    )
        .into_response()
}
