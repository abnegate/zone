use abnegate_secret::{MasterKey, SecretValue};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::sso::{Fields, Protocol};

use super::connection::Connection;
use super::provider_error::{ProviderError, ProviderResult};
use crate::secret::Unseal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    pub id: Uuid,
    pub protocol: Protocol,
    pub display_name: String,
    pub issuer: String,
    pub connection: Connection,
    pub allowed_domains: Vec<String>,
    pub require_verified_email: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProviderRecord {
    pub fn client_id(&self) -> Option<&str> {
        match &self.connection {
            Connection::Oidc { client_id, .. } => Some(client_id),
            Connection::Saml { .. } => None,
        }
    }

    pub fn client_secret(&self, key: &MasterKey) -> ProviderResult<SecretValue> {
        let Connection::Oidc { sealed_secret, .. } = &self.connection else {
            return Err(ProviderError::Mismatch(self.id));
        };
        key.open_strict(sealed_secret)
            .map_err(|error| ProviderError::Unopenable(self.id, error))
    }

    pub fn fields<'a>(&'a self, client_secret: &'a SecretValue) -> ProviderResult<Fields<'a>> {
        let Connection::Oidc { client_id, .. } = &self.connection else {
            return Err(ProviderError::Mismatch(self.id));
        };
        Ok(Fields {
            protocol: self.protocol,
            display_name: &self.display_name,
            issuer: &self.issuer,
            client_id,
            client_secret: client_secret.expose(),
            allowed_domains: &self.allowed_domains,
            require_verified_email: self.require_verified_email,
        })
    }
}
