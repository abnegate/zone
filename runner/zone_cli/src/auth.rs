//! Authentication management
//!
//! Handles login/logout via the hosted manager and stores credentials
//! securely in the OS keychain.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SERVICE_NAME: &str = "zone-cli";
const ACCESS_TOKEN_KEY: &str = "access-token";
const REFRESH_TOKEN_KEY: &str = "refresh-token";
const METADATA_KEY: &str = "metadata";

/// Authentication error
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Not logged in")]
    NotLoggedIn,

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Server error: {0}")]
    Server(String),
}

/// Stored token metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    pub host: String,
    pub expires_at: i64,
    pub user_id: String,
    pub email: String,
}

/// Authentication manager
pub struct AuthManager {
    client: reqwest::Client,
}

impl AuthManager {
    /// Create a new auth manager
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Log in to a Zone server
    pub async fn login(
        &self,
        host: &str,
        email: &str,
        password: &str,
    ) -> Result<TokenMetadata, AuthError> {
        #[derive(Serialize)]
        struct LoginRequest<'a> {
            email: &'a str,
            password: &'a str,
        }

        #[derive(Deserialize)]
        struct LoginResponse {
            success: bool,
            data: Option<LoginData>,
            error: Option<String>,
        }

        #[derive(Deserialize)]
        struct LoginData {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
            user: UserInfo,
        }

        #[derive(Deserialize)]
        struct UserInfo {
            id: String,
            email: String,
        }

        let url = format!("{}/api/auth/login", host.trim_end_matches('/'));
        let response: LoginResponse = self
            .client
            .post(&url)
            .json(&LoginRequest { email, password })
            .send()
            .await?
            .json()
            .await?;

        if !response.success {
            return Err(AuthError::Server(
                response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        let data = response.data.ok_or(AuthError::InvalidCredentials)?;
        let expires_at = chrono::Utc::now().timestamp() + data.expires_in;

        // Store tokens in keychain
        self.store_token(ACCESS_TOKEN_KEY, &data.access_token)?;
        self.store_token(REFRESH_TOKEN_KEY, &data.refresh_token)?;

        let metadata = TokenMetadata {
            host: host.to_string(),
            expires_at,
            user_id: data.user.id,
            email: data.user.email,
        };
        self.store_token(METADATA_KEY, &serde_json::to_string(&metadata)?)?;

        Ok(metadata)
    }

    /// Log out and clear credentials
    pub fn logout(&self) -> Result<(), AuthError> {
        self.delete_token(ACCESS_TOKEN_KEY).ok();
        self.delete_token(REFRESH_TOKEN_KEY).ok();
        self.delete_token(METADATA_KEY).ok();
        Ok(())
    }

    /// Get the current access token, refreshing if needed
    pub async fn get_access_token(&self) -> Result<String, AuthError> {
        let metadata = self.get_metadata()?;

        // Check if token is expired (with 60s buffer)
        let now = chrono::Utc::now().timestamp();
        if now >= metadata.expires_at - 60 {
            return self.refresh_token().await;
        }

        self.get_token(ACCESS_TOKEN_KEY)
    }

    /// Refresh the access token
    async fn refresh_token(&self) -> Result<String, AuthError> {
        let metadata = self.get_metadata()?;
        let refresh_token = self.get_token(REFRESH_TOKEN_KEY)?;

        #[derive(Serialize)]
        struct RefreshRequest<'a> {
            refresh_token: &'a str,
        }

        #[derive(Deserialize)]
        struct RefreshResponse {
            success: bool,
            data: Option<RefreshData>,
        }

        #[derive(Deserialize)]
        struct RefreshData {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
        }

        let url = format!("{}/api/auth/refresh", metadata.host.trim_end_matches('/'));
        let response: RefreshResponse = self
            .client
            .post(&url)
            .json(&RefreshRequest {
                refresh_token: &refresh_token,
            })
            .send()
            .await?
            .json()
            .await?;

        if !response.success {
            // Clear tokens on refresh failure
            self.logout()?;
            return Err(AuthError::TokenExpired);
        }

        let data = response.data.ok_or(AuthError::TokenExpired)?;
        let expires_at = chrono::Utc::now().timestamp() + data.expires_in;

        // Store new tokens
        self.store_token(ACCESS_TOKEN_KEY, &data.access_token)?;
        self.store_token(REFRESH_TOKEN_KEY, &data.refresh_token)?;

        let new_metadata = TokenMetadata {
            expires_at,
            ..metadata
        };
        self.store_token(METADATA_KEY, &serde_json::to_string(&new_metadata)?)?;

        Ok(data.access_token)
    }

    /// Get stored metadata
    pub fn get_metadata(&self) -> Result<TokenMetadata, AuthError> {
        let json = self.get_token(METADATA_KEY)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Check if user is logged in
    pub fn is_logged_in(&self) -> bool {
        self.get_token(ACCESS_TOKEN_KEY).is_ok()
    }

    fn store_token(&self, key: &str, value: &str) -> Result<(), AuthError> {
        let entry = Entry::new(SERVICE_NAME, key)?;
        entry.set_password(value)?;
        Ok(())
    }

    fn get_token(&self, key: &str) -> Result<String, AuthError> {
        let entry = Entry::new(SERVICE_NAME, key)?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => AuthError::NotLoggedIn,
            _ => AuthError::Keyring(e),
        })
    }

