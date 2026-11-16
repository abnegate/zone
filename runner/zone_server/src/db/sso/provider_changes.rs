use abnegate_secret::SecretValue;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderChanges {
    pub display_name: Option<String>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<SecretValue>,
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
            && self.allowed_domains.is_none()
            && self.require_verified_email.is_none()
            && self.enabled.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
