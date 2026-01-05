//! Authentication middleware for axum
//!
//! Provides middleware for extracting and validating JWT tokens.

use axum::{
    Json,
    body::Body,
    extract::{FromRef, FromRequestParts, State},
    http::{Request, StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::jwt::{Claims, extract_bearer_token, validate_token};
use crate::state::AppState;

/// Auth error response
#[derive(Debug)]
pub struct AuthError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "error": self.message
        }));
        (self.status, body).into_response()
    }
}

/// Extractor for authenticated user claims
///
/// Use this as an extractor in route handlers to require authentication:
/// ```ignore
/// async fn handler(claims: AuthUser) -> impl IntoResponse {
///     format!("Hello, {}", claims.0.email)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthUser(pub Claims);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Get the app state
        let app_state = AppState::from_ref(state);

        // Get the Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError {
                status: StatusCode::UNAUTHORIZED,
                message: "Missing authorization header".to_string(),
            })?;

        // Extract the bearer token
        let token = extract_bearer_token(auth_header).ok_or(AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid authorization header format".to_string(),
        })?;

        // Validate the token
        let claims =
            validate_token(token, app_state.config().jwt_secret()).map_err(|e| AuthError {
                status: StatusCode::UNAUTHORIZED,
                message: format!("Invalid token: {}", e),
            })?;

        Ok(AuthUser(claims))
    }
}

/// Middleware that requires authentication
///
/// This middleware validates the JWT token and adds the claims to the request extensions.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AuthError> {
    // Get the Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Missing authorization header".to_string(),
        })?;

    // Extract the bearer token
    let token = extract_bearer_token(auth_header).ok_or(AuthError {
        status: StatusCode::UNAUTHORIZED,
        message: "Invalid authorization header format".to_string(),
    })?;

    // Validate the token
    let claims = validate_token(token, state.config().jwt_secret()).map_err(|e| AuthError {
        status: StatusCode::UNAUTHORIZED,
        message: format!("Invalid token: {}", e),
    })?;

    // Add claims to request extensions
    request.extensions_mut().insert(claims);

    Ok(next.run(request).await)
}

/// Check if the user has a required permission
#[allow(dead_code)]
pub fn require_permission(claims: &Claims, permission: &str) -> Result<(), AuthError> {
    if claims.has_permission(permission) {
        Ok(())
    } else {
        Err(AuthError {
            status: StatusCode::FORBIDDEN,
            message: format!("Missing required permission: {}", permission),
        })
    }
}

/// Check if the user has a required role
#[allow(dead_code)]
pub fn require_role(claims: &Claims, role: &str) -> Result<(), AuthError> {
    if claims.has_role(role) {
        Ok(())
    } else {
        Err(AuthError {
            status: StatusCode::FORBIDDEN,
            message: format!("Missing required role: {}", role),
        })
    }
}

/// Check if the user is an admin
#[allow(dead_code)]
pub fn require_admin(claims: &Claims) -> Result<(), AuthError> {
    if claims.is_admin {
        Ok(())
    } else {
        Err(AuthError {
            status: StatusCode::FORBIDDEN,
            message: "Admin access required".to_string(),
        })
    }
}
