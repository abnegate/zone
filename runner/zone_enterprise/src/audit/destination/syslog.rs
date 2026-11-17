use std::fmt;
use std::net::{IpAddr, Ipv6Addr};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::audit::destination::{REDACTED, SecretName};
use crate::audit::error::Error;

const DEFAULT_FACILITY: u8 = 13;
const MAXIMUM_FACILITY: u8 = 23;
const DEFAULT_HOSTNAME: &str = "zone";
const HOSTNAME_LENGTH: RangeInclusive<usize> = 1..=255;
const PRINTABLE: RangeInclusive<u8> = 33..=126;
const HOST_MAXIMUM: usize = 253;
const LABEL_MAXIMUM: usize = 63;
const DOT: char = '.';
const HYPHEN: char = '-';
const UNDERSCORE: char = '_';
const OPENING_BRACKET: char = '[';
const CLOSING_BRACKET: char = ']';
const CERTIFICATE_BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const CERTIFICATE_END: &str = "-----END CERTIFICATE-----";
const KEY_BEGIN: &str = "-----BEGIN ";
const KEY_END: &str = "-----END ";
const KEY_LABEL: &str = "PRIVATE KEY-----";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tls,
    Udp,
}

impl Protocol {
    pub const ALL: [Self; 2] = [Self::Tls, Self::Udp];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Facility(u8);

impl Facility {
    pub fn new(value: u8) -> Result<Self, Error> {
        if value > MAXIMUM_FACILITY {
            return Err(Error::FacilityInvalid(value));
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u8 {
        self.0
    }
}

impl Default for Facility {
    fn default() -> Self {
        Self(DEFAULT_FACILITY)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyslogSettings {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    #[serde(default)]
    pub facility: Facility,
    #[serde(default = "default_hostname")]
    pub hostname: String,
}

impl SyslogSettings {
    pub fn validated(self) -> Result<Self, Error> {
        let raw = self.host.trim();
        let host = host(raw).ok_or_else(|| Error::HostInvalid(raw.to_owned()))?;
        if self.port == 0 {
            return Err(Error::PortInvalid);
        }
        let hostname = self.hostname.trim();
        if !valid_hostname(hostname) {
            return Err(Error::HostnameInvalid(hostname.to_owned()));
        }
        Ok(Self {
            host,
            port: self.port,
            protocol: self.protocol,
            facility: Facility::new(self.facility.value())?,
            hostname: hostname.to_owned(),
        })
    }

    pub fn authority(&self) -> String {
        match self.host.parse::<Ipv6Addr>() {
            Ok(address) => format!("[{address}]:{}", self.port),
            Err(_) => format!("{}:{}", self.host, self.port),
        }
    }
}

fn default_hostname() -> String {
    DEFAULT_HOSTNAME.to_owned()
}

fn host(raw: &str) -> Option<String> {
    let lowered = raw.to_ascii_lowercase();
    let unbracketed = lowered
        .strip_prefix(OPENING_BRACKET)
        .and_then(|inner| inner.strip_suffix(CLOSING_BRACKET));
    if let Some(address) = unbracketed {
        return address
            .parse::<Ipv6Addr>()
            .ok()
            .map(|address| address.to_string());
    }
    if let Ok(address) = lowered.parse::<IpAddr>() {
        return Some(address.to_string());
    }
    let name = lowered.strip_suffix(DOT).unwrap_or(&lowered);
    valid_name(name).then(|| name.to_owned())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= HOST_MAXIMUM
        && name.split(DOT).all(valid_label)
        && !name
            .rsplit(DOT)
            .next()
            .is_some_and(|top| top.chars().all(|character| character.is_ascii_digit()))
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= LABEL_MAXIMUM
        && !label.starts_with(HYPHEN)
        && !label.ends_with(HYPHEN)
        && label.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == HYPHEN
                || character == UNDERSCORE
        })
}

fn valid_hostname(hostname: &str) -> bool {
    HOSTNAME_LENGTH.contains(&hostname.len())
        && hostname.bytes().all(|byte| PRINTABLE.contains(&byte))
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyslogSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_certificate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_certificate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key: Option<String>,
}

impl SyslogSecrets {
    pub(super) fn validated(self) -> Result<Self, Error> {
        for (name, value) in [
            (SecretName::CaCertificate, &self.ca_certificate),
            (SecretName::ClientCertificate, &self.client_certificate),
        ] {
            if value.as_deref().is_some_and(|value| !certificate(value)) {
                return Err(Error::CertificateInvalid(name));
            }
        }
        if self.client_key.as_deref().is_some_and(|value| !key(value)) {
            return Err(Error::CertificateInvalid(SecretName::ClientKey));
        }
        if self.client_certificate.is_some() != self.client_key.is_some() {
            return Err(Error::CertificateUnpaired);
        }
        Ok(self)
    }
}

fn certificate(value: &str) -> bool {
    value
        .find(CERTIFICATE_BEGIN)
        .is_some_and(|begin| value[begin..].contains(CERTIFICATE_END))
}

fn key(value: &str) -> bool {
    value.find(KEY_BEGIN).is_some_and(|begin| {
        let block = &value[begin..];
        block.contains(KEY_LABEL) && block.contains(KEY_END)
    })
}

impl fmt::Debug for SyslogSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyslogSecrets")
            .field(
                "ca_certificate",
                &self.ca_certificate.as_ref().map(|_| REDACTED),
            )
            .field(
                "client_certificate",
                &self.client_certificate.as_ref().map(|_| REDACTED),
            )
            .field("client_key", &self.client_key.as_ref().map(|_| REDACTED))
            .finish()
    }
}

#[cfg(test)]
mod tests {
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
}
