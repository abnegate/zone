use std::fmt;

use abnegate_secret::{MasterKey, REDACTED, SecretValue};
use zone_enterprise::sso::Fields;

use crate::db::sso::{Connection, ProviderError, ProviderRecord, ProviderResult};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
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
    pub fn client_secret(
        &self,
        record: &ProviderRecord,
        key: &MasterKey,
    ) -> ProviderResult<SecretValue> {
        match self.client_secret {
            Some(revised) => Ok(SecretValue::new(revised)),
            None => record.client_secret(key),
        }
    }

    /// A stored secret that no longer opens counts as replaced by any secret the revision carries.
    pub fn replaces_secret(&self, record: &ProviderRecord, key: &MasterKey) -> bool {
        self.client_secret.is_some_and(|revised| {
            record
                .client_secret(key)
                .map_or(true, |stored| stored.expose() != revised)
        })
    }

    pub fn fields<'b>(
        &self,
        record: &'b ProviderRecord,
        client_secret: &'b SecretValue,
    ) -> ProviderResult<Fields<'b>>
    where
        'a: 'b,
    {
        let Connection::Oidc { client_id, .. } = &record.connection else {
            return Err(ProviderError::Mismatch(record.id));
        };
        Ok(Fields {
            protocol: record.protocol,
            display_name: self.display_name.unwrap_or(&record.display_name),
            issuer: self.issuer.unwrap_or(&record.issuer),
            client_id: self.client_id.unwrap_or(client_id),
            client_secret: client_secret.expose(),
            allowed_domains: self.allowed_domains.unwrap_or(&record.allowed_domains),
            require_verified_email: self
                .require_verified_email
                .unwrap_or(record.require_verified_email),
        })
    }

    pub fn enabled(&self, record: &ProviderRecord) -> bool {
        self.enabled.unwrap_or(record.enabled)
    }

    pub fn only_disables(&self) -> bool {
        *self
            == Self {
                enabled: Some(false),
                ..Self::default()
            }
    }
}

impl fmt::Debug for Revision<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Revision")
            .field("display_name", &self.display_name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.map(|_| REDACTED))
            .field("allowed_domains", &self.allowed_domains)
            .field("require_verified_email", &self.require_verified_email)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use abnegate_secret::encrypt_value;
    use chrono::Utc;
    use uuid::Uuid;
    use zone_enterprise::sso::Protocol;

    use super::*;

    const SECRET: &str = "s3cret";

    fn key() -> MasterKey {
        MasterKey::generate().expect("a key")
    }

    fn record(key: &MasterKey) -> ProviderRecord {
        ProviderRecord {
            id: Uuid::new_v4(),
            protocol: Protocol::Oidc,
            display_name: "Okta".into(),
            issuer: "https://acme.okta.com".into(),
            connection: Connection::Oidc {
                client_id: "0oa1".into(),
                sealed_secret: encrypt_value(&SecretValue::new(SECRET), key)
                    .expect("a sealed secret"),
            },
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: true,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn an_empty_revision_keeps_every_stored_value() {
        let key = key();
        let record = record(&key);
        let revision = Revision::default();
        let secret = revision.client_secret(&record, &key).unwrap();
        assert_eq!(secret.expose(), SECRET);
        assert_eq!(
            revision.fields(&record, &secret).unwrap(),
            record.fields(&secret).unwrap()
        );
        assert!(revision.enabled(&record));
        assert!(!revision.replaces_secret(&record, &key));
    }

    #[test]
    fn each_present_field_replaces_its_stored_value() {
        let key = key();
        let record = record(&key);
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
        let secret = revision.client_secret(&record, &key).unwrap();
        let fields = revision.fields(&record, &secret).unwrap();
        assert_eq!(fields.display_name, "Entra");
        assert_eq!(fields.issuer, "https://login.microsoftonline.com/t/v2.0");
        assert_eq!(fields.client_id, "app");
        assert_eq!(fields.client_secret, "rotated");
        assert_eq!(fields.allowed_domains, ["acme.io"]);
        assert!(!fields.require_verified_email);
        assert!(!revision.enabled(&record));
        assert!(revision.replaces_secret(&record, &key));
    }

    #[test]
    fn a_revision_that_brings_a_secret_never_opens_the_stored_one() {
        let record = record(&key());
        let other = key();
        let revision = Revision {
            client_secret: Some("rotated"),
            ..Revision::default()
        };
        assert!(Revision::default().client_secret(&record, &other).is_err());
        assert_eq!(
            revision.client_secret(&record, &other).unwrap().expose(),
            "rotated"
        );
        assert!(revision.replaces_secret(&record, &other));
    }

    #[test]
    fn the_same_secret_again_replaces_nothing() {
        let key = key();
        let record = record(&key);
        let revision = Revision {
            client_secret: Some(SECRET),
            ..Revision::default()
        };
        assert!(!revision.replaces_secret(&record, &key));
    }

    #[test]
    fn only_a_bare_disable_only_disables() {
        let disable = Revision {
            enabled: Some(false),
            ..Revision::default()
        };
        assert!(disable.only_disables());
        for revision in [
            Revision::default(),
            Revision {
                enabled: Some(true),
                ..Revision::default()
            },
            Revision {
                display_name: Some("Okta"),
                ..disable
            },
            Revision {
                require_verified_email: Some(false),
                ..disable
            },
            Revision {
                allowed_domains: Some(&[][..]),
                ..disable
            },
        ] {
            assert!(!revision.only_disables(), "{revision:?}");
        }
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let revision = Revision {
            client_secret: Some(SECRET),
            ..Revision::default()
        };
        let shown = format!("{revision:?}");
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
