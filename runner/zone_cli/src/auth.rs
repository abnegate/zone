//! Authentication management
//!
//! Handles login/logout via the hosted manager and stores credentials
//! securely in the OS keychain.

use abnegate_config::{Application, TokenMetadata, TokenStore};
use abnegate_secret::SecretValue;
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The keychain service the CLI's tokens are stored under.
const SERVICE: &str = "zone-cli";

/// How long before its expiry an access token is refreshed rather than used.
const REFRESH_LEEWAY: TimeDelta = TimeDelta::seconds(60);

/// Authentication error
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Not logged in")]
    NotLoggedIn,

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Credential store error: {0}")]
    Store(abnegate_config::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Server error: {0}")]
    Server(String),
}

impl From<abnegate_config::Error> for AuthError {
    fn from(error: abnegate_config::Error) -> Self {
        match error {
            abnegate_config::Error::NoCredential { .. } => AuthError::NotLoggedIn,
            other => AuthError::Store(other),
        }
    }
}

/// The body `POST /api/auth/login` and `POST /api/auth/refresh` answer with:
/// the tokens at the top level beside the user, roles and permissions. Only
/// the fields the CLI keeps are named.
#[derive(Debug, Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    user: Option<User>,
}

impl Tokens {
    fn expires_at(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now + TimeDelta::seconds(self.expires_in)
    }

    fn store(self, store: &TokenStore) -> Result<SecretValue, AuthError> {
        let access_token = SecretValue::new(self.access_token);
        store.set_access_token(&access_token)?;
        store.set_refresh_token(&SecretValue::new(self.refresh_token))?;
        Ok(access_token)
    }
}

#[derive(Debug, Deserialize)]
struct User {
    id: String,
    email: String,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: String,
}

/// A 401 is refused credentials, any other failure carries `{"error": ...}`,
/// and success is the flat token body.
fn parse_tokens(status: reqwest::StatusCode, body: &str) -> Result<Tokens, AuthError> {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AuthError::InvalidCredentials);
    }
    if !status.is_success() {
        let message = serde_json::from_str::<ErrorBody>(body)
            .map(|error| error.error)
            .unwrap_or_else(|_| format!("HTTP {status}"));
        return Err(AuthError::Server(message));
    }
    Ok(serde_json::from_str(body)?)
}

/// The keychain entries the CLI's tokens live in.
fn token_store() -> Result<TokenStore, AuthError> {
    let application = Application::new(SERVICE).map_err(abnegate_config::Error::from)?;
    Ok(TokenStore::new(application))
}

/// Authentication manager
pub struct AuthManager {
    client: reqwest::Client,
    store: TokenStore,
}

impl AuthManager {
    /// Create a new auth manager
    pub fn new() -> Result<Self, AuthError> {
        Ok(Self {
            client: reqwest::Client::new(),
            store: token_store()?,
        })
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

        let host = host.trim_end_matches('/');
        let response = self
            .client
            .post(format!("{host}/api/auth/login"))
            .json(&LoginRequest { email, password })
            .send()
            .await?;
        let status = response.status();
        let mut tokens = parse_tokens(status, &response.text().await?)?;
        let expires_at = tokens.expires_at(Utc::now());
        let user = tokens
            .user
            .take()
            .ok_or_else(|| AuthError::Server("The login response carried no user".to_string()))?;

        tokens.store(&self.store)?;

        let mut metadata = TokenMetadata::new(host, expires_at);
        metadata.user_id = Some(user.id);
        metadata.email = Some(user.email);
        self.store.set_metadata(&metadata)?;

        match crate::config::Config::load() {
            Ok(mut cfg) => {
                cfg.host = Some(host.to_string());
                if let Err(err) = cfg.save() {
                    eprintln!("warning: could not save host to config: {err}");
                }
            }
            Err(err) => eprintln!("warning: could not load config to save host: {err}"),
        }

        Ok(metadata)
    }

    /// Log out and clear credentials
    pub fn logout(&self) -> Result<(), AuthError> {
        Ok(self.store.clear()?)
    }

