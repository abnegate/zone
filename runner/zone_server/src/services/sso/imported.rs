use zone_enterprise::sso::Error;
use zone_enterprise::sso::saml::{Client, Metadata, MetadataUrl};

pub const MISSING: &str = "Provide a metadata URL or metadata XML.";
pub const AMBIGUOUS: &str = "Provide a metadata URL or metadata XML, not both.";

/// Identity provider metadata read from the one source a request named. `metadata_url` is the
/// address to keep on the provider: a pasted document leaves none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub metadata: Metadata,
    pub metadata_url: Option<String>,
}

impl Imported {
    pub async fn read(
        client: &Client,
        metadata_url: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<Option<Self>, Error> {
        match (metadata_url, metadata) {
            (None, None) => Ok(None),
            (Some(_), Some(_)) => Err(Error::MetadataInvalid(AMBIGUOUS.to_owned())),
            (Some(address), None) => {
                let address = MetadataUrl::parse(address)?;
                Ok(Some(Self {
                    metadata: client.metadata(&address).await?,
                    metadata_url: Some(address.to_string()),
                }))
            }
            (None, Some(document)) => Ok(Some(Self {
                metadata: Metadata::parse(document.as_bytes())?,
                metadata_url: None,
            })),
        }
    }

    pub fn missing() -> Error {
        Error::MetadataInvalid(MISSING.to_owned())
    }
}
