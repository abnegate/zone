use std::fmt;

use reqwest::header::ACCEPT;
use reqwest::redirect;

use crate::sso::body;
use crate::sso::error::Error;
use crate::sso::oidc::TIMEOUT;
use crate::sso::saml::metadata::Metadata;
use crate::sso::saml::metadata_url::MetadataUrl;

const ACCEPTED: &str = "application/samlmetadata+xml, application/xml";

#[derive(Clone)]
pub struct Client {
    client: reqwest::Client,
}

impl Client {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .redirect(redirect::Policy::none())
            .build()
            .expect("an HTTP client with a timeout and no redirects always builds");
        Self { client }
    }

    pub async fn metadata(&self, url: &MetadataUrl) -> Result<Metadata, Error> {
        let response = self
            .client
            .get(url.as_url().clone())
            .header(ACCEPT, ACCEPTED)
            .send()
            .await
            .map_err(|error| {
                Error::MetadataInvalid(format!(
                    "{url} couldn't be fetched ({}).",
                    error.without_url()
                ))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::MetadataInvalid(format!(
                "{url} answered HTTP {}.",
                status.as_u16()
            )));
        }
        let body = body::read(response).await.map_err(|unread| {
            Error::MetadataInvalid(format!("{url} couldn't be read ({unread})."))
        })?;
        Metadata::parse(&body)
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Client").finish_non_exhaustive()
    }
}
