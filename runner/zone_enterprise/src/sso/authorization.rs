use std::fmt;

use url::Url;
use url::form_urlencoded::Serializer;

use crate::sso::client_id::ClientId;
use crate::sso::pkce::{self, Pkce};
use crate::sso::{REDACTED, random};

pub const SCOPE: &str = "openid email profile";

const RESPONSE_TYPE: &str = "code";
const FORM_SPACE: &str = "+";
const PERCENT_SPACE: &str = "%20";

#[derive(Clone, PartialEq, Eq)]
pub struct Authorization {
    pub url: Url,
    pub state: String,
    pub nonce: String,
    pub verifier: String,
}

impl Authorization {
    pub fn new(endpoint: &Url, client_id: &ClientId, redirect_uri: &str) -> Self {
        let state = random::token();
        let nonce = random::token();
        let pkce = Pkce::generate();
        let mut url = endpoint.clone();
        let mut query = Serializer::new(url.query().unwrap_or_default().to_owned());
        query
            .append_pair("response_type", RESPONSE_TYPE)
            .append_pair("client_id", client_id.as_str())
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", SCOPE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", pkce::METHOD);
        // Form encoding writes a literal '+' as %2B, so every remaining '+' is a space.
        url.set_query(Some(&query.finish().replace(FORM_SPACE, PERCENT_SPACE)));
        Self {
            url,
            state,
            nonce,
            verifier: pkce.verifier,
        }
    }
}

impl fmt::Debug for Authorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut endpoint = self.url.clone();
        endpoint.set_query(None);
        formatter
            .debug_struct("Authorization")
            .field("endpoint", &endpoint.as_str())
            .field("state", &REDACTED)
            .field("nonce", &REDACTED)
            .field("verifier", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const REDIRECT: &str = "https://zone.example.com/sso/callback";

    fn authorization(endpoint: &str) -> Authorization {
        Authorization::new(
            &Url::parse(endpoint).unwrap(),
            &ClientId::parse("0oa1+zone").unwrap(),
            REDIRECT,
        )
    }

    fn parameters(authorization: &Authorization) -> HashMap<String, String> {
        authorization.url.query_pairs().into_owned().collect()
    }

    #[test]
    fn carries_every_parameter() {
        let authorization = authorization("https://acme.okta.com/oauth2/v1/authorize");
        let parameters = parameters(&authorization);
        assert_eq!(parameters["response_type"], "code");
        assert_eq!(parameters["client_id"], "0oa1+zone");
        assert_eq!(parameters["redirect_uri"], REDIRECT);
        assert_eq!(parameters["scope"], "openid email profile");
        assert_eq!(parameters["state"], authorization.state);
        assert_eq!(parameters["nonce"], authorization.nonce);
        assert_eq!(
            parameters["code_challenge"],
            pkce::challenge(&authorization.verifier)
        );
        assert_eq!(parameters["code_challenge_method"], "S256");
        assert_eq!(parameters.len(), 8);
    }

    #[test]
    fn keeps_the_endpoint_and_encodes_spaces_as_percent_twenty() {
        let authorization = authorization("https://acme.okta.com/oauth2/v1/authorize");
        let url = authorization.url.as_str();
        assert!(url.starts_with("https://acme.okta.com/oauth2/v1/authorize?response_type=code&client_id=0oa1%2Bzone&redirect_uri="));
        assert!(url.contains("&scope=openid%20email%20profile&"), "{url}");
        assert!(!url.contains('+'), "{url}");
    }

    #[test]
    fn keeps_an_existing_endpoint_query() {
        let authorization = authorization("https://idp.acme.test/authorize?tenant=acme");
        assert_eq!(parameters(&authorization)["tenant"], "acme");
        assert!(
            authorization
                .url
                .as_str()
                .starts_with("https://idp.acme.test/authorize?tenant=acme&response_type=code")
        );
    }

    #[test]
    fn state_nonce_and_verifier_are_fresh_and_distinct() {
        let first = authorization("https://idp.acme.test/authorize");
        let second = authorization("https://idp.acme.test/authorize");
        assert_ne!(first.state, second.state);
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.state, first.nonce);
    }

    #[test]
    fn debug_never_shows_the_state_nonce_or_verifier() {
        let authorization = authorization("https://idp.acme.test/authorize");
        let shown = format!("{authorization:?}");
        for secret in [
            &authorization.state,
            &authorization.nonce,
            &authorization.verifier,
        ] {
            assert!(!shown.contains(secret.as_str()), "{shown}");
        }
        assert!(shown.contains("https://idp.acme.test/authorize"), "{shown}");
    }
}
