mod s3;
mod syslog;
mod webhook;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::audit::error::Error;
use crate::audit::kind::Kind;
use crate::audit::url_reason::UrlReason;

pub use s3::{Addressing, S3Secrets, S3Settings};
pub use syslog::{Facility, Protocol, SyslogSecrets, SyslogSettings};
pub use webhook::{WebhookSecrets, WebhookSettings};

const REDACTED: &str = "<redacted>";
const SCHEMES: [&str; 2] = ["http", "https"];

pub fn name(value: &str) -> Result<String, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(Error::NameInvalid);
    }
    Ok(trimmed.to_owned())
}

fn url(raw: &str) -> Result<String, Error> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed).map_err(|_| Error::UrlInvalid(UrlReason::Unparseable))?;
    let reason = if !SCHEMES.contains(&url.scheme()) {
        Some(UrlReason::Scheme)
    } else if url.host_str().is_none_or(str::is_empty) {
        Some(UrlReason::Host)
    } else if !url.username().is_empty() || url.password().is_some() {
        Some(UrlReason::Userinfo)
    } else if url.query().is_some() {
        Some(UrlReason::Query)
    } else if url.fragment().is_some() {
        Some(UrlReason::Fragment)
    } else {
        None
    };
    match reason {
        Some(reason) => Err(Error::UrlInvalid(reason)),
        None => Ok(trimmed.to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Settings {
    Webhook(WebhookSettings),
    S3(S3Settings),
    Syslog(SyslogSettings),
}

impl Settings {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Webhook(_) => Kind::Webhook,
            Self::S3(_) => Kind::S3,
            Self::Syslog(_) => Kind::Syslog,
        }
    }

    pub fn validated(self) -> Result<Self, Error> {
        Ok(match self {
            Self::Webhook(settings) => Self::Webhook(settings.validated()?),
            Self::S3(settings) => Self::S3(settings.validated()?),
            Self::Syslog(settings) => Self::Syslog(settings.validated()?),
        })
    }

    pub fn admits(&self, secrets: &Secrets) -> Result<(), Error> {
        match (self, secrets) {
            (Self::Syslog(settings), Secrets::Syslog(_))
                if settings.protocol == Protocol::Udp && !secrets.names().is_empty() =>
            {
                Err(Error::CertificateWithoutTls)
            }
            (Self::Webhook(_), Secrets::Webhook(_))
            | (Self::S3(_), Secrets::S3(_))
            | (Self::Syslog(_), Secrets::Syslog(_)) => Ok(()),
            _ => Err(Error::KindInvalid),
        }
    }

    pub fn from_stored(kind: Kind, value: Value) -> serde_json::Result<Self> {
        let settings = match kind {
            Kind::Webhook => Self::Webhook(serde_json::from_value(value)?),
            Kind::S3 => Self::S3(serde_json::from_value(value)?),
            Kind::Syslog => Self::Syslog(serde_json::from_value(value)?),
        };
        settings.validated().map_err(serde::de::Error::custom)
    }

    pub fn stored(&self) -> Value {
        match self {
            Self::Webhook(settings) => serde_json::to_value(settings),
            Self::S3(settings) => serde_json::to_value(settings),
            Self::Syslog(settings) => serde_json::to_value(settings),
        }
        .expect("settings hold only strings, numbers and enums")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretName {
    SigningSecret,
    AccessKeyId,
    SecretAccessKey,
    CaCertificate,
    ClientCertificate,
    ClientKey,
}

impl SecretName {
    pub const ALL: [Self; 6] = [
        Self::SigningSecret,
        Self::AccessKeyId,
        Self::SecretAccessKey,
        Self::CaCertificate,
        Self::ClientCertificate,
        Self::ClientKey,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SigningSecret => "signing_secret",
            Self::AccessKeyId => "access_key_id",
            Self::SecretAccessKey => "secret_access_key",
            Self::CaCertificate => "ca_certificate",
            Self::ClientCertificate => "client_certificate",
            Self::ClientKey => "client_key",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SigningSecret => "signing secret",
            Self::AccessKeyId => "access key ID",
            Self::SecretAccessKey => "secret access key",
            Self::CaCertificate => "CA certificate",
            Self::ClientCertificate => "client certificate",
            Self::ClientKey => "client key",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::SigningSecret => Kind::Webhook,
            Self::AccessKeyId | Self::SecretAccessKey => Kind::S3,
            Self::CaCertificate | Self::ClientCertificate | Self::ClientKey => Kind::Syslog,
        }
    }

    pub fn required(self) -> bool {
        self.kind() != Kind::Syslog
    }

    pub fn of(kind: Kind) -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(move |name| name.kind() == kind)
    }

    fn parse(kind: Kind, value: &str) -> Option<Self> {
        Self::of(kind).find(|name| name.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Secrets {
    Webhook(WebhookSecrets),
    S3(S3Secrets),
    Syslog(SyslogSecrets),
}

impl Secrets {
    pub fn parse(kind: Kind, value: Value) -> Result<Self, Error> {
        let object = match value {
            Value::Null => Map::new(),
            Value::Object(object) => object,
            _ => return Err(Error::SecretsMalformed),
        };
        let mut entries = Map::new();
        for (key, value) in object {
            let name = SecretName::parse(kind, &key).ok_or(Error::SecretUnknown(key))?;
            let Value::String(text) = value else {
                return Err(Error::SecretInvalid(name));
            };
            if !text.trim().is_empty() {
                entries.insert(name.as_str().to_owned(), Value::String(text));
            }
        }
        if let Some(missing) = SecretName::of(kind)
            .find(|name| name.required() && !entries.contains_key(name.as_str()))
        {
            return Err(Error::SecretInvalid(missing));
        }
        Self::from_entries(kind, entries)
            .map_err(|_| Error::SecretsMalformed)?
            .validated()
    }

    pub fn merged(&self, patch: Value) -> Result<Self, Error> {
        let patch = match patch {
            Value::Null => return Ok(self.clone()),
            Value::Object(patch) => patch,
            _ => return Err(Error::SecretsMalformed),
        };
        let kind = self.kind();
        let mut entries = self.entries();
        for (key, value) in patch {
            let name = SecretName::parse(kind, &key).ok_or(Error::SecretUnknown(key))?;
            match value {
                Value::String(text) if text.trim().is_empty() => {
                    entries.remove(name.as_str());
                }
                Value::String(text) => {
                    entries.insert(name.as_str().to_owned(), Value::String(text));
                }
                _ => return Err(Error::SecretInvalid(name)),
            }
        }
        Self::parse(kind, Value::Object(entries))
    }

    pub fn kind(&self) -> Kind {
        match self {
            Self::Webhook(_) => Kind::Webhook,
            Self::S3(_) => Kind::S3,
            Self::Syslog(_) => Kind::Syslog,
        }
    }

    pub fn names(&self) -> Vec<SecretName> {
        let entries = self.entries();
        SecretName::of(self.kind())
            .filter(|name| entries.contains_key(name.as_str()))
            .collect()
    }

    pub fn sealed_form(&self) -> String {
        Value::Object(self.entries()).to_string()
    }

    pub fn from_sealed_form(kind: Kind, sealed: &str) -> serde_json::Result<Self> {
        Self::from_entries(kind, serde_json::from_str(sealed)?)
    }

    fn from_entries(kind: Kind, entries: Map<String, Value>) -> serde_json::Result<Self> {
        let object = Value::Object(entries);
        Ok(match kind {
            Kind::Webhook => Self::Webhook(serde_json::from_value(object)?),
            Kind::S3 => Self::S3(serde_json::from_value(object)?),
            Kind::Syslog => Self::Syslog(serde_json::from_value(object)?),
        })
    }

    fn entries(&self) -> Map<String, Value> {
        let value = match self {
            Self::Webhook(secrets) => serde_json::to_value(secrets),
            Self::S3(secrets) => serde_json::to_value(secrets),
            Self::Syslog(secrets) => serde_json::to_value(secrets),
        }
        .expect("secrets hold only strings");
        let Value::Object(entries) = value else {
            unreachable!("secrets serialize to an object");
        };
        entries
    }

    fn validated(self) -> Result<Self, Error> {
        match self {
            Self::Syslog(secrets) => secrets.validated().map(Self::Syslog),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----\n";

    fn bodies() -> [(Kind, Value); 3] {
        [
            (
                Kind::Webhook,
                json!({"kind": "webhook", "url": "http://10.0.0.5/hook"}),
            ),
            (
                Kind::S3,
                json!({
                    "kind": "s3",
                    "bucket": "audit-logs",
                    "region": "us-east-1",
                    "endpoint": "http://minio:9000",
                    "prefix": "zone/",
                    "addressing": "path",
                }),
            ),
            (
                Kind::Syslog,
                json!({
                    "kind": "syslog",
                    "host": "siem.internal",
                    "port": 6514,
                    "protocol": "tls",
                    "facility": 10,
                    "hostname": "zone-eu",
                }),
            ),
        ]
    }

    fn webhook_secrets() -> Secrets {
        Secrets::parse(Kind::Webhook, json!({"signing_secret": "whsec"})).unwrap()
    }

    fn s3_secrets() -> Secrets {
        Secrets::parse(
            Kind::S3,
            json!({"access_key_id": "AKIAEXAMPLE", "secret_access_key": "wJalrXUtnFEMI"}),
        )
        .unwrap()
    }

    fn syslog_secrets() -> Secrets {
        Secrets::parse(
            Kind::Syslog,
            json!({"ca_certificate": CERTIFICATE, "client_certificate": CERTIFICATE, "client_key": KEY}),
        )
        .unwrap()
    }

    #[test]
    fn a_name_is_trimmed_and_printable() {
        assert_eq!(name("  Splunk  ").unwrap(), "Splunk");
        assert_eq!(name("SIEM — EU").unwrap(), "SIEM — EU");
        for value in ["", "   ", "Spl\nunk", "a\u{7}b", "tab\there"] {
            assert_eq!(name(value), Err(Error::NameInvalid), "{value:?}");
        }
    }

    #[test]
    fn a_url_is_absolute_http_with_a_host_and_nothing_else() {
        for raw in [
            "http://10.0.0.5/hook",
            "http://splunk:8088/services/collector",
            "http://localhost:3000",
            "http://[::1]:8080/events",
            "https://hooks.example.com/zone",
            " https://example.com ",
        ] {
            assert_eq!(url(raw).as_deref(), Ok(raw.trim()), "{raw:?}");
        }
        for (raw, reason) in [
            ("", UrlReason::Unparseable),
            ("example.com/hook", UrlReason::Unparseable),
            ("ftp://example.com", UrlReason::Scheme),
            ("file:///etc/passwd", UrlReason::Scheme),
            ("unix:/var/run/socket", UrlReason::Scheme),
            ("http://user:secret@example.com", UrlReason::Userinfo),
            ("https://user@example.com", UrlReason::Userinfo),
            ("https://example.com/hook?token=1", UrlReason::Query),
            ("https://example.com/hook?", UrlReason::Query),
            ("https://example.com/hook#part", UrlReason::Fragment),
        ] {
            assert_eq!(url(raw), Err(Error::UrlInvalid(reason)), "{raw:?}");
        }
    }

    #[test]
    fn a_tagged_body_for_every_kind_deserializes() {
        for (kind, body) in bodies() {
            let settings: Settings = serde_json::from_value(body.clone()).unwrap();
            assert_eq!(settings.kind(), kind);
            assert_eq!(serde_json::to_value(&settings).unwrap(), body);
        }
    }

    #[test]
    fn a_tagged_body_takes_the_defaults_of_its_kind() {
        let s3: Settings = serde_json::from_value(
            json!({"kind": "s3", "bucket": "audit-logs", "region": "eu-west-2"}),
        )
        .unwrap();
        assert_eq!(
            s3,
            Settings::S3(S3Settings {
                bucket: "audit-logs".into(),
                region: "eu-west-2".into(),
                endpoint: None,
                prefix: String::new(),
                addressing: Addressing::VirtualHost,
            })
        );
        let syslog: Settings = serde_json::from_value(
            json!({"kind": "syslog", "host": "10.0.0.9", "port": 514, "protocol": "udp"}),
        )
        .unwrap();
        let Settings::Syslog(syslog) = syslog else {
            panic!("a syslog body is syslog settings");
        };
        assert_eq!(syslog.facility.value(), 13);
        assert_eq!(syslog.hostname, "zone");
    }

    #[test]
    fn an_unknown_field_in_a_tagged_body_is_rejected() {
        for (kind, mut body) in bodies() {
            body["password"] = json!("hunter2");
            assert!(
                serde_json::from_value::<Settings>(body).is_err(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn another_kinds_field_in_a_tagged_body_is_rejected() {
        let body = json!({"kind": "webhook", "url": "https://example.com", "bucket": "audit-logs"});
        assert!(serde_json::from_value::<Settings>(body).is_err());
        let body = json!({"kind": "s3", "url": "https://example.com"});
        assert!(serde_json::from_value::<Settings>(body).is_err());
    }

    #[test]
    fn an_unknown_or_missing_kind_is_rejected() {
        for body in [
            json!({"kind": "kafka", "url": "https://example.com"}),
            json!({"kind": "Webhook", "url": "https://example.com"}),
            json!({"url": "https://example.com"}),
        ] {
            assert!(
                serde_json::from_value::<Settings>(body.clone()).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn validation_runs_each_kinds_rules() {
        for (_, body) in bodies() {
            let settings: Settings = serde_json::from_value(body).unwrap();
            assert_eq!(settings.clone().validated(), Ok(settings));
        }
        let invalid: Settings =
            serde_json::from_value(json!({"kind": "webhook", "url": "ftp://example.com"})).unwrap();
        assert_eq!(
            invalid.validated(),
            Err(Error::UrlInvalid(UrlReason::Scheme))
        );
    }

    #[test]
    fn stored_settings_drop_the_tag_and_round_trip() {
        for (kind, body) in bodies() {
            let settings: Settings = serde_json::from_value(body.clone()).unwrap();
            let stored = settings.stored();
            assert!(stored.get("kind").is_none(), "{stored}");
            let mut expected = body;
            expected.as_object_mut().unwrap().remove("kind");
            assert_eq!(stored, expected);
            assert_eq!(Settings::from_stored(kind, stored).unwrap(), settings);
        }
    }

    #[test]
    fn stored_settings_are_read_back_only_when_valid() {
        let stored = json!({"bucket": "Not_A_Bucket", "region": "us-east-1"});
        assert!(Settings::from_stored(Kind::S3, stored).is_err());
        let stored = json!({"url": "https://example.com"});
        assert!(Settings::from_stored(Kind::Syslog, stored).is_err());
    }

    #[test]
    fn settings_admit_only_secrets_of_their_kind() {
        let settings: Vec<Settings> = bodies()
            .into_iter()
            .map(|(_, body)| serde_json::from_value(body).unwrap())
            .collect();
        let secrets = [webhook_secrets(), s3_secrets(), syslog_secrets()];
        for (row, settings) in settings.iter().enumerate() {
            for (column, secrets) in secrets.iter().enumerate() {
                let expected = if row == column {
                    Ok(())
                } else {
                    Err(Error::KindInvalid)
                };
                assert_eq!(settings.admits(secrets), expected, "{row} {column}");
            }
        }
    }

    #[test]
    fn udp_syslog_admits_no_certificates() {
        let udp: Settings = serde_json::from_value(
            json!({"kind": "syslog", "host": "10.0.0.9", "port": 514, "protocol": "udp"}),
        )
        .unwrap();
        assert_eq!(
            udp.admits(&syslog_secrets()),
            Err(Error::CertificateWithoutTls)
        );
        let ca_only = Secrets::parse(Kind::Syslog, json!({"ca_certificate": CERTIFICATE})).unwrap();
        assert_eq!(udp.admits(&ca_only), Err(Error::CertificateWithoutTls));
        assert_eq!(
            udp.admits(&Secrets::parse(Kind::Syslog, Value::Null).unwrap()),
            Ok(())
        );
    }

    #[test]
    fn secret_names_are_the_wire_contract() {
        let values: Vec<Value> = SecretName::ALL
            .iter()
            .map(|name| serde_json::to_value(name).unwrap())
            .collect();
        assert_eq!(
            values,
            [
                "signing_secret",
                "access_key_id",
                "secret_access_key",
                "ca_certificate",
                "client_certificate",
                "client_key",
            ]
        );
        for name in SecretName::ALL {
            assert_eq!(serde_json::to_value(name).unwrap(), name.as_str());
        }
    }

    #[test]
    fn each_kind_names_its_own_secrets() {
        let names = |kind| SecretName::of(kind).collect::<Vec<_>>();
        assert_eq!(names(Kind::Webhook), [SecretName::SigningSecret]);
        assert_eq!(
            names(Kind::S3),
            [SecretName::AccessKeyId, SecretName::SecretAccessKey]
        );
        assert_eq!(
            names(Kind::Syslog),
            [
                SecretName::CaCertificate,
                SecretName::ClientCertificate,
                SecretName::ClientKey,
            ]
        );
        assert!(
            SecretName::ALL
                .iter()
                .all(|name| name.required() == (name.kind() != Kind::Syslog))
        );
    }

    #[test]
    fn webhook_and_s3_secrets_are_required() {
        for (kind, body, missing) in [
            (Kind::Webhook, json!({}), SecretName::SigningSecret),
            (Kind::Webhook, Value::Null, SecretName::SigningSecret),
            (
                Kind::Webhook,
                json!({"signing_secret": "  "}),
                SecretName::SigningSecret,
            ),
            (
                Kind::S3,
                json!({"access_key_id": "AKIA"}),
                SecretName::SecretAccessKey,
            ),
            (
                Kind::S3,
                json!({"secret_access_key": "wJal"}),
                SecretName::AccessKeyId,
            ),
        ] {
            assert_eq!(
                Secrets::parse(kind, body.clone()),
                Err(Error::SecretInvalid(missing)),
                "{body}"
            );
        }
    }

    #[test]
    fn secrets_keep_their_values_exactly() {
        let Secrets::Webhook(secrets) =
            Secrets::parse(Kind::Webhook, json!({"signing_secret": " s3cret:+% "})).unwrap()
        else {
            panic!("webhook secrets");
        };
        assert_eq!(secrets.signing_secret, " s3cret:+% ");
    }

    #[test]
    fn syslog_secrets_are_optional() {
        for body in [Value::Null, json!({}), json!({"ca_certificate": ""})] {
            let secrets = Secrets::parse(Kind::Syslog, body.clone()).unwrap();
            assert_eq!(secrets, Secrets::Syslog(SyslogSecrets::default()), "{body}");
            assert!(secrets.names().is_empty());
        }
    }

    #[test]
    fn secrets_refuse_unknown_names_and_non_text_values() {
        assert_eq!(
            Secrets::parse(
                Kind::Webhook,
                json!({"signing_secret": "a", "password": "b"})
            ),
            Err(Error::SecretUnknown("password".into()))
        );
        assert_eq!(
            Secrets::parse(
                Kind::Webhook,
                json!({"signing_secret": "a", "access_key_id": "b"})
            ),
            Err(Error::SecretUnknown("access_key_id".into()))
        );
        assert_eq!(
            Secrets::parse(Kind::Webhook, json!({"signing_secret": 7})),
            Err(Error::SecretInvalid(SecretName::SigningSecret))
        );
        assert_eq!(
            Secrets::parse(Kind::Syslog, json!({"client_key": null})),
            Err(Error::SecretInvalid(SecretName::ClientKey))
        );
        for body in [json!("whsec"), json!(["whsec"]), json!(1)] {
            assert_eq!(
                Secrets::parse(Kind::Webhook, body.clone()),
                Err(Error::SecretsMalformed),
                "{body}"
            );
        }
    }

    #[test]
    fn names_list_only_secrets_that_hold_a_value() {
        assert_eq!(webhook_secrets().names(), [SecretName::SigningSecret]);
        assert_eq!(
            s3_secrets().names(),
            [SecretName::AccessKeyId, SecretName::SecretAccessKey]
        );
        let ca_only = Secrets::parse(Kind::Syslog, json!({"ca_certificate": CERTIFICATE})).unwrap();
        assert_eq!(ca_only.names(), [SecretName::CaCertificate]);
        assert_eq!(
            syslog_secrets().names(),
            [
                SecretName::CaCertificate,
                SecretName::ClientCertificate,
                SecretName::ClientKey,
            ]
        );
    }

    #[test]
    fn a_merge_replaces_only_the_named_secrets() {
        let merged = s3_secrets()
            .merged(json!({"secret_access_key": "rotated"}))
            .unwrap();
        let Secrets::S3(merged) = merged else {
            panic!("s3 secrets");
        };
        assert_eq!(merged.access_key_id, "AKIAEXAMPLE");
        assert_eq!(merged.secret_access_key, "rotated");
        assert_eq!(s3_secrets().merged(Value::Null), Ok(s3_secrets()));
        assert_eq!(s3_secrets().merged(json!({})), Ok(s3_secrets()));
    }

    #[test]
    fn a_merge_clears_an_optional_secret_with_an_empty_string() {
        let merged = syslog_secrets()
            .merged(json!({"ca_certificate": ""}))
            .unwrap();
        assert_eq!(
            merged.names(),
            [SecretName::ClientCertificate, SecretName::ClientKey]
        );
        let cleared = syslog_secrets()
            .merged(json!({"ca_certificate": "", "client_certificate": "", "client_key": ""}))
            .unwrap();
        assert!(cleared.names().is_empty());
    }

    #[test]
    fn a_merge_cannot_clear_a_required_secret() {
        assert_eq!(
            webhook_secrets().merged(json!({"signing_secret": ""})),
            Err(Error::SecretInvalid(SecretName::SigningSecret))
        );
    }

    #[test]
    fn a_merge_keeps_the_client_certificate_and_key_together() {
        assert_eq!(
            syslog_secrets().merged(json!({"client_key": ""})),
            Err(Error::CertificateUnpaired)
        );
    }

    #[test]
    fn a_merge_refuses_what_a_parse_refuses() {
        assert_eq!(
            webhook_secrets().merged(json!({"access_key_id": "AKIA"})),
            Err(Error::SecretUnknown("access_key_id".into()))
        );
        assert_eq!(
            webhook_secrets().merged(json!({"signing_secret": false})),
            Err(Error::SecretInvalid(SecretName::SigningSecret))
        );
        assert_eq!(
            webhook_secrets().merged(json!("whsec")),
            Err(Error::SecretsMalformed)
        );
        assert_eq!(
            syslog_secrets().merged(json!({"ca_certificate": "not pem"})),
            Err(Error::CertificateInvalid(SecretName::CaCertificate))
        );
    }

    #[test]
    fn the_sealed_form_round_trips() {
        for secrets in [
            webhook_secrets(),
            s3_secrets(),
            syslog_secrets(),
            Secrets::parse(Kind::Syslog, Value::Null).unwrap(),
        ] {
            let sealed = secrets.sealed_form();
            assert_eq!(
                Secrets::from_sealed_form(secrets.kind(), &sealed).unwrap(),
                secrets,
                "{sealed}"
            );
        }
        assert_eq!(
            webhook_secrets().sealed_form(),
            r#"{"signing_secret":"whsec"}"#
        );
        assert_eq!(
            Secrets::parse(Kind::Syslog, Value::Null)
                .unwrap()
                .sealed_form(),
            "{}"
        );
    }

    #[test]
    fn the_sealed_form_is_read_only_as_its_own_kind() {
        let sealed = webhook_secrets().sealed_form();
        assert!(Secrets::from_sealed_form(Kind::S3, &sealed).is_err());
        assert!(Secrets::from_sealed_form(Kind::Syslog, &sealed).is_err());
        assert!(Secrets::from_sealed_form(Kind::Webhook, "not json").is_err());
    }

    #[test]
    fn debug_never_shows_a_secret() {
        for (secrets, values) in [
            (webhook_secrets(), vec!["whsec"]),
            (s3_secrets(), vec!["AKIAEXAMPLE", "wJalrXUtnFEMI"]),
            (syslog_secrets(), vec!["MIIB", "MIIE", "BEGIN"]),
        ] {
            let shown = format!("{secrets:?} {secrets:#?}");
            for value in values {
                assert!(!shown.contains(value), "{shown}");
            }
            assert!(shown.contains(REDACTED), "{shown}");
        }
        let empty = format!("{:?}", Secrets::parse(Kind::Syslog, Value::Null).unwrap());
        assert!(empty.contains("None"), "{empty}");
    }
}
