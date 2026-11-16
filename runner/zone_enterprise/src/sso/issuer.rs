use std::fmt;
use std::net::Ipv6Addr;

use url::{Host, Url};

use crate::sso::error::Error;

pub const GOOGLE: &str = "https://accounts.google.com";

const DISCOVERY_PATH: &str = "/.well-known/openid-configuration";
const SECURE_SCHEME: &str = "https";
const PLAIN_SCHEME: &str = "http";
const LOOPBACK_NAME: &str = "localhost";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Issuer(String);

impl Issuer {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let trimmed = value.trim();
        let url = Url::parse(trimmed).map_err(|_| Error::IssuerInvalid)?;
        let plain = url.query().is_none()
            && url.fragment().is_none()
            && url.username().is_empty()
            && url.password().is_none();
        if !plain || !endpoint_allowed(&url) {
            return Err(Error::IssuerInvalid);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_google(&self) -> bool {
        self.0 == GOOGLE
    }

    pub fn discovery_url(&self) -> Url {
        let base = self.0.trim_end_matches('/');
        Url::parse(&format!("{base}{DISCOVERY_PATH}"))
            .expect("a parsed issuer with a path appended is still a URL")
    }
}

impl fmt::Display for Issuer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub fn endpoint_allowed(url: &Url) -> bool {
    match url.scheme() {
        SECURE_SCHEME => url.host().is_some(),
        PLAIN_SCHEME => url.host().is_some_and(loopback),
        _ => false,
    }
}

fn loopback(host: Host<&str>) -> bool {
    match host {
        Host::Domain(name) => name.eq_ignore_ascii_case(LOOPBACK_NAME),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address == Ipv6Addr::LOCALHOST,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_issuers_and_keeps_them_exactly() {
        for value in [
            "https://acme.okta.com",
            "https://acme.okta.com/oauth2/default",
            "https://login.microsoftonline.com/0f1e/v2.0",
            "https://accounts.google.com",
            "https://idp.acme.test/",
            "https://10.0.0.5:8443/realms/zone",
        ] {
            assert_eq!(Issuer::parse(value).unwrap().as_str(), value, "{value}");
        }
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            Issuer::parse("  https://acme.okta.com ")
                .unwrap()
                .to_string(),
            "https://acme.okta.com"
        );
    }

    #[test]
    fn accepts_plain_http_only_on_loopback() {
        for value in [
            "http://localhost",
            "http://localhost:8080/realms/zone",
            "http://127.0.0.1:9000",
            "http://127.0.0.2",
            "http://[::1]:9000",
        ] {
            assert!(Issuer::parse(value).is_ok(), "{value}");
        }
    }

    #[test]
    fn refuses_plain_http_elsewhere() {
        for value in [
            "http://intranet.acme.test",
            "http://10.0.0.5",
            "http://192.168.1.10:8080",
            "http://[::2]",
            "http://localhost.acme.test",
        ] {
            assert_eq!(Issuer::parse(value), Err(Error::IssuerInvalid), "{value}");
        }
    }

    #[test]
    fn refuses_relative_odd_scheme_query_fragment_and_credentials() {
        for value in [
            "",
            "acme.okta.com",
            "/oauth2/default",
            "ftp://acme.okta.com",
            "file:///etc/passwd",
            "https://acme.okta.com?tenant=1",
            "https://acme.okta.com#top",
            "https://user:pass@acme.okta.com",
            "https://user@acme.okta.com",
        ] {
            assert_eq!(Issuer::parse(value), Err(Error::IssuerInvalid), "{value}");
        }
    }

    #[test]
    fn discovery_url_appends_the_well_known_path_once() {
        for (issuer, expected) in [
            (
                "https://acme.okta.com",
                "https://acme.okta.com/.well-known/openid-configuration",
            ),
            (
                "https://acme.okta.com/",
                "https://acme.okta.com/.well-known/openid-configuration",
            ),
            (
                "https://login.microsoftonline.com/0f1e/v2.0",
                "https://login.microsoftonline.com/0f1e/v2.0/.well-known/openid-configuration",
            ),
        ] {
            assert_eq!(
                Issuer::parse(issuer).unwrap().discovery_url().as_str(),
                expected
            );
        }
    }

    #[test]
    fn recognises_google() {
        assert!(Issuer::parse(GOOGLE).unwrap().is_google());
        assert!(!Issuer::parse("https://acme.okta.com").unwrap().is_google());
    }

    #[test]
    fn endpoints_follow_the_same_rule() {
        let allowed = |value: &str| endpoint_allowed(&Url::parse(value).unwrap());
        assert!(allowed("https://idp.acme.test/token"));
        assert!(allowed("http://127.0.0.1:9000/token"));
        assert!(!allowed("http://idp.acme.test/token"));
        assert!(!allowed("javascript:alert(1)"));
    }
}
