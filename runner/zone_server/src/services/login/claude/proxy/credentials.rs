//! The username and password a proxy is signed in to with.

use std::fmt;

use abnegate_secret::SecretValue;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::header::HeaderValue;
use zeroize::Zeroizing;

const BASIC_SCHEME: &str = "Basic ";
const SEPARATOR: char = ':';

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    username: String,
    password: SecretValue,
}

impl Credentials {
    fn new(username: &str, password: &str) -> Self {
        Self {
            username: username.to_string(),
            password: SecretValue::new(password),
        }
    }

    /// The credentials an HTTP Basic `header` carries, split where the proxy splits them: at the
    /// first colon. Sent on again, they are the header itself, byte for byte. `None` for a header
    /// that is not HTTP Basic credentials.
    pub fn basic(header: &HeaderValue) -> Option<Self> {
        let encoded = header.to_str().ok()?.strip_prefix(BASIC_SCHEME)?;
        let decoded = Zeroizing::new(STANDARD.decode(encoded).ok()?);
        let (username, password) = std::str::from_utf8(&decoded).ok()?.split_once(SEPARATOR)?;
        Some(Self::new(username, password))
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
            assert!(Credentials::basic(&header).is_none(), "{header:?}");
        }
    }

    #[test]
    fn credentials_never_print_their_password() {
        let credentials = Credentials::new("zone", "hunter2");

        assert!(!format!("{credentials:?}").contains("hunter2"));
    }
}
