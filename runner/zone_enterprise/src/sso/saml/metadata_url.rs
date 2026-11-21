use std::fmt;

use url::Url;

use crate::sso::error::Error;
use crate::sso::issuer::endpoint_allowed;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetadataUrl(Url);

impl MetadataUrl {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let refused = || {
            Error::MetadataInvalid(
                "The metadata URL must be an absolute https URL with no credentials or fragment; plain http is only allowed for localhost."
                    .to_owned(),
            )
        };
        let url = Url::parse(value.trim()).map_err(|_| refused())?;
        let plain =
            url.username().is_empty() && url.password().is_none() && url.fragment().is_none();
        if !plain || !endpoint_allowed(&url) {
            return Err(refused());
        }
        Ok(Self(url))
    }

    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Display for MetadataUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}
