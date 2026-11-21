use std::fmt;

use abnegate_secret::REDACTED;
use reqwest::Url;
use zone_enterprise::sso::{Authorization, saml};

#[derive(Clone, PartialEq, Eq)]
pub struct Start {
    pub url: Url,
    pub state: String,
}

impl From<Authorization> for Start {
    fn from(authorization: Authorization) -> Self {
        Self {
            url: authorization.url,
            state: authorization.state,
        }
    }
}

impl From<saml::Authorization> for Start {
    fn from(authorization: saml::Authorization) -> Self {
        Self {
            url: authorization.url,
            state: authorization.state,
        }
    }
}

impl fmt::Debug for Start {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut endpoint = self.url.clone();
        endpoint.set_query(None);
        formatter
            .debug_struct("Start")
            .field("endpoint", &endpoint.as_str())
            .field("state", &REDACTED)
            .finish()
    }
}
