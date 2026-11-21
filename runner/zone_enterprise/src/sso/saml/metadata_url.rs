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
        endpoint(value).map(Self).ok_or_else(refused)
    }

    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

pub(super) fn endpoint(value: &str) -> Option<Url> {
    let url = Url::parse(value.trim()).ok()?;
    let plain = url.username().is_empty() && url.password().is_none() && url.fragment().is_none();
    (plain && endpoint_allowed(&url)).then_some(url)
}

impl fmt::Display for MetadataUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}
