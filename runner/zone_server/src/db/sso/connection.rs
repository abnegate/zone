use std::fmt;

use abnegate_secret::REDACTED;

#[derive(Clone, PartialEq, Eq)]
pub enum Connection {
    Oidc {
        client_id: String,
        sealed_secret: String,
    },
    Saml {
        sso_url: String,
        certificates: Vec<String>,
        metadata_url: Option<String>,
    },
}

impl fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oidc { client_id, .. } => formatter
                .debug_struct("Oidc")
                .field("client_id", client_id)
                .field("sealed_secret", &REDACTED)
                .finish(),
            Self::Saml {
                sso_url,
                certificates,
                metadata_url,
            } => formatter
                .debug_struct("Saml")
                .field("sso_url", sso_url)
                .field("certificates", &certificates.len())
                .field("metadata_url", metadata_url)
                .finish(),
        }
    }
}
