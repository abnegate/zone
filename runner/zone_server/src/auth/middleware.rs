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
    // Axum can run this layer on unmatched paths after a merge. Never gate
    // liveness/scrape — Prometheus uses those to set `up{job="manager"}`.
    let path = request.uri().path();
    if path == "/health" || path == "/metrics" {
        return Ok(next.run(request).await);
    }

    // Get the Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            crate::metrics::record_auth_failure("missing_header");
            AuthError {
                status: StatusCode::UNAUTHORIZED,
                message: "Missing authorization header".to_string(),
            }
        })?;

    // Extract the bearer token
    let token = extract_bearer_token(auth_header).ok_or_else(|| {
        crate::metrics::record_auth_failure("bad_format");
        AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid authorization header format".to_string(),
        }
    })?;

    // Validate the token
    let claims = validate_token(token, state.config().jwt_secret()).map_err(|e| {
        crate::metrics::record_auth_failure("invalid_token");
        AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: format!("Invalid token: {}", e),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn create_test_claims(is_admin: bool, roles: Vec<&str>, permissions: Vec<&str>) -> Claims {
        Claims {
            sub: "test-user-id".to_string(),
            email: "test@example.com".to_string(),
            is_admin,
            roles: roles.into_iter().map(|s| s.to_string()).collect(),
            permissions: permissions.into_iter().map(|s| s.to_string()).collect(),
            exp: chrono::Utc::now().timestamp() + 3600,
            iat: chrono::Utc::now().timestamp(),
            jti: "test-jti".to_string(),
        }
    }

    // Tests for AuthError
    #[test]
    fn test_auth_error_debug() {
        let error = AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Test error".to_string(),
        };
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("AuthError"));
        assert!(debug_str.contains("Test error"));
    }

    #[test]
    fn test_auth_error_into_response() {
        let error = AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Unauthorized".to_string(),
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_auth_error_forbidden() {
        let error = AuthError {
            status: StatusCode::FORBIDDEN,
            message: "Forbidden".to_string(),
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // Tests for require_permission
    #[test]
    fn test_require_permission_has_permission() {
        let claims = create_test_claims(false, vec![], vec!["read:users"]);
        let result = require_permission(&claims, "read:users");
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_permission_missing_permission() {
        let claims = create_test_claims(false, vec![], vec!["read:users"]);
        let result = require_permission(&claims, "write:users");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert!(error.message.contains("Missing required permission"));
        assert!(error.message.contains("write:users"));
    }

    #[test]
    fn test_require_permission_admin_has_all() {
        let claims = create_test_claims(true, vec![], vec![]);
        let result = require_permission(&claims, "any:permission");
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_permission_empty_permissions() {
        let claims = create_test_claims(false, vec![], vec![]);
        let result = require_permission(&claims, "read:users");
        assert!(result.is_err());
    }

    // Tests for require_role
    #[test]
    fn test_require_role_has_role() {
        let claims = create_test_claims(false, vec!["editor"], vec![]);
        let result = require_role(&claims, "editor");
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_role_missing_role() {
        let claims = create_test_claims(false, vec!["viewer"], vec![]);
        let result = require_role(&claims, "editor");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert!(error.message.contains("Missing required role"));
        assert!(error.message.contains("editor"));
    }

    #[test]
    fn test_require_role_admin_has_all() {
        let claims = create_test_claims(true, vec![], vec![]);
        let result = require_role(&claims, "any_role");
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_role_empty_roles() {
        let claims = create_test_claims(false, vec![], vec![]);
        let result = require_role(&claims, "viewer");
        assert!(result.is_err());
    }

    #[test]
    fn test_require_role_multiple_roles() {
        let claims = create_test_claims(false, vec!["viewer", "editor", "admin"], vec![]);
        assert!(require_role(&claims, "viewer").is_ok());
        assert!(require_role(&claims, "editor").is_ok());
        assert!(require_role(&claims, "admin").is_ok());
        assert!(require_role(&claims, "owner").is_err());
    }

    // Tests for require_admin
    #[test]
    fn test_require_admin_is_admin() {
        let claims = create_test_claims(true, vec![], vec![]);
        let result = require_admin(&claims);
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_admin_not_admin() {
        let claims = create_test_claims(false, vec![], vec![]);
        let result = require_admin(&claims);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert!(error.message.contains("Admin access required"));
    }

    #[test]
    fn test_require_admin_with_roles_not_admin() {
        let claims = create_test_claims(false, vec!["admin"], vec![]);
        // Having an "admin" role doesn't make is_admin true
        let result = require_admin(&claims);
        assert!(result.is_err());
    }

    // Tests for AuthUser
    #[test]
    fn test_auth_user_debug() {
        let claims = create_test_claims(false, vec![], vec![]);
        let auth_user = AuthUser(claims.clone());
        let debug_str = format!("{:?}", auth_user);
        assert!(debug_str.contains("AuthUser"));
    }

    #[test]
    fn test_auth_user_clone() {
        let claims = create_test_claims(false, vec!["viewer"], vec!["read:users"]);
        let auth_user = AuthUser(claims);
        let cloned = auth_user.clone();
        assert_eq!(cloned.0.email, auth_user.0.email);
        assert_eq!(cloned.0.roles, auth_user.0.roles);
    }
}
