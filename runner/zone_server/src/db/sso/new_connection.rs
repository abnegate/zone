use std::fmt;

use abnegate_secret::SecretValue;

#[derive(Clone, PartialEq, Eq)]
pub enum NewConnection {
    Oidc {
        client_id: String,
        client_secret: SecretValue,
    },
    Saml {
        sso_url: String,
        certificates: Vec<String>,
        metadata_url: Option<String>,
    },
}

impl fmt::Debug for NewConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oidc {
                client_id,
                client_secret,
            } => formatter
                .debug_struct("Oidc")
                .field("client_id", client_id)
                .field("client_secret", client_secret)
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
