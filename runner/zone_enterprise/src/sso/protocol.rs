use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Oidc,
    Saml,
}

impl Protocol {
    pub const ALL: [Self; 2] = [Self::Oidc, Self::Saml];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Saml => "saml",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|protocol| protocol.as_str() == value)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Protocol::Oidc).unwrap(), r#""oidc""#);
        assert_eq!(serde_json::to_string(&Protocol::Saml).unwrap(), r#""saml""#);
        assert_eq!(
            serde_json::from_str::<Protocol>(r#""oidc""#).unwrap(),
            Protocol::Oidc
        );
        assert_eq!(
            serde_json::from_str::<Protocol>(r#""saml""#).unwrap(),
            Protocol::Saml
        );
    }

    #[test]
    fn defaults_to_oidc() {
        assert_eq!(Protocol::default(), Protocol::Oidc);
    }

    #[test]
    fn lists_every_protocol_once() {
        assert_eq!(Protocol::ALL, [Protocol::Oidc, Protocol::Saml]);
    }

    #[test]
    fn displays_and_parses_its_wire_value() {
        for protocol in Protocol::ALL {
            assert_eq!(
                serde_json::to_value(protocol).unwrap(),
                protocol.to_string()
            );
            assert_eq!(Protocol::parse(protocol.as_str()), Some(protocol));
        }
    }

    #[test]
    fn refuses_an_unknown_protocol() {
        assert_eq!(Protocol::parse("ldap"), None);
        assert_eq!(Protocol::parse("OIDC"), None);
        assert_eq!(Protocol::parse("SAML"), None);
        assert_eq!(Protocol::parse(""), None);
    }
}
