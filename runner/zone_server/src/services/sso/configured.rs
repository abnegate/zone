use abnegate_secret::MasterKey;
use zone_enterprise::sso::{Policy, Provider, saml};

use super::sign_in_failure::SignInFailure;
use crate::db::sso::ProviderRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Configured {
    Oidc(Provider),
    Saml(saml::Provider),
}

impl Configured {
    pub fn parse(record: &ProviderRecord, key: &MasterKey) -> Result<Self, SignInFailure> {
        match record.saml_fields() {
            Some(fields) => saml::Provider::parse(&fields)
                .map(Self::Saml)
                .map_err(|error| SignInFailure::Unreadable(record.id, error)),
            None => {
                let client_secret = record.client_secret(key)?;
                Ok(Self::Oidc(Provider::parse(
                    &record.fields(&client_secret)?,
                )?))
            }
        }
    }

    pub fn policy(&self) -> &Policy {
        match self {
            Self::Oidc(provider) => &provider.policy,
            Self::Saml(provider) => &provider.policy,
        }
    }

    pub fn vouches_for_email(&self) -> bool {
        matches!(self, Self::Oidc(_))
    }
}
