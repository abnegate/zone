use super::*;

const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
const KEY: &str = "-----BEGIN EC PRIVATE KEY-----\nMHcC\n-----END EC PRIVATE KEY-----\n";

fn settings() -> SyslogSettings {
    SyslogSettings {
        host: "siem.internal".into(),
        port: 6514,
        protocol: Protocol::Tls,
        facility: Facility::default(),
        hostname: default_hostname(),
    }
}

fn secrets(
    ca_certificate: Option<&str>,
    client_certificate: Option<&str>,
    client_key: Option<&str>,
) -> SyslogSecrets {
    SyslogSecrets {
        ca_certificate: ca_certificate.map(Into::into),
        client_certificate: client_certificate.map(Into::into),
        client_key: client_key.map(Into::into),
    }
}

#[test]
fn protocols_are_lowercase() {
    let values: Vec<String> = Protocol::ALL
        .iter()
        .map(|protocol| serde_json::to_string(protocol).unwrap())
        .collect();
    assert_eq!(values, [r#""tls""#, r#""udp""#]);
    for protocol in Protocol::ALL {
        assert_eq!(serde_json::to_value(protocol).unwrap(), protocol.as_str());
    }
    assert!(serde_json::from_str::<Protocol>(r#""tcp""#).is_err());
}

#[test]
fn hosts_are_hostnames_or_ip_literals() {
    for (raw, expected) in [
        ("siem.internal", "siem.internal"),
        ("SIEM.Example.com.", "siem.example.com"),
        ("splunk", "splunk"),
        ("zone_syslog", "zone_syslog"),
        ("10.0.0.9", "10.0.0.9"),
        ("::1", "::1"),
        ("[2001:DB8:0:0::1]", "2001:db8::1"),
    ] {
        let validated = SyslogSettings {
            host: format!(" {raw} "),
            ..settings()
        }
        .validated()
        .unwrap();
        assert_eq!(validated.host, expected, "{raw:?}");
    }
}

#[test]
fn hosts_refuse_schemes_ports_paths_and_malformed_names() {
    for raw in [
        "",
        "tls://siem.internal",
        "siem.internal:6514",
        "siem.internal/path",
        "user@siem",
        "siem..internal",
        "-siem",
        "siem-",
        "si em",
        "bücher.de",
        "10.0.0.256",
        "1.2.3",
        "[10.0.0.1]",
        "[::1",
    ] {
        assert_eq!(
            SyslogSettings {
                host: raw.into(),
                ..settings()
            }
            .validated(),
            Err(Error::HostInvalid(raw.into())),
            "{raw:?}"
        );
    }
}

#[test]
fn port_zero_is_refused() {
    assert_eq!(
        SyslogSettings {
            port: 0,
            ..settings()
        }
        .validated(),
        Err(Error::PortInvalid)
    );
    for port in [1, 514, 6514, u16::MAX] {
        assert!(
            SyslogSettings { port, ..settings() }.validated().is_ok(),
            "{port}"
        );
    }
}

#[test]
fn facilities_run_from_zero_to_twenty_three() {
    assert_eq!(Facility::default().value(), 13);
    for value in 0..=23 {
        assert_eq!(Facility::new(value).unwrap().value(), value);
    }
    for value in [24, 255] {
        assert_eq!(Facility::new(value), Err(Error::FacilityInvalid(value)));
        let deserialized: Facility = serde_json::from_value(value.into()).unwrap();
        assert_eq!(
            SyslogSettings {
                facility: deserialized,
                ..settings()
            }
            .validated(),
            Err(Error::FacilityInvalid(value))
        );
    }
    assert!(serde_json::from_str::<Facility>("256").is_err());
    assert_eq!(serde_json::to_string(&Facility::default()).unwrap(), "13");
}

#[test]
fn hostnames_are_printable_ascii_without_spaces() {
    for hostname in ["zone", "zone-eu.example.com", "a", &"z".repeat(255), "!~"] {
        assert!(
            SyslogSettings {
                hostname: hostname.into(),
                ..settings()
            }
            .validated()
            .is_ok(),
            "{hostname:?}"
        );
    }
    for hostname in ["", "zone eu", "zöne", "zone\tu", &"z".repeat(256)] {
        assert_eq!(
            SyslogSettings {
                hostname: hostname.into(),
                ..settings()
            }
            .validated(),
            Err(Error::HostnameInvalid(hostname.trim().into())),
            "{hostname:?}"
        );
    }
}

#[test]
fn the_authority_brackets_ipv6() {
    assert_eq!(settings().authority(), "siem.internal:6514");
    let ipv6 = SyslogSettings {
        host: "[::1]".into(),
        port: 514,
        ..settings()
    }
    .validated()
    .unwrap();
    assert_eq!(ipv6.authority(), "[::1]:514");
    let ipv4 = SyslogSettings {
        host: "10.0.0.9".into(),
        ..settings()
    };
    assert_eq!(ipv4.authority(), "10.0.0.9:6514");
}

#[test]
fn certificates_must_be_pem() {
    assert!(
        secrets(Some(CERTIFICATE), Some(CERTIFICATE), Some(KEY))
            .validated()
            .is_ok()
    );
    let pkcs8 = "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----";
    assert!(
        secrets(None, Some(CERTIFICATE), Some(pkcs8))
            .validated()
            .is_ok()
    );
    for (candidate, name) in [
        (secrets(Some("MIIB"), None, None), SecretName::CaCertificate),
        (
            secrets(Some("-----BEGIN CERTIFICATE-----\nMIIB"), None, None),
            SecretName::CaCertificate,
        ),
        (
            secrets(None, Some(KEY), Some(KEY)),
            SecretName::ClientCertificate,
        ),
        (
            secrets(None, Some(CERTIFICATE), Some(CERTIFICATE)),
            SecretName::ClientKey,
        ),
    ] {
        assert_eq!(candidate.validated(), Err(Error::CertificateInvalid(name)));
    }
}

#[test]
fn a_client_certificate_needs_its_key() {
    assert_eq!(
        secrets(None, Some(CERTIFICATE), None).validated(),
        Err(Error::CertificateUnpaired)
    );
    assert_eq!(
        secrets(Some(CERTIFICATE), None, Some(KEY)).validated(),
        Err(Error::CertificateUnpaired)
    );
}

#[test]
fn debug_shows_which_secrets_are_set_but_never_their_values() {
    let shown = format!("{:?}", secrets(Some(CERTIFICATE), None, None));
    assert!(
        !shown.contains("MIIB") && !shown.contains("BEGIN"),
        "{shown}"
    );
    assert!(
        shown.contains(REDACTED) && shown.contains("None"),
        "{shown}"
    );
}
