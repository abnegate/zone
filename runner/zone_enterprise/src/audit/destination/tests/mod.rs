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
