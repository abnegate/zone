use serde::Deserialize;
use url::Url;

use crate::sso::client_authentication::ClientAuthentication;
use crate::sso::error::Error;
use crate::sso::issuer::{Issuer, endpoint_allowed};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub keys_endpoint: Url,
    pub userinfo_endpoint: Option<Url>,
    pub signing_algorithms: Option<Vec<String>>,
    pub authentication_methods: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Document {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    userinfo_endpoint: Option<String>,
    #[serde(default)]
    id_token_signing_alg_values_supported: Option<Vec<String>>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
}

impl Discovery {
    pub fn parse(body: &[u8], issuer: &Issuer) -> Result<Self, Error> {
        let document: Document = serde_json::from_slice(body).map_err(|error| {
            Error::Discovery(format!(
                "the discovery document isn't valid OpenID Connect metadata ({error})."
            ))
        })?;
        if document.issuer != issuer.as_str() {
            return Err(Error::Discovery(format!(
                "the discovery document names the issuer {}, not {issuer}.",
                document.issuer
            )));
        }
        Ok(Self {
            authorization_endpoint: endpoint(
                "authorization endpoint",
                &document.authorization_endpoint,
            )?,
            token_endpoint: endpoint("token endpoint", &document.token_endpoint)?,
            keys_endpoint: endpoint("JWKS URI", &document.jwks_uri)?,
            userinfo_endpoint: document
                .userinfo_endpoint
                .as_deref()
                .map(|value| endpoint("userinfo endpoint", value))
                .transpose()?,
            issuer: document.issuer,
            signing_algorithms: document.id_token_signing_alg_values_supported,
            authentication_methods: document.token_endpoint_auth_methods_supported,
        })
    }

    pub fn authentication(&self) -> ClientAuthentication {
        ClientAuthentication::choose(self.authentication_methods.as_deref())
    }
}

fn endpoint(name: &str, value: &str) -> Result<Url, Error> {
    let url = Url::parse(value)
        .map_err(|_| Error::Discovery(format!("the {name} {value} isn't a URL.")))?;
    if !endpoint_allowed(&url) {
        return Err(Error::Discovery(format!(
            "the {name} {value} must use https."
        )));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const ISSUER: &str = "https://acme.okta.com";

    fn document() -> Value {
        json!({
            "issuer": ISSUER,
            "authorization_endpoint": "https://acme.okta.com/oauth2/v1/authorize",
            "token_endpoint": "https://acme.okta.com/oauth2/v1/token",
            "jwks_uri": "https://acme.okta.com/oauth2/v1/keys",
            "response_types_supported": ["code"]
        })
    }

    fn parse(document: &Value) -> Result<Discovery, Error> {
        Discovery::parse(
            &serde_json::to_vec(document).unwrap(),
            &Issuer::parse(ISSUER).unwrap(),
        )
    }

    #[test]
    fn reads_the_endpoints() {
        let discovery = parse(&document()).unwrap();
        assert_eq!(discovery.issuer, ISSUER);
        assert_eq!(
            discovery.authorization_endpoint.as_str(),
            "https://acme.okta.com/oauth2/v1/authorize"
        );
        assert_eq!(
            discovery.token_endpoint.as_str(),
            "https://acme.okta.com/oauth2/v1/token"
        );
        assert_eq!(
            discovery.keys_endpoint.as_str(),
            "https://acme.okta.com/oauth2/v1/keys"
        );
        assert_eq!(discovery.userinfo_endpoint, None);
        assert_eq!(discovery.signing_algorithms, None);
        assert_eq!(discovery.authentication(), ClientAuthentication::Basic);
    }

    #[test]
    fn reads_the_optional_userinfo_endpoint() {
        let mut document = document();
        document["userinfo_endpoint"] = json!("https://acme.okta.com/oauth2/v1/userinfo");
        assert_eq!(
            parse(&document)
                .unwrap()
                .userinfo_endpoint
                .map(String::from),
            Some("https://acme.okta.com/oauth2/v1/userinfo".to_owned())
        );
    }

    #[test]
    fn the_userinfo_endpoint_follows_the_https_rule() {
        let mut document = document();
        document["userinfo_endpoint"] = json!("http://acme.okta.com/oauth2/v1/userinfo");
        let Err(Error::Discovery(message)) = parse(&document) else {
            panic!("a plain http userinfo endpoint was accepted");
        };
        assert!(message.contains("userinfo endpoint"), "{message}");
        document["userinfo_endpoint"] = json!("http://127.0.0.1:9000/userinfo");
        assert!(parse(&document).unwrap().userinfo_endpoint.is_some());
        document["userinfo_endpoint"] = json!("/userinfo");
        assert!(matches!(parse(&document), Err(Error::Discovery(_))));
    }

    #[test]
    fn refuses_an_issuer_mismatch_even_by_a_trailing_slash() {
        for issuer in ["https://evil.test", "https://acme.okta.com/"] {
            let mut document = document();
            document["issuer"] = json!(issuer);
            let Err(Error::Discovery(message)) = parse(&document) else {
                panic!("{issuer} was accepted");
            };
            assert!(message.contains(issuer), "{message}");
        }
    }

    #[test]
    fn refuses_plain_http_endpoints_off_loopback() {
        for field in ["authorization_endpoint", "token_endpoint", "jwks_uri"] {
            let mut document = document();
            document[field] = json!("http://acme.okta.com/oauth2/v1/endpoint");
            let Err(Error::Discovery(message)) = parse(&document) else {
                panic!("{field} over http was accepted");
            };
            assert!(message.contains("https"), "{message}");
        }
    }

    #[test]
    fn accepts_loopback_http_endpoints() {
        let mut document = document();
        document["token_endpoint"] = json!("http://127.0.0.1:9000/token");
        assert!(parse(&document).is_ok());
    }

    #[test]
    fn refuses_missing_fields_and_non_urls() {
        let mut missing = document();
        missing.as_object_mut().unwrap().remove("jwks_uri");
        assert!(matches!(parse(&missing), Err(Error::Discovery(_))));
        let mut relative = document();
        relative["token_endpoint"] = json!("/token");
        assert!(matches!(parse(&relative), Err(Error::Discovery(_))));
        assert!(matches!(
            Discovery::parse(b"<html>", &Issuer::parse(ISSUER).unwrap()),
            Err(Error::Discovery(_))
        ));
    }

    #[test]
    fn reads_advertised_algorithms_and_methods() {
        let mut document = document();
        document["id_token_signing_alg_values_supported"] = json!(["RS256"]);
        document["token_endpoint_auth_methods_supported"] = json!(["client_secret_post"]);
        let discovery = parse(&document).unwrap();
        assert_eq!(discovery.signing_algorithms, Some(vec!["RS256".to_owned()]));
        assert_eq!(discovery.authentication(), ClientAuthentication::Post);
    }
}
