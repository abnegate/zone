use abnegate_secret::{MasterKey, SecretValue};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::sso::{Fields, Protocol};

use super::provider_error::{ProviderError, ProviderResult};
use crate::secret::Unseal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    pub id: Uuid,
    pub protocol: Protocol,
    pub display_name: String,
    pub issuer: String,
    pub client_id: String,
    pub sealed_secret: String,
    pub allowed_domains: Vec<String>,
    pub require_verified_email: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProviderRecord {
    pub fn client_secret(&self, key: &MasterKey) -> ProviderResult<SecretValue> {
        key.open_strict(&self.sealed_secret)
            .map_err(|error| ProviderError::Unopenable(self.id, error))
    }

    pub fn fields<'a>(&'a self, client_secret: &'a SecretValue) -> Fields<'a> {
        Fields {
            protocol: self.protocol,
            display_name: &self.display_name,
            issuer: &self.issuer,
            client_id: &self.client_id,
            client_secret: client_secret.expose(),
            allowed_domains: &self.allowed_domains,
            require_verified_email: self.require_verified_email,
        }
    }
}
