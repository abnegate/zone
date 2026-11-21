use crate::sso::saml::uri;

pub const UNSPECIFIED: &str = "urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameIdFormat {
    Unspecified,
    Persistent,
    EmailAddress,
    Transient,
    Other,
}

impl NameIdFormat {
    pub fn parse(format: Option<&str>) -> Self {
        match format {
            None | Some(UNSPECIFIED) => Self::Unspecified,
            Some(uri::PERSISTENT) => Self::Persistent,
            Some(uri::EMAIL_ADDRESS) => Self::EmailAddress,
            Some(uri::TRANSIENT) => Self::Transient,
            Some(_) => Self::Other,
        }
    }
}
