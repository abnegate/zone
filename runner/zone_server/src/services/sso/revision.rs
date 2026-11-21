use std::fmt;

use abnegate_secret::{MasterKey, REDACTED, SecretValue};
use zone_enterprise::sso::{Fields, Protocol, saml};

use crate::db::sso::{Connection, ProviderError, ProviderRecord, ProviderResult};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Revision<'a> {
    pub display_name: Option<&'a str>,
    pub issuer: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub client_secret: Option<&'a str>,
    pub metadata_url: Option<&'a str>,
    pub metadata: Option<&'a str>,
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

    /// The stored SAML connection with this revision's shared settings. New metadata, when the
    /// revision brings some, replaces the entity ID, SSO URL and certificates read from here.
    pub fn saml_fields<'b>(&self, record: &'b ProviderRecord) -> ProviderResult<saml::Fields<'b>>
    where
        'a: 'b,
    {
        let Connection::Saml {
            sso_url,
            certificates,
            ..
        } = &record.connection
        else {
            return Err(ProviderError::Protocol(
                record.id,
                record.protocol.to_string(),
            ));
        };
        Ok(saml::Fields {
            display_name: self.display_name.unwrap_or(&record.display_name),
            entity_id: &record.issuer,
            sso_url,
            certificates,
            allowed_domains: self.allowed_domains.unwrap_or(&record.allowed_domains),
            require_verified_email: self
                .require_verified_email
                .unwrap_or(record.require_verified_email),
        })
    }

    /// Whether the revision sets a field the provider's protocol doesn't have.
    pub fn mismatches(&self, protocol: Protocol) -> bool {
        match protocol {
            Protocol::Oidc => self.metadata_url.is_some() || self.metadata.is_some(),
            Protocol::Saml => {
                self.issuer.is_some() || self.client_id.is_some() || self.client_secret.is_some()
            }
        }
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

    /// A metadata refresh keeps certificate rotation possible without a license, so it must carry
    /// nothing else.
    pub fn only_refreshes_metadata(&self) -> bool {
        (self.metadata_url.is_some() || self.metadata.is_some())
            && *self
                == Self {
                    metadata_url: self.metadata_url,
                    metadata: self.metadata,
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
            .field("metadata_url", &self.metadata_url)
            .field("metadata", &self.metadata.map(str::len))
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
            ..Revision::default()
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

    fn saml_record() -> ProviderRecord {
        ProviderRecord {
            id: Uuid::new_v4(),
            protocol: Protocol::Saml,
            display_name: "Okta".into(),
            issuer: "http://www.okta.com/exk1".into(),
            connection: Connection::Saml {
                sso_url: "https://acme.okta.com/app/sso/saml".into(),
                certificates: vec!["MIIC".into()],
                metadata_url: Some("https://acme.okta.com/app/exk1/sso/saml/metadata".into()),
            },
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: false,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn saml_fields_keep_the_stored_connection_and_take_the_revised_settings() {
        let record = saml_record();
        let domains = vec!["acme.io".to_owned()];
        assert_eq!(
            Revision::default().saml_fields(&record).unwrap(),
            saml::Fields {
                display_name: "Okta",
                entity_id: "http://www.okta.com/exk1",
                sso_url: "https://acme.okta.com/app/sso/saml",
                certificates: &["MIIC".to_owned()],
                allowed_domains: &["acme.com".to_owned()],
                require_verified_email: false,
            }
        );
        let revision = Revision {
            display_name: Some("Okta SAML"),
            allowed_domains: Some(&domains),
            require_verified_email: Some(true),
            ..Revision::default()
        };
        let fields = revision.saml_fields(&record).unwrap();
        assert_eq!(fields.display_name, "Okta SAML");
        assert_eq!(fields.entity_id, "http://www.okta.com/exk1");
        assert_eq!(fields.allowed_domains, ["acme.io"]);
        assert!(fields.require_verified_email);
    }

    #[test]
    fn an_oidc_record_has_no_saml_fields() {
        let record = record(&key());
        assert!(matches!(
            Revision::default().saml_fields(&record),
            Err(ProviderError::Protocol(id, protocol)) if id == record.id && protocol == "oidc"
        ));
    }

    #[test]
    fn a_field_of_the_other_protocol_mismatches() {
        let cases = [
            (
                Revision {
                    metadata_url: Some("https://idp.example.com/metadata"),
                    ..Revision::default()
                },
                Protocol::Oidc,
            ),
            (
                Revision {
                    metadata: Some("<md:EntityDescriptor/>"),
                    ..Revision::default()
                },
                Protocol::Oidc,
            ),
            (
                Revision {
                    issuer: Some("https://acme.okta.com"),
                    ..Revision::default()
                },
                Protocol::Saml,
            ),
            (
                Revision {
                    client_id: Some("0oa1"),
                    ..Revision::default()
                },
                Protocol::Saml,
            ),
            (
                Revision {
                    client_secret: Some(SECRET),
                    ..Revision::default()
                },
                Protocol::Saml,
            ),
        ];
        for (revision, protocol) in cases {
            assert!(revision.mismatches(protocol), "{revision:?} on {protocol}");
        }
        let shared = Revision {
            display_name: Some("Okta"),
            allowed_domains: Some(&[][..]),
            require_verified_email: Some(false),
            enabled: Some(true),
            ..Revision::default()
        };
        for protocol in Protocol::ALL {
            assert!(!shared.mismatches(protocol), "{protocol}");
        }
    }

    #[test]
    fn only_a_bare_metadata_source_only_refreshes_metadata() {
        let by_url = Revision {
            metadata_url: Some("https://idp.example.com/metadata"),
            ..Revision::default()
        };
        let by_document = Revision {
            metadata: Some("<md:EntityDescriptor/>"),
            ..Revision::default()
        };
        assert!(by_url.only_refreshes_metadata());
        assert!(by_document.only_refreshes_metadata());
        for revision in [
            Revision::default(),
            Revision {
                enabled: Some(false),
                ..Revision::default()
            },
            Revision {
                display_name: Some("Okta"),
                ..by_url
            },
            Revision {
                allowed_domains: Some(&[][..]),
                ..by_document
            },
            Revision {
                require_verified_email: Some(false),
                ..by_url
            },
            Revision {
                enabled: Some(true),
                ..by_document
            },
        ] {
            assert!(!revision.only_refreshes_metadata(), "{revision:?}");
        }
        assert!(!by_url.only_disables());
    }

    #[test]
    fn debug_shows_the_size_of_pasted_metadata_but_not_the_document() {
        let revision = Revision {
            metadata: Some("<md:EntityDescriptor/>"),
            ..Revision::default()
        };
        let shown = format!("{revision:?}");
        assert!(!shown.contains("EntityDescriptor"), "{shown}");
        assert!(shown.contains("metadata: Some(22)"), "{shown}");
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
