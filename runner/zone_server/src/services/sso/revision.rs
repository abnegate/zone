use zone_enterprise::sso::Fields;

use crate::db::sso::ProviderRecord;

/// A sparse change to a provider: a field left out keeps its stored value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Revision<'a> {
    pub display_name: Option<&'a str>,
    pub issuer: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub client_secret: Option<&'a str>,
    pub allowed_domains: Option<&'a [String]>,
    pub require_verified_email: Option<bool>,
    pub enabled: Option<bool>,
}

impl<'a> Revision<'a> {
    pub fn fields<'b>(&self, record: &'b ProviderRecord) -> Fields<'b>
    where
        'a: 'b,
    {
        Fields {
            protocol: record.protocol,
            display_name: self.display_name.unwrap_or(&record.display_name),
            issuer: self.issuer.unwrap_or(&record.issuer),
            client_id: self.client_id.unwrap_or(&record.client_id),
            client_secret: self.client_secret.unwrap_or(record.client_secret.expose()),
            allowed_domains: self.allowed_domains.unwrap_or(&record.allowed_domains),
            require_verified_email: self
                .require_verified_email
                .unwrap_or(record.require_verified_email),
        }
    }

    pub fn enabled(&self, record: &ProviderRecord) -> bool {
        self.enabled.unwrap_or(record.enabled)
    }
}

#[cfg(test)]
mod tests {
    use abnegate_secret::SecretValue;
    use chrono::Utc;
    use uuid::Uuid;
    use zone_enterprise::sso::Protocol;

    use super::*;

    fn record() -> ProviderRecord {
        ProviderRecord {
            id: Uuid::new_v4(),
            protocol: Protocol::Oidc,
            display_name: "Okta".into(),
            issuer: "https://acme.okta.com".into(),
            client_id: "0oa1".into(),
            client_secret: SecretValue::new("s3cret"),
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: true,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn an_empty_revision_keeps_every_stored_value() {
        let record = record();
        let revision = Revision::default();
        assert_eq!(revision.fields(&record), record.fields());
        assert!(revision.enabled(&record));
    }

    #[test]
    fn each_present_field_replaces_its_stored_value() {
        let record = record();
        let domains = vec!["acme.io".to_owned()];
        let revision = Revision {
            display_name: Some("Entra"),
            issuer: Some("https://login.microsoftonline.com/t/v2.0"),
            client_id: Some("app"),
            client_secret: Some("rotated"),
            allowed_domains: Some(&domains),
            require_verified_email: Some(false),
            enabled: Some(false),
        };
        let fields = revision.fields(&record);
        assert_eq!(fields.display_name, "Entra");
        assert_eq!(fields.issuer, "https://login.microsoftonline.com/t/v2.0");
        assert_eq!(fields.client_id, "app");
        assert_eq!(fields.client_secret, "rotated");
        assert_eq!(fields.allowed_domains, ["acme.io"]);
        assert!(!fields.require_verified_email);
        assert!(!revision.enabled(&record));
    }
}
