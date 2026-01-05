//! Authentication endpoints

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::{
    AuthUser, create_access_token, create_refresh_token, hash_password, verify_password,
};
use crate::db::{refresh_tokens, users};
use crate::state::AppState;

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

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
}

/// User response
#[derive(Debug, Serialize)]
pub struct UserResponse {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    is_admin: bool,
}

/// POST /api/auth/register
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    // Validate email
    if !req.email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid email format")),
        )
            .into_response();
    }

    // Validate password
    if req.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Password must be at least 8 characters")),
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
    let (access_token, refresh_token) = match generate_tokens(&state, &user_perms).await {
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
            user: UserResponse {
                id: user.id,
                email: user.email,
                display_name: user.display_name,
                is_admin: user.is_admin.unwrap_or(false),
            },
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
    let (access_token, refresh_token) = match generate_tokens(&state, &user_perms).await {
        Ok(tokens) => tokens,
        Err(response) => return response.into_response(),
    };

    Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: state.config().jwt_access_lifetime,
        user: UserResponse {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            is_admin: user.is_admin.unwrap_or(false),
        },
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
    let (access_token, refresh_token) = match generate_tokens(&state, &user_perms).await {
        Ok(tokens) => tokens,
        Err(response) => return response.into_response(),
    };

    Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: state.config().jwt_access_lifetime,
        user: UserResponse {
            id: user_perms.user.id,
            email: user_perms.user.email,
            display_name: user_perms.user.display_name,
            is_admin: user_perms.user.is_admin.unwrap_or(false),
        },
    })
    .into_response()
}

/// POST /api/auth/logout
pub async fn logout(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    // Revoke all refresh tokens for this user
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

    StatusCode::NO_CONTENT.into_response()
}

/// Generate access and refresh tokens
async fn generate_tokens(
    state: &AppState,
    user: &users::UserWithPermissions,
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
        None,
        None,
    )
    .await
    {
        tracing::error!("Failed to store refresh token: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        ));
    }

    Ok((access_token, refresh_token))
}

/// Hash a token for storage
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
