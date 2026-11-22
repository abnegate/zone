use abnegate_secret::SecretValue;
use zone_enterprise::sso::{Protocol, Provider, saml};

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

    /// The entity ID is stored as the issuer, which keeps one uniqueness rule and one subject
    /// namespace for both protocols.
    pub fn saml(provider: &saml::Provider, metadata_url: Option<String>, enabled: bool) -> Self {
        Self {
            protocol: Protocol::Saml,
            display_name: provider.display_name.as_str().to_owned(),
            issuer: provider.entity_id.as_str().to_owned(),
            connection: NewConnection::Saml {
                sso_url: provider.sso_url.as_str().to_owned(),
                certificates: provider
                    .certificates
                    .iter()
                    .map(saml::Certificate::encoded)
                    .collect(),
                metadata_url,
            },
            allowed_domains: provider.policy.domains.as_slice().to_vec(),
            require_verified_email: provider.policy.require_verified_email,
            enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use zone_enterprise::sso::saml::testing::{CERTIFICATE, ENTITY_ID, SSO_URL, metadata};

    use super::*;

    #[test]
    fn a_saml_provider_stores_its_entity_id_as_the_issuer_and_its_certificates_encoded() {
        let document = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]);
        let parsed = saml::Metadata::parse(document.as_bytes()).unwrap();
        let provider =
            saml::Provider::from_metadata(" Okta ", &parsed, &["ACME.com".to_owned()], false)
                .unwrap();
        let metadata_url = Some("https://idp.example.com/metadata".to_owned());

        let new = NewProvider::saml(&provider, metadata_url.clone(), true);

        assert_eq!(
            new,
            NewProvider {
                protocol: Protocol::Saml,
                display_name: "Okta".into(),
                issuer: ENTITY_ID.into(),
                connection: NewConnection::Saml {
                    sso_url: SSO_URL.into(),
                    certificates: vec![CERTIFICATE.into()],
                    metadata_url,
                },
                allowed_domains: vec!["acme.com".into()],
                require_verified_email: false,
                enabled: true,
            }
        );
    }
}
