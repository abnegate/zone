//! Claude's own headless sign-in, as `claude setup-token` runs it.

mod authorization;
mod client;
mod code;
mod error;
mod scope;
mod tokens;

use reqwest::Url;

pub use authorization::Authorization;
pub use client::Client;
pub use code::Code;
pub use error::Error;
pub use scope::Scope;
pub use tokens::Tokens;

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
pub const REDIRECT_URL: &str = "https://platform.claude.com/oauth/code/callback";
pub const LIFETIME: u64 = 31_536_000;

pub fn token_endpoint(url: &str) -> Result<Url, Error> {
    let parsed = Url::parse(url).map_err(|_| {
        Error::Malformed("The Claude token URL must be an absolute URL with a host")
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::Malformed(
            "The Claude token URL must use http or https",
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Error::Malformed(
            "The Claude token URL must not include userinfo",
        ));
    }
    if parsed.host_str().is_none() {
        return Err(Error::Malformed("The Claude token URL must include a host"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_endpoint_hands_back_the_url_it_checked() {
        for url in [
            "https://platform.claude.com/v1/oauth/token",
            "http://127.0.0.1:4010/v1/oauth/token",
        ] {
            let checked = token_endpoint(url).unwrap_or_else(|error| panic!("{url}: {error}"));
            assert_eq!(checked.as_str(), url);
        }
    }

    #[test]
    fn the_token_endpoint_refuses_other_schemes_userinfo_and_a_missing_host() {
        for (url, rule) in [
            ("ftp://platform.claude.com/v1/oauth/token", "http or https"),
            ("file:///etc/passwd", "http or https"),
            (
                "https://user:secret@platform.claude.com/v1/oauth/token",
                "userinfo",
            ),
            (
                "https://user@platform.claude.com/v1/oauth/token",
                "userinfo",
            ),
            ("https://", "host"),
            ("/v1/oauth/token", "host"),
        ] {
            match token_endpoint(url) {
                Err(Error::Malformed(message)) => {
                    assert!(message.contains(rule), "{url}: {message}")
                }
                other => panic!("{url} must be refused for its {rule}, got {other:?}"),
            }
        }
    }
}