    /// Get the current access token, refreshing if needed
    pub async fn get_access_token(&self) -> Result<SecretValue, AuthError> {
        let metadata = self.get_metadata()?;

        if metadata.expires_within(REFRESH_LEEWAY) {
            return self.refresh_token(metadata).await;
        }

        Ok(self.store.access_token()?)
    }

    /// Refresh the access token
    async fn refresh_token(&self, metadata: TokenMetadata) -> Result<SecretValue, AuthError> {
        let refresh_token = self.store.refresh_token()?;

        #[derive(Serialize)]
        struct RefreshRequest<'a> {
            refresh_token: &'a str,
        }

        let url = format!("{}/api/auth/refresh", metadata.host.trim_end_matches('/'));
        let response = self
            .client
            .post(&url)
            .json(&RefreshRequest {
                refresh_token: refresh_token.expose(),
            })
            .send()
            .await?;
        let status = response.status();
        let tokens = match parse_tokens(status, &response.text().await?) {
            Ok(tokens) => tokens,
            Err(AuthError::InvalidCredentials) => {
                self.logout()?;
                return Err(AuthError::TokenExpired);
            }
            Err(error) => return Err(error),
        };

        let mut refreshed = metadata;
        refreshed.expires_at = tokens.expires_at(Utc::now());
        let access_token = tokens.store(&self.store)?;
        self.store.set_metadata(&refreshed)?;

