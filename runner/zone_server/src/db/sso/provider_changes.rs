use std::fmt;

use abnegate_secret::SecretValue;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProviderChanges {
    pub display_name: Option<String>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<SecretValue>,
    pub sso_url: Option<String>,
    pub certificates: Option<Vec<String>>,
    pub metadata_url: Option<Option<String>>,
    pub allowed_domains: Option<Vec<String>>,
    pub require_verified_email: Option<bool>,
    pub enabled: Option<bool>,
}

impl ProviderChanges {
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.issuer.is_none()
            && self.client_id.is_none()
            && self.client_secret.is_none()
            && self.sso_url.is_none()
            && self.certificates.is_none()
            && self.metadata_url.is_none()
            && self.allowed_domains.is_none()
            && self.require_verified_email.is_none()
            && self.enabled.is_none()
    }

    /// A subject is unique only within one issuer (for SAML, the entity ID), and some providers
    /// give each client its own, so a new issuer or client ID makes every stored subject
    /// meaningless.
    pub fn renames_subjects(&self) -> bool {
        self.issuer.is_some() || self.client_id.is_some()
    }

    /// A SAML identity is checked when the identity provider posts it, so an attempt asserted
    /// before the entity ID, SSO URL or certificates change carries an identity checked against
    /// settings the provider no longer has.
    pub fn voids_attempts(&self) -> bool {
        self.issuer.is_some() || self.sso_url.is_some() || self.certificates.is_some()
    }
}

impl fmt::Debug for ProviderChanges {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderChanges")
            .field("display_name", &self.display_name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret)
            .field("sso_url", &self.sso_url)
            .field("certificates", &self.certificates.as_ref().map(Vec::len))
            .field("metadata_url", &self.metadata_url)
            .field("allowed_domains", &self.allowed_domains)
            .field("require_verified_email", &self.require_verified_email)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_new_issuer_or_client_id_renames_subjects() {
        assert!(!ProviderChanges::default().renames_subjects());
        assert!(
            ProviderChanges {
                issuer: Some("https://other.example.com".into()),
                ..ProviderChanges::default()
            }
            .renames_subjects()
        );
        assert!(
            ProviderChanges {
                client_id: Some("0oa2".into()),
                ..ProviderChanges::default()
            }
            .renames_subjects()
        );
        assert!(
            !ProviderChanges {
                display_name: Some("Okta".into()),
                client_secret: Some(SecretValue::new("rotated")),
                sso_url: Some("https://acme.okta.com/app/sso/saml".into()),
                certificates: Some(vec!["MIIC".into()]),
                metadata_url: Some(None),
                allowed_domains: Some(vec!["acme.io".into()]),
                require_verified_email: Some(false),
                enabled: Some(false),
                ..ProviderChanges::default()
            }
            .renames_subjects()
        );
    }

    #[test]
    fn only_a_new_identity_provider_voids_attempts() {
        assert!(!ProviderChanges::default().voids_attempts());
        for changes in [
            ProviderChanges {
                issuer: Some("https://idp.example.com/other".into()),
                ..ProviderChanges::default()
            },
            ProviderChanges {
                sso_url: Some("https://idp.example.com/other/sso".into()),
                ..ProviderChanges::default()
            },
            ProviderChanges {
                certificates: Some(vec!["MIIC".into()]),
                ..ProviderChanges::default()
            },
        ] {
            assert!(changes.voids_attempts(), "{changes:?}");
        }
        assert!(
            !ProviderChanges {
                display_name: Some("Okta".into()),
                client_id: Some("0oa2".into()),
                client_secret: Some(SecretValue::new("rotated")),
                metadata_url: Some(None),
                allowed_domains: Some(vec!["acme.io".into()]),
                require_verified_email: Some(false),
                enabled: Some(false),
                ..ProviderChanges::default()
            }
            .voids_attempts()
        );
    }

    #[test]
    fn only_the_default_is_empty() {
        assert!(ProviderChanges::default().is_empty());
        let changes = [
            ProviderChanges {
                display_name: Some("Okta".into()),
                ..Default::default()
            },
            ProviderChanges {
                issuer: Some("https://acme.okta.com".into()),
                ..Default::default()
            },
            ProviderChanges {
                client_id: Some("0oa1".into()),
                ..Default::default()
            },
            ProviderChanges {
                client_secret: Some(SecretValue::new("s3cret")),
                ..Default::default()
            },
            ProviderChanges {
                sso_url: Some("https://acme.okta.com/app/sso/saml".into()),
                ..Default::default()
            },
            ProviderChanges {
                certificates: Some(Vec::new()),
                ..Default::default()
            },
            ProviderChanges {
                metadata_url: Some(None),
                ..Default::default()
            },
            ProviderChanges {
                allowed_domains: Some(Vec::new()),
                ..Default::default()
            },
            ProviderChanges {
                require_verified_email: Some(false),
                ..Default::default()
            },
            ProviderChanges {
                enabled: Some(true),
                ..Default::default()
            },
        ];
        for change in changes {
            assert!(!change.is_empty(), "{change:?}");
        }
    }
}
