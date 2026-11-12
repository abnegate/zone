use std::fmt;
use std::str::FromStr;

use abnegate_agent::mcp::McpTransport;
use serde::Deserialize;
use serde::Serialize;

use crate::services::mcp_servers::validation::ValidationError;

const STDIO: &str = "stdio";
const HTTP: &str = "http";
const SSE: &str = "sse";
const FIELD: &str = "transport";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    Http,
    Sse,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => STDIO,
            Self::Http => HTTP,
            Self::Sse => SSE,
        }
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Transport {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            STDIO => Ok(Self::Stdio),
            HTTP => Ok(Self::Http),
            SSE => Ok(Self::Sse),
            _ => Err(ValidationError::Transport {
                field: FIELD.to_string(),
                value: value.to_string(),
            }),
        }
    }
}

impl From<Transport> for McpTransport {
    fn from(transport: Transport) -> Self {
        match transport {
            Transport::Stdio => Self::Stdio,
            Transport::Http => Self::Http,
            Transport::Sse => Self::Sse,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Transport; 3] = [Transport::Stdio, Transport::Http, Transport::Sse];

    #[test]
    fn serde_uses_the_lowercase_name() {
        assert_eq!(serde_json::to_string(&Transport::Http).unwrap(), "\"http\"");
        assert_eq!(
            serde_json::from_str::<Transport>("\"http\"").unwrap(),
            Transport::Http
        );
        for transport in ALL {
            let json = serde_json::to_string(&transport).unwrap();
            assert_eq!(json, format!("\"{}\"", transport.as_str()));
            assert_eq!(serde_json::from_str::<Transport>(&json).unwrap(), transport);
        }
    }

    #[test]
    fn serde_refuses_other_spellings() {
        assert!(serde_json::from_str::<Transport>("\"Http\"").is_err());
        assert!(serde_json::from_str::<Transport>("\"streamable-http\"").is_err());
    }

    #[test]
    fn from_str_round_trips_as_str() {
        for transport in ALL {
            assert_eq!(transport.as_str().parse::<Transport>(), Ok(transport));
            assert_eq!(transport.to_string(), transport.as_str());
        }
    }

    #[test]
    fn from_str_refuses_unknown_names() {
        for value in ["HTTP", "streamable-http", "ws", ""] {
            assert_eq!(
                value.parse::<Transport>(),
                Err(ValidationError::Transport {
                    field: FIELD.to_string(),
                    value: value.to_string(),
                })
            );
        }
        assert_eq!(
            "ws".parse::<Transport>().unwrap_err().to_string(),
            "`transport` must be `stdio`, `http` or `sse`, not `ws`"
        );
    }

    #[test]
    fn maps_to_the_crate_transport() {
        assert_eq!(McpTransport::from(Transport::Stdio), McpTransport::Stdio);
        assert_eq!(McpTransport::from(Transport::Http), McpTransport::Http);
        assert_eq!(McpTransport::from(Transport::Sse), McpTransport::Sse);
        for transport in ALL {
            assert_eq!(McpTransport::from(transport).as_str(), transport.as_str());
        }
    }
}