        Ok(access_token)
    }

    /// Get stored metadata
    pub fn get_metadata(&self) -> Result<TokenMetadata, AuthError> {
        Ok(self.store.metadata()?)
    }

    /// Check if user is logged in
    pub fn is_logged_in(&self) -> bool {
        self.store.is_authenticated()
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

    /// Tokens a CLI stored before the shared token store keep working only
    /// while the keychain service they were written under is the one read.
    #[test]
    fn tokens_are_kept_under_the_keychain_service_they_were_always_written_to() {
        assert_eq!(token_store().unwrap().service(), "zone-cli");
    }

    /// What an earlier CLI wrote to the keychain, entry by entry, has to be
    /// what the shared store reads, or every signed-in user is signed out by
    /// the upgrade. The platform keychain is swapped for keyring's in-memory
    /// store, which the earlier CLI's `Entry` writes go through as well.
    /// keyring initialises the platform store before any other, and on Linux
    /// that needs a running Secret Service, so this runs on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_keychain_entries_an_earlier_cli_wrote_still_sign_the_user_in() {
        keyring::Entry::store_status()
            .as_ref()
            .expect("the platform store initialises before it is replaced");
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let manager = AuthManager::new().unwrap();

        assert!(!manager.is_logged_in());
        assert!(matches!(
            manager.get_metadata(),
            Err(AuthError::NotLoggedIn)
        ));

        let access_token = concat!("eyJhbGciOiJIUzI1NiJ9", ".access");
        let refresh_token = concat!("3f1c2b7a", "9e");
        let metadata = r#"{"host":"https://api.zone.io","expires_at":4102444800,"user_id":"abc-456","email":"user@zone.io"}"#;
        for (name, value) in [
            ("access-token", access_token),
            ("refresh-token", refresh_token),
            ("metadata", metadata),
        ] {
            keyring::Entry::new("zone-cli", name)
                .unwrap()
                .set_password(value)
                .unwrap();
        }

        assert!(manager.is_logged_in());
        let read = manager.get_metadata().unwrap();
        assert_eq!(read.host, "https://api.zone.io");
        assert_eq!(read.email.as_deref(), Some("user@zone.io"));
        assert_eq!(manager.store.access_token().unwrap().expose(), access_token);
        assert_eq!(
            manager.store.refresh_token().unwrap().expose(),
            refresh_token
        );

        manager.logout().unwrap();

        assert!(!manager.is_logged_in());
        assert!(
            keyring::Entry::new("zone-cli", "metadata")
                .unwrap()
                .get_password()
                .is_err()
        );
    }

    /// The metadata a CLI stored before the shared token store, byte for
    /// byte, so a signed-in user stays signed in across the upgrade.
    #[test]
    fn metadata_stored_by_an_earlier_cli_still_reads() {
        let stored = r#"{"host":"https://api.zone.io","expires_at":1800000000,"user_id":"abc-456","email":"user@zone.io"}"#;

        let metadata: TokenMetadata = serde_json::from_str(stored).unwrap();

        assert_eq!(metadata.host, "https://api.zone.io");
        assert_eq!(metadata.expires_at.timestamp(), 1_800_000_000);
        assert_eq!(metadata.user_id.as_deref(), Some("abc-456"));
        assert_eq!(metadata.email.as_deref(), Some("user@zone.io"));
        assert_eq!(serde_json::to_string(&metadata).unwrap(), stored);
    }

    #[test]
    fn a_token_is_refreshed_within_a_minute_of_its_expiry() {
        let soon = TokenMetadata::new("https://zone.test", Utc::now() + TimeDelta::seconds(59));
        let later = TokenMetadata::new("https://zone.test", Utc::now() + TimeDelta::seconds(120));

        assert!(soon.expires_within(REFRESH_LEEWAY));
        assert!(!later.expires_within(REFRESH_LEEWAY));
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

    /// A literal copy of what `POST /api/auth/login` answers, so the CLI is
    /// tested against the shape the server sends and not one it imagined.
    const LOGIN_BODY: &str = r#"{
        "access_token": "eyJhbGciOiJIUzI1NiJ9.access",
        "refresh_token": "3f1c2b7a9e",
        "token_type": "Bearer",
        "expires_in": 900,
        "user": {
            "id": "0d9f4a2e-6b1c-4e3a-9f2d-1a2b3c4d5e6f",
            "email": "owner@zone.test",
            "display_name": "Owner",
            "is_admin": false,
            "is_active": true,
            "email_verified": true,
            "created_at": "2026-09-20T01:02:03Z",
            "updated_at": "2026-09-20T01:02:03Z",
            "last_login_at": null
        },
        "roles": ["member"],
        "permissions": ["chats:read", "chats:write"]
    }"#;

    #[test]
    fn login_body_is_read_flat_as_the_server_sends_it() {
        let tokens = parse_tokens(reqwest::StatusCode::OK, LOGIN_BODY).unwrap();
        assert_eq!(tokens.access_token, "eyJhbGciOiJIUzI1NiJ9.access");
        assert_eq!(tokens.refresh_token, "3f1c2b7a9e");
        let now = DateTime::from_timestamp(1_000, 0).unwrap();
        assert_eq!(tokens.expires_at(now).timestamp(), 1_900);
        let user = tokens.user.unwrap();
        assert_eq!(user.id, "0d9f4a2e-6b1c-4e3a-9f2d-1a2b3c4d5e6f");
        assert_eq!(user.email, "owner@zone.test");
    }

    #[test]
    fn refresh_body_needs_no_user() {
        let body =
            r#"{"access_token":"a","refresh_token":"r","token_type":"Bearer","expires_in":900}"#;
        let tokens = parse_tokens(reqwest::StatusCode::OK, body).unwrap();
        assert_eq!(tokens.access_token, "a");
        assert!(tokens.user.is_none());
    }

    #[test]
    fn a_401_is_refused_credentials_and_other_failures_carry_the_server_message() {
        let refused = parse_tokens(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":"Invalid email or password"}"#,
        );
        assert!(matches!(refused, Err(AuthError::InvalidCredentials)));

        let disabled = parse_tokens(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"error":"Account is disabled"}"#,
        );
        assert_eq!(
            disabled.unwrap_err().to_string(),
            "Server error: Account is disabled"
        );

        let opaque = parse_tokens(reqwest::StatusCode::BAD_GATEWAY, "<html>bad gateway</html>");
        assert_eq!(
            opaque.unwrap_err().to_string(),
            "Server error: HTTP 502 Bad Gateway"
        );
    }

    #[test]
    fn the_old_wrapped_shape_is_not_what_the_server_sends() {
        let wrapped = r#"{"success":true,"data":{"access_token":"a"}}"#;
        assert!(matches!(
            parse_tokens(reqwest::StatusCode::OK, wrapped),
            Err(AuthError::Json(_))
        ));
    }
}
