use abnegate_secret::SecretValue;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::sso::{Fields, Protocol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    pub id: Uuid,
    pub protocol: Protocol,
    pub display_name: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: SecretValue,
    pub allowed_domains: Vec<String>,
    pub require_verified_email: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProviderRecord {
    pub fn fields(&self) -> Fields<'_> {
        Fields {
            protocol: self.protocol,
            display_name: &self.display_name,
            issuer: &self.issuer,
            client_id: &self.client_id,
            client_secret: self.client_secret.expose(),
            allowed_domains: &self.allowed_domains,
            require_verified_email: self.require_verified_email,
        }
    }
}
