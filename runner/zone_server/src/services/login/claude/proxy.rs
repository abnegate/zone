//! The proxy a Claude sign-in reaches its token endpoint through, and who it signs in to the proxy
//! as.

mod credentials;

use abnegate_agent_cli::login::LoginError;
use abnegate_agent_cli::login::claude::Client;
use hyper_util::client::proxy::matcher::Intercept;

use credentials::Credentials;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    url: String,
    credentials: Option<Credentials>,
}

impl Proxy {
    /// The proxy `intercept` chose, signed in to with the username and password the server's
    /// proxy variable carried for it.
    pub fn intercepted(intercept: &Intercept) -> Result<Self, LoginError> {
        let credentials = match (intercept.basic_auth(), intercept.raw_auth()) {
            (Some(header), _) => Some(Credentials::basic(header)?),
            (None, Some((username, password))) => Some(Credentials::new(username, password)),
            (None, None) => None,
        };
        Ok(Self {
            url: intercept.uri().to_string(),
            credentials,
        })
    }

    /// `client`, sending every request through this proxy.
    pub fn through(&self, client: Client) -> Result<Client, LoginError> {
        match &self.credentials {
            Some(credentials) => client.with_authenticated_proxy(
                &self.url,
                credentials.username(),
                credentials.password().clone(),
            ),
            None => client.with_proxy(&self.url),
        }
    }
}

#[cfg(test)]
mod tests {
    use hyper_util::client::proxy::matcher::Matcher;

    use super::*;

    const TOKEN: &str = "https://platform.claude.com/v1/oauth/token";
    const PASSWORD: &str = "hunter2-proxy-password";

    fn chosen(proxy: &str) -> Proxy {
        let intercept = Matcher::builder()
            .https(proxy)
            .build()
            .intercept(&TOKEN.parse().expect("a token URI"))
            .expect("a proxy for the token endpoint");
        Proxy::intercepted(&intercept).expect("a proxy")
    }

    #[test]
    fn a_proxy_keeps_its_credentials_out_of_its_url_and_debug() {
        let proxy = chosen(&format!("http://zone:{PASSWORD}@proxy.internal:3128"));

        assert_eq!(proxy.url, "http://proxy.internal:3128/");
        assert!(proxy.credentials.is_some());
        assert!(!format!("{proxy:?}").contains(PASSWORD), "{proxy:?}");
    }

    #[test]
    fn a_proxy_named_without_credentials_carries_none() {
        let proxy = chosen("http://proxy.internal:3128");

        assert_eq!(proxy.credentials, None);
    }

    #[test]
    fn a_changed_password_is_another_proxy() {
        assert_ne!(
            chosen(&format!("http://zone:{PASSWORD}@proxy.internal:3128")),
            chosen("http://zone:another-password@proxy.internal:3128")
        );
        assert_eq!(
            chosen(&format!("http://zone:{PASSWORD}@proxy.internal:3128")),
            chosen(&format!("http://zone:{PASSWORD}@proxy.internal:3128"))
        );
    }
}
