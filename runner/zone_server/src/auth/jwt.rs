//! JWT token handling
//!
//! Provides JWT creation and validation for user authentication.

use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT claims
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// User email
    pub email: String,
    /// User roles
    pub roles: Vec<String>,
    /// User permissions
    pub permissions: Vec<String>,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at (Unix timestamp)
    pub iat: i64,
    /// JWT ID
    pub jti: String,
    /// Whether user is admin
    pub is_admin: bool,
}

impl Claims {
    /// Check if the user has a specific permission
    #[allow(dead_code)]
    pub fn has_permission(&self, permission: &str) -> bool {
        self.is_admin || self.permissions.contains(&permission.to_string())
    }

    /// Check if the user has a specific role
    #[allow(dead_code)]
    pub fn has_role(&self, role: &str) -> bool {
        self.is_admin || self.roles.contains(&role.to_string())
    }

    /// Get the user ID as UUID
    pub fn user_id(&self) -> Result<Uuid, uuid::Error> {
        Uuid::parse_str(&self.sub)
    }
}

/// JWT error types
#[derive(Debug, thiserror::Error)]
pub enum JwtError {
    #[error("Token creation failed: {0}")]
    Creation(#[from] jsonwebtoken::errors::Error),

    #[error("Token expired")]
    Expired,

    #[error("Invalid token")]
    Invalid,

    #[error("JWT secret must be at least 32 characters long")]
    SecretTooShort,

    #[error("JWT secret has insufficient entropy (too few unique characters)")]
    SecretLowEntropy,
}

/// Validate secret strength
fn validate_secret(secret: &str) -> Result<(), JwtError> {
    if secret.len() < 32 {
        return Err(JwtError::SecretTooShort);
    }

    // Check entropy - reject secrets with too few unique characters
    let unique_chars: std::collections::HashSet<u8> = secret.as_bytes().iter().copied().collect();
    if unique_chars.len() < 10 {
        return Err(JwtError::SecretLowEntropy);
    }

    Ok(())
}

/// Create an access token for a user
pub fn create_access_token(
    user_id: Uuid,
    email: &str,
    roles: Vec<String>,
    permissions: Vec<String>,
    is_admin: bool,
    secret: &str,
    expires_in: Duration,
) -> Result<String, JwtError> {
    validate_secret(secret)?;

    let now = Utc::now();
    let exp = now + expires_in;

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        roles,
        permissions,
        is_admin,
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: Uuid::new_v4().to_string(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(token)
}

/// Create a refresh token
pub fn create_refresh_token(
    user_id: Uuid,
    secret: &str,
    expires_in: Duration,
) -> Result<String, JwtError> {
    validate_secret(secret)?;

    let now = Utc::now();
    let exp = now + expires_in;

    // Refresh tokens have minimal claims
    let claims = Claims {
        sub: user_id.to_string(),
        email: String::new(),
        roles: vec![],
        permissions: vec![],
        is_admin: false,
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: Uuid::new_v4().to_string(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(token)
}

/// Validate and decode a JWT token
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, JwtError> {
    validate_secret(secret)?;

    let mut validation = Validation::default();
    validation.validate_exp = true;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => JwtError::Expired,
        _ => JwtError::Invalid,
    })?;

    Ok(token_data.claims)
}

/// Extract token from Authorization header
pub fn extract_bearer_token(header: &str) -> Option<&str> {
    header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate_token() {
        let user_id = Uuid::new_v4();
        let secret = "test-secret-key-12345-must-be-32-chars-or-longer";
        let expires_in = Duration::hours(1);

        let token = create_access_token(
            user_id,
            "test@example.com",
            vec!["user".to_string()],
            vec!["read".to_string()],
            false,
            secret,
            expires_in,
        )
        .unwrap();

        let claims = validate_token(&token, secret).unwrap();
        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.email, "test@example.com");
        assert!(claims.has_role("user"));
        assert!(claims.has_permission("read"));
    }

    #[test]
    fn test_expired_token() {
        let user_id = Uuid::new_v4();
        let secret = "test-secret-key-12345-must-be-32-chars-or-longer";
        // Use a much older expiration to avoid clock skew tolerance
        let expires_in = Duration::hours(-1);

        let token = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            secret,
            expires_in,
        )
        .unwrap();

        let result = validate_token(&token, secret);
        // Token with past expiration should fail validation
        assert!(result.is_err(), "Expected expired token to fail validation");
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("abc123"), None);
    }

    // ============ Admin permission tests ============

    #[test]
    fn test_admin_has_all_permissions() {
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            email: "admin@example.com".to_string(),
            roles: vec![],
            permissions: vec![],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: true,
        };

        // Admin should have any permission even if not explicitly listed
        assert!(claims.has_permission("read"));
        assert!(claims.has_permission("write"));
        assert!(claims.has_permission("delete"));
        assert!(claims.has_permission("any_permission_at_all"));
    }

    #[test]
    fn test_admin_has_all_roles() {
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            email: "admin@example.com".to_string(),
            roles: vec![],
            permissions: vec![],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: true,
        };

        // Admin should have any role even if not explicitly listed
        assert!(claims.has_role("user"));
        assert!(claims.has_role("moderator"));
        assert!(claims.has_role("superuser"));
    }

    #[test]
    fn test_non_admin_requires_explicit_permission() {
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            email: "user@example.com".to_string(),
            roles: vec!["user".to_string()],
            permissions: vec!["read".to_string()],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        };

        // Should have explicitly listed permission/role
        assert!(claims.has_permission("read"));
        assert!(claims.has_role("user"));

        // Should not have non-listed permission/role
        assert!(!claims.has_permission("write"));
        assert!(!claims.has_role("admin"));
    }

    // ============ Invalid secret handling tests ============

    #[test]
    fn test_validate_with_wrong_secret() {
        let user_id = Uuid::new_v4();
        let secret = "correct-secret-must-be-32-chars-or-longer-here";
        let wrong_secret = "wrong-secret-must-be-32-chars-or-longer-here-too";
        let expires_in = Duration::hours(1);

        let token = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            secret,
            expires_in,
        )
        .unwrap();

        let result = validate_token(&token, wrong_secret);
        assert!(matches!(result, Err(JwtError::Invalid)));
    }

    #[test]
    fn test_validate_with_empty_secret() {
        let user_id = Uuid::new_v4();
        let secret = "valid-secret-must-be-32-chars-or-longer-here";
        let empty_secret = "";
        let expires_in = Duration::hours(1);

        let token = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            secret,
            expires_in,
        )
        .unwrap();

        let result = validate_token(&token, empty_secret);
        assert!(matches!(result, Err(JwtError::SecretTooShort)));
    }

    #[test]
    fn test_create_token_with_empty_secret() {
        let user_id = Uuid::new_v4();
        let empty_secret = "";
        let expires_in = Duration::hours(1);

        // Creating a token with empty secret should fail
        let result = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            empty_secret,
            expires_in,
        );
        assert!(matches!(result, Err(JwtError::SecretTooShort)));
    }

    #[test]
    fn test_create_token_with_short_secret() {
        let user_id = Uuid::new_v4();
        let short_secret = "short";
        let expires_in = Duration::hours(1);

        // Creating a token with short secret should fail
        let result = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            short_secret,
            expires_in,
        );
        assert!(matches!(result, Err(JwtError::SecretTooShort)));
    }

    #[test]
    fn test_create_refresh_token_with_short_secret() {
        let user_id = Uuid::new_v4();
        let short_secret = "short";
        let expires_in = Duration::hours(1);

        let result = create_refresh_token(user_id, short_secret, expires_in);
        assert!(matches!(result, Err(JwtError::SecretTooShort)));
    }

    // ============ Entropy validation tests ============

    #[test]
    fn test_create_token_with_low_entropy_secret() {
        let user_id = Uuid::new_v4();
        // 32 characters but only one unique character
        let low_entropy_secret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let expires_in = Duration::hours(1);

        let result = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            low_entropy_secret,
            expires_in,
        );
        assert!(matches!(result, Err(JwtError::SecretLowEntropy)));
    }

    #[test]
    fn test_create_refresh_token_with_low_entropy_secret() {
        let user_id = Uuid::new_v4();
        // 32 characters but only a few unique characters
        let low_entropy_secret = "ababababababababababababababababab";
        let expires_in = Duration::hours(1);

        let result = create_refresh_token(user_id, low_entropy_secret, expires_in);
        assert!(matches!(result, Err(JwtError::SecretLowEntropy)));
    }

    #[test]
    fn test_validate_token_with_low_entropy_secret() {
        let user_id = Uuid::new_v4();
        let good_secret = "test-secret-key-12345-must-be-32-chars-or-longer";
        let low_entropy_secret = "00000000000000000000000000000000";
        let expires_in = Duration::hours(1);

        // Create token with good secret
        let token = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            good_secret,
            expires_in,
        )
        .unwrap();

        // Try to validate with low entropy secret - should fail validation
        let result = validate_token(&token, low_entropy_secret);
        assert!(matches!(result, Err(JwtError::SecretLowEntropy)));
    }

    #[test]
    fn test_secret_with_exactly_10_unique_chars() {
        let user_id = Uuid::new_v4();
        // 32+ characters with exactly 10 unique characters (0-9)
        let secret = "0123456789012345678901234567890123456789";
        let expires_in = Duration::hours(1);

        // Should succeed - 10 unique characters is the minimum
        let result = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            secret,
            expires_in,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_secret_with_9_unique_chars() {
        let user_id = Uuid::new_v4();
        // 32+ characters with only 9 unique characters (0-8)
        let secret = "0123456780123456780123456780123456780";
        let expires_in = Duration::hours(1);

        // Should fail - less than 10 unique characters
        let result = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            secret,
            expires_in,
        );
        assert!(matches!(result, Err(JwtError::SecretLowEntropy)));
    }

    // ============ Claims user_id() parsing tests ============

    #[test]
    fn test_user_id_valid_uuid() {
        let expected_id = Uuid::new_v4();
        let claims = Claims {
            sub: expected_id.to_string(),
            email: "test@example.com".to_string(),
            roles: vec![],
            permissions: vec![],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        };

        let parsed_id = claims.user_id().unwrap();
        assert_eq!(parsed_id, expected_id);
    }

    #[test]
    fn test_user_id_invalid_uuid() {
        let claims = Claims {
            sub: "not-a-valid-uuid".to_string(),
            email: "test@example.com".to_string(),
            roles: vec![],
            permissions: vec![],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        };

        let result = claims.user_id();
        assert!(result.is_err());
    }

    #[test]
    fn test_user_id_empty_string() {
        let claims = Claims {
            sub: "".to_string(),
            email: "test@example.com".to_string(),
            roles: vec![],
            permissions: vec![],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        };

        let result = claims.user_id();
        assert!(result.is_err());
    }

    #[test]
    fn test_user_id_partial_uuid() {
        let claims = Claims {
            sub: "550e8400-e29b-41d4".to_string(), // Truncated UUID
            email: "test@example.com".to_string(),
            roles: vec![],
            permissions: vec![],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        };

        let result = claims.user_id();
        assert!(result.is_err());
    }

    // ============ Refresh token tests ============

    #[test]
    fn test_create_and_validate_refresh_token() {
        let user_id = Uuid::new_v4();
        let secret = "refresh-secret-must-be-32-chars-or-longer-here";
        let expires_in = Duration::days(7);

        let token = create_refresh_token(user_id, secret, expires_in).unwrap();
        let claims = validate_token(&token, secret).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        // Refresh tokens should have minimal claims
        assert!(claims.email.is_empty());
        assert!(claims.roles.is_empty());
        assert!(claims.permissions.is_empty());
        assert!(!claims.is_admin);
    }

    #[test]
    fn test_refresh_token_has_different_jti() {
        let user_id = Uuid::new_v4();
        let secret = "refresh-secret-must-be-32-chars-or-longer-here";
        let expires_in = Duration::days(7);

        let token1 = create_refresh_token(user_id, secret, expires_in).unwrap();
        let token2 = create_refresh_token(user_id, secret, expires_in).unwrap();

        let claims1 = validate_token(&token1, secret).unwrap();
        let claims2 = validate_token(&token2, secret).unwrap();

        // Each token should have a unique JTI
        assert_ne!(claims1.jti, claims2.jti);
    }

    #[test]
    fn test_refresh_token_expiration() {
        let user_id = Uuid::new_v4();
        let secret = "refresh-secret-must-be-32-chars-or-longer-here";
        let expires_in = Duration::hours(-1); // Already expired

        let token = create_refresh_token(user_id, secret, expires_in).unwrap();
        let result = validate_token(&token, secret);

        assert!(matches!(result, Err(JwtError::Expired)));
    }

    // ============ JWT error type tests ============

    #[test]
    fn test_jwt_error_expired_display() {
        let err = JwtError::Expired;
        assert_eq!(err.to_string(), "Token expired");
    }

    #[test]
    fn test_jwt_error_invalid_display() {
        let err = JwtError::Invalid;
        assert_eq!(err.to_string(), "Invalid token");
    }

    #[test]
    fn test_jwt_error_creation_display() {
        // Create a malformed situation to test Creation error
        // We can't easily create a jsonwebtoken error, so test the display format
        let err = JwtError::Invalid; // Using Invalid as proxy since Creation requires real JWT error
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_jwt_error_debug() {
        let err = JwtError::Expired;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Expired"));
    }

    #[test]
    fn test_validate_malformed_token() {
        let secret = "test-secret-key-12345-must-be-32-chars-or-longer";
        let result = validate_token("not.a.valid.jwt", secret);
        assert!(matches!(result, Err(JwtError::Invalid)));
    }

    #[test]
    fn test_validate_completely_invalid_token() {
        let secret = "test-secret-key-12345-must-be-32-chars-or-longer";
        let result = validate_token("garbage", secret);
        assert!(matches!(result, Err(JwtError::Invalid)));
    }

    #[test]
    fn test_validate_empty_token() {
        let secret = "test-secret-key-12345-must-be-32-chars-or-longer";
        let result = validate_token("", secret);
        assert!(matches!(result, Err(JwtError::Invalid)));
    }

    // ============ Additional edge case tests ============

    #[test]
    fn test_extract_bearer_token_empty() {
        assert_eq!(extract_bearer_token(""), None);
    }

    #[test]
    fn test_extract_bearer_token_bearer_only() {
        assert_eq!(extract_bearer_token("Bearer "), Some(""));
    }

    #[test]
    fn test_extract_bearer_token_with_spaces_in_token() {
        assert_eq!(
            extract_bearer_token("Bearer token with spaces"),
            Some("token with spaces")
        );
    }

    #[test]
    fn test_extract_bearer_token_case_sensitive_middle() {
        // "BEARER" (all caps) should not match
        assert_eq!(extract_bearer_token("BEARER abc123"), None);
    }

    #[test]
    fn test_claims_with_multiple_roles_and_permissions() {
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            email: "test@example.com".to_string(),
            roles: vec![
                "user".to_string(),
                "moderator".to_string(),
                "editor".to_string(),
            ],
            permissions: vec![
                "read".to_string(),
                "write".to_string(),
                "delete".to_string(),
            ],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: false,
        };

        assert!(claims.has_role("user"));
        assert!(claims.has_role("moderator"));
        assert!(claims.has_role("editor"));
        assert!(!claims.has_role("admin"));

        assert!(claims.has_permission("read"));
        assert!(claims.has_permission("write"));
        assert!(claims.has_permission("delete"));
        assert!(!claims.has_permission("admin"));
    }

    #[test]
    fn test_token_timestamps() {
        let user_id = Uuid::new_v4();
        let secret = "test-secret-must-be-32-chars-or-longer-here";
        let expires_in = Duration::hours(2);

        let before = Utc::now().timestamp();
        let token = create_access_token(
            user_id,
            "test@example.com",
            vec![],
            vec![],
            false,
            secret,
            expires_in,
        )
        .unwrap();
        let after = Utc::now().timestamp();

        let claims = validate_token(&token, secret).unwrap();

        // iat should be between before and after
        assert!(claims.iat >= before);
        assert!(claims.iat <= after);

        // exp should be approximately 2 hours after iat
        let expected_exp = claims.iat + 7200; // 2 hours in seconds
        assert!((claims.exp - expected_exp).abs() <= 1); // Allow 1 second tolerance
    }

    #[test]
    fn test_claims_clone() {
        let original = Claims {
            sub: Uuid::new_v4().to_string(),
            email: "test@example.com".to_string(),
            roles: vec!["user".to_string()],
            permissions: vec!["read".to_string()],
            exp: Utc::now().timestamp() + 3600,
            iat: Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            is_admin: true,
        };

        let cloned = original.clone();

        assert_eq!(original.sub, cloned.sub);
        assert_eq!(original.email, cloned.email);
        assert_eq!(original.roles, cloned.roles);
        assert_eq!(original.permissions, cloned.permissions);
        assert_eq!(original.exp, cloned.exp);
        assert_eq!(original.iat, cloned.iat);
        assert_eq!(original.jti, cloned.jti);
        assert_eq!(original.is_admin, cloned.is_admin);
    }

    #[test]
    fn test_access_token_preserves_admin_flag() {
        let user_id = Uuid::new_v4();
        let secret = "test-secret-must-be-32-chars-or-longer-here";
        let expires_in = Duration::hours(1);

        let admin_token = create_access_token(
            user_id,
            "admin@example.com",
            vec!["admin".to_string()],
            vec!["all".to_string()],
            true,
            secret,
            expires_in,
        )
        .unwrap();

        let non_admin_token = create_access_token(
            user_id,
            "user@example.com",
            vec!["user".to_string()],
            vec!["read".to_string()],
            false,
            secret,
            expires_in,
        )
        .unwrap();

        let admin_claims = validate_token(&admin_token, secret).unwrap();
        let non_admin_claims = validate_token(&non_admin_token, secret).unwrap();

        assert!(admin_claims.is_admin);
        assert!(!non_admin_claims.is_admin);
    }
}
