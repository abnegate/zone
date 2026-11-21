use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    Connect,
    Timeout,
    Tls,
    Status,
    Target,
    Configuration,
    Unsealable,
}

impl FailureCode {
    pub const ALL: [Self; 7] = [
        Self::Connect,
        Self::Timeout,
        Self::Tls,
        Self::Status,
        Self::Target,
        Self::Configuration,
        Self::Unsealable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Timeout => "timeout",
            Self::Tls => "tls",
            Self::Status => "status",
            Self::Target => "target",
            Self::Configuration => "configuration",
            Self::Unsealable => "unsealable",
        }
    }

    /// The sentence a failure shows its organization's admins. It names no host, port or
    /// underlying error, so failures can't be used to probe the network Zone runs on.
    pub fn message(self) -> &'static str {
        match self {
            Self::Connect => "Zone couldn't connect to the destination or finish sending to it.",
            Self::Timeout => "The destination didn't answer in time.",
            Self::Tls => "The TLS handshake with the destination failed.",
            Self::Status => "The destination answered with a status outside 2xx.",
            Self::Target => {
                "The destination's host resolves only to link-local or cloud metadata addresses, which audit delivery never reaches."
            }
            Self::Configuration => "The destination can't be used as saved.",
            Self::Unsealable => {
                "The destination's secrets can't be opened with this server's encryption key."
            }
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == value)
    }
}

impl fmt::Display for FailureCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_the_wire_contract() {
        let values: Vec<&str> = FailureCode::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(
            values,
            [
                "connect",
                "timeout",
                "tls",
                "status",
                "target",
                "configuration",
                "unsealable"
            ]
        );
    }

    #[test]
    fn serializes_and_displays_its_wire_value() {
        for code in FailureCode::ALL {
            assert_eq!(serde_json::to_value(code).unwrap(), code.as_str());
            assert_eq!(code.to_string(), code.as_str());
        }
    }

    #[test]
    fn every_message_is_one_fixed_sentence() {
        for code in FailureCode::ALL {
            let message = code.message();
            assert!(
                message.ends_with('.') && !message.contains('\n'),
                "{message}"
            );
            assert!(!message.contains(':'), "{code}: {message}");
        }
    }

    #[test]
    fn parses_its_wire_value() {
        for code in FailureCode::ALL {
            assert_eq!(FailureCode::parse(code.as_str()), Some(code));
            assert_eq!(
                serde_json::from_value::<FailureCode>(code.as_str().into()).unwrap(),
                code
            );
        }
        assert_eq!(FailureCode::parse("Connect"), None);
        assert_eq!(FailureCode::parse(""), None);
    }
}
