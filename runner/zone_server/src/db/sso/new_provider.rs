use abnegate_secret::SecretValue;
use zone_enterprise::sso::{Protocol, Provider};

use super::new_connection::NewConnection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProvider {
    pub protocol: Protocol,
    pub display_name: String,
    pub issuer: String,
    pub connection: NewConnection,
    pub allowed_domains: Vec<String>,
    pub require_verified_email: bool,
    pub enabled: bool,
}

impl NewProvider {
    pub fn new(provider: &Provider, enabled: bool) -> Self {
        Self {
            protocol: provider.protocol,
            display_name: provider.display_name.as_str().to_owned(),
            issuer: provider.issuer.as_str().to_owned(),
            connection: NewConnection::Oidc {
                client_id: provider.client_id.as_str().to_owned(),
                client_secret: SecretValue::new(provider.client_secret.expose()),
            },
            allowed_domains: provider.policy.domains.as_slice().to_vec(),
            require_verified_email: provider.policy.require_verified_email,
            enabled,
        }
    }
}