    fn delete_token(&self, key: &str) -> Result<(), AuthError> {
        let entry = Entry::new(SERVICE_NAME, key)?;
        entry.delete_credential()?;
        Ok(())
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_error_not_logged_in() {
        let err = AuthError::NotLoggedIn;
        assert_eq!(err.to_string(), "Not logged in");
    }

    #[test]
    fn test_auth_error_token_expired() {
        let err = AuthError::TokenExpired;
        assert_eq!(err.to_string(), "Token expired");
    }

    #[test]
    fn test_auth_error_invalid_credentials() {
        let err = AuthError::InvalidCredentials;
        assert_eq!(err.to_string(), "Invalid credentials");
    }

    #[test]
    fn test_auth_error_server() {
        let err = AuthError::Server("Connection refused".to_string());
        assert_eq!(err.to_string(), "Server error: Connection refused");
    }

    #[test]
    fn test_auth_error_json() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err: AuthError = json_err.into();
        assert!(matches!(err, AuthError::Json(_)));
        assert!(err.to_string().contains("JSON error"));
    }

    #[test]
    fn test_token_metadata_serialization() {
        let metadata = TokenMetadata {
            host: "https://zone.example.com".to_string(),
            expires_at: 1700000000,
            user_id: "user-123".to_string(),
            email: "test@example.com".to_string(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("zone.example.com"));
        assert!(json.contains("1700000000"));
        assert!(json.contains("user-123"));
        assert!(json.contains("test@example.com"));
    }

    #[test]
    fn test_token_metadata_deserialization() {
        let json = r#"{
            "host": "https://api.zone.io",
            "expires_at": 1800000000,
            "user_id": "abc-456",
            "email": "user@zone.io"
        }"#;

        let metadata: TokenMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(metadata.host, "https://api.zone.io");
        assert_eq!(metadata.expires_at, 1800000000);
        assert_eq!(metadata.user_id, "abc-456");
        assert_eq!(metadata.email, "user@zone.io");
    }

    #[test]
    fn test_token_metadata_clone() {
        let metadata = TokenMetadata {
            host: "https://zone.example.com".to_string(),
            expires_at: 1700000000,
            user_id: "user-123".to_string(),
            email: "test@example.com".to_string(),
        };

        let cloned = metadata.clone();

        assert_eq!(metadata.host, cloned.host);
        assert_eq!(metadata.expires_at, cloned.expires_at);
        assert_eq!(metadata.user_id, cloned.user_id);
        assert_eq!(metadata.email, cloned.email);
    }

    #[test]
    fn test_token_metadata_debug() {
        let metadata = TokenMetadata {
            host: "https://zone.example.com".to_string(),
            expires_at: 1700000000,
            user_id: "user-123".to_string(),
            email: "test@example.com".to_string(),
        };

        let debug_str = format!("{:?}", metadata);
        assert!(debug_str.contains("TokenMetadata"));
        assert!(debug_str.contains("zone.example.com"));
    }

    #[test]
    fn test_token_metadata_roundtrip() {
        let original = TokenMetadata {
            host: "https://zone.test.com".to_string(),
            expires_at: 1900000000,
            user_id: "xyz-789".to_string(),
            email: "admin@zone.test.com".to_string(),
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: TokenMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(original.host, deserialized.host);
        assert_eq!(original.expires_at, deserialized.expires_at);
        assert_eq!(original.user_id, deserialized.user_id);
        assert_eq!(original.email, deserialized.email);
    }

    #[test]
    fn test_auth_manager_new() {
        let manager = AuthManager::new();
        // Just verify it can be created
        assert!(std::mem::size_of_val(&manager) > 0);
    }

    #[test]
    fn test_auth_manager_default() {
        let manager = AuthManager::default();
        // Just verify default works
        assert!(std::mem::size_of_val(&manager) > 0);
    }

    #[test]
    fn test_service_name_constant() {
        assert_eq!(SERVICE_NAME, "zone-cli");
    }

    #[test]
    fn test_token_key_constants() {
        assert_eq!(ACCESS_TOKEN_KEY, "access-token");
        assert_eq!(REFRESH_TOKEN_KEY, "refresh-token");
        assert_eq!(METADATA_KEY, "metadata");
    }

    #[test]
    fn test_token_metadata_with_different_hosts() {
        let hosts = [
            "http://localhost:8000",
            "https://zone.example.com",
            "https://api.zone.io:443",
            "http://192.168.1.100:3000",
        ];

        for host in hosts {
            let metadata = TokenMetadata {
                host: host.to_string(),
                expires_at: 1700000000,
                user_id: "user".to_string(),
                email: "test@test.com".to_string(),
            };

            let json = serde_json::to_string(&metadata).unwrap();
            let parsed: TokenMetadata = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.host, host);
        }
    }

    #[test]
    fn test_token_metadata_expires_at_values() {
        // Test with various expiration timestamps
        let timestamps = [0i64, 1000000000, 1700000000, 2000000000];

        for ts in timestamps {
            let metadata = TokenMetadata {
                host: "https://zone.example.com".to_string(),
                expires_at: ts,
                user_id: "user".to_string(),
                email: "test@test.com".to_string(),
            };

            let json = serde_json::to_string(&metadata).unwrap();
            let parsed: TokenMetadata = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.expires_at, ts);
        }
    }

    #[test]
    fn test_auth_error_debug() {
        let errors = vec![
            AuthError::NotLoggedIn,
            AuthError::TokenExpired,
            AuthError::InvalidCredentials,
            AuthError::Server("test".to_string()),
        ];

        for err in errors {
            let debug_str = format!("{:?}", err);
            assert!(!debug_str.is_empty());
        }
    }

    // Note: We can't easily test the actual keyring operations in unit tests
    // because they require OS-specific keychain access. Those would be
    // integration tests that run with actual keychain permissions.
    //
    // The login/refresh/logout methods also require a running server,
    // so they would be tested in integration tests with a mock server.
}
