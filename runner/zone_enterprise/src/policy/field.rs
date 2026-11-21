use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Providers,
    Agents,
    Models,
    Ceiling,
    McpServers,
    Domains,
}

impl Field {
    pub const ALL: [Self; 6] = [
        Self::Providers,
        Self::Agents,
        Self::Models,
        Self::Ceiling,
        Self::McpServers,
        Self::Domains,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Providers => "providers",
            Self::Agents => "agents",
            Self::Models => "models",
            Self::Ceiling => "ceiling",
            Self::McpServers => "mcp_servers",
            Self::Domains => "domains",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Providers => "providers",
            Self::Agents => "coding agents",
            Self::Models => "models",
            Self::Ceiling => "tool approval ceiling",
            Self::McpServers => "MCP servers",
            Self::Domains => "domains",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_snake_case() {
        for field in Field::ALL {
            assert_eq!(serde_json::to_value(field).unwrap(), field.as_str());
        }
        assert_eq!(
            serde_json::to_string(&Field::McpServers).unwrap(),
            r#""mcp_servers""#
        );
    }

    #[test]
    fn refuses_unknown_values() {
        for value in [r#""mcpServers""#, r#""tier""#] {
            assert!(serde_json::from_str::<Field>(value).is_err(), "{value}");
        }
    }
}
