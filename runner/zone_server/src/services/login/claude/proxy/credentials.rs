//! The username and password a proxy is signed in to with.

use std::fmt;

use abnegate_agent_cli::login::LoginError;
use abnegate_secret::SecretValue;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::header::HeaderValue;
use zeroize::Zeroizing;

const BASIC_SCHEME: &str = "Basic ";
const SEPARATOR: char = ':';
const UNREADABLE: &str = "The username and password of the server's proxy cannot be read";

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    username: String,
    password: SecretValue,
}

impl Credentials {
    pub fn new(username: &str, password: &str) -> Self {
        Self {
            username: username.to_string(),
            password: SecretValue::new(password),
        }
    }

    /// The credentials an HTTP Basic `header` carries, split where the proxy splits them: at the
    /// first colon. Sent on again, they are the header itself, byte for byte.
    pub fn basic(header: &HeaderValue) -> Result<Self, LoginError> {
        let unreadable = || LoginError::Malformed(UNREADABLE);
        let encoded = header
            .to_str()
            .ok()
            .and_then(|header| header.strip_prefix(BASIC_SCHEME))
            .ok_or_else(unreadable)?;
        let decoded = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| unreadable())?);
        let (username, password) = std::str::from_utf8(&decoded)
            .ok()
            .and_then(|decoded| decoded.split_once(SEPARATOR))
            .ok_or_else(unreadable)?;
        Ok(Self::new(username, password))
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &SecretValue {
        &self.password
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(credentials: &str) -> HeaderValue {
        HeaderValue::from_str(&format!("{BASIC_SCHEME}{}", STANDARD.encode(credentials)))
            .expect("a header")
    }

    #[test]
    fn basic_credentials_split_at_the_first_colon() {
        for (sent, username, password) in [
            ("zone:hunter2", "zone", "hunter2"),
            ("zone:p@ss:word", "zone", "p@ss:word"),
            ("zone:", "zone", ""),
            (":", "", ""),
        ] {
            let credentials = Credentials::basic(&header(sent)).expect("credentials");

            assert_eq!(credentials.username(), username, "{sent}");
            assert_eq!(credentials.password().expose(), password, "{sent}");
        }
    }

    #[test]
    fn a_header_that_is_not_basic_credentials_is_unreadable() {
        for header in [
            HeaderValue::from_static("Bearer abc"),
            HeaderValue::from_static("Basic !!!"),
            header("no-separator"),
        ] {
            assert!(
                matches!(
                    Credentials::basic(&header),
                    Err(LoginError::Malformed(UNREADABLE))
                ),
                "{header:?}"
            );
        }
    }

    #[test]
    fn credentials_never_print_their_password() {
        let credentials = Credentials::new("zone", "hunter2");

        assert!(!format!("{credentials:?}").contains("hunter2"));
    }
}
