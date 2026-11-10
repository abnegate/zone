use abnegate_agent::mcp::McpTransport;
use serde::Serialize;

use super::definition::Definition;

pub const MCP_DISABLED: &str = "MCP servers are turned off on this instance (ZONE_MCP_ENABLED)";
pub const COMMANDS_DISABLED: &str =
    "Command servers are turned off on this instance (ZONE_MCP_COMMANDS)";
pub const SSE_UNSUPPORTED: &str = "SSE servers are not supported; use streamable HTTP (`http`)";
pub const HTTP_PENDING: &str = "Streamable HTTP servers are not connected by this build yet";
pub const UNCONNECTED: &str = "did not connect; see the server log";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Connected,
    Failed,
    Disabled,
    Connecting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub status: Status,
    pub error: Option<String>,
    pub tools: Vec<String>,
}

impl Connection {
    pub fn connected(tools: Vec<String>) -> Self {
        Self {
            status: Status::Connected,
            error: None,
            tools,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: Status::Failed,
            error: Some(error.into()),
            tools: Vec::new(),
        }
    }

    pub fn disabled() -> Self {
        Self::quiet(Status::Disabled)
    }

    pub fn connecting() -> Self {
        Self::quiet(Status::Connecting)
    }

    fn quiet(status: Status) -> Self {
        Self {
            status,
            error: None,
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub enabled: bool,
    pub commands: bool,
}

pub fn static_status(definition: &Definition, policy: Policy) -> Option<Connection> {
    if !policy.enabled {
        return Some(Connection::failed(MCP_DISABLED));
    }
    let server = &definition.server;
    if !definition.enabled || server.disabled {
        return Some(Connection::disabled());
    }
    if server.command.is_some() {
        if !policy.commands {
            return Some(Connection::failed(COMMANDS_DISABLED));
        }
    } else if server.transport == Some(McpTransport::Sse) {
        return Some(Connection::failed(SSE_UNSUPPORTED));
    } else if server.url.is_some() {
        return Some(Connection::failed(HTTP_PENDING));
    }
    (!server.valid()).then(|| Connection::failed(UNCONNECTED))
}

#[cfg(test)]
mod tests {
    use abnegate_agent::mcp::McpServer;

    use super::*;

    const COMMANDS: Policy = Policy {
        enabled: true,
        commands: true,
    };
    const REMOTE: Policy = Policy {
        enabled: true,
        commands: false,
    };
    const OFF: Policy = Policy {
        enabled: false,
        commands: true,
    };

    fn definition(server: McpServer) -> Definition {
        Definition::of("docs", server)
    }

    #[test]
    fn the_master_switch_fails_every_definition_before_anything_else() {
        let stdio = definition(McpServer::command("python3", ["stub.py"]));
        let http = definition(McpServer::remote("http://127.0.0.1/mcp"));
        let mut disabled = definition(McpServer::command("python3", ["stub.py"]));
        disabled.enabled = false;

        for definition in [stdio, http, disabled] {
            assert_eq!(
                static_status(&definition, OFF),
                Some(Connection::failed(MCP_DISABLED)),
                "{definition:?}"
            );
        }
    }

    #[test]
    fn statuses_serialize_as_the_frontend_reads_them() {
        assert_eq!(
            serde_json::to_value([
                Status::Connected,
                Status::Failed,
                Status::Disabled,
                Status::Connecting
            ])
            .unwrap(),
            serde_json::json!(["connected", "failed", "disabled", "connecting"])
        );
        assert_eq!(
            serde_json::to_value(Connection::failed(UNCONNECTED)).unwrap(),
            serde_json::json!({"status": "failed", "error": UNCONNECTED, "tools": []})
        );
    }

    #[test]
    fn a_disabled_definition_is_disabled_whatever_its_server() {
        let mut stdio = definition(McpServer::command("python3", ["stub.py"]));
        stdio.enabled = false;
        assert_eq!(static_status(&stdio, REMOTE), Some(Connection::disabled()));

        let mut sse =
            definition(McpServer::remote("http://127.0.0.1/sse").with_transport(McpTransport::Sse));
        sse.enabled = false;
        assert_eq!(static_status(&sse, COMMANDS), Some(Connection::disabled()));
    }

    #[test]
    fn a_command_server_needs_commands_turned_on() {
        let stdio = definition(McpServer::command("python3", ["stub.py"]));
        assert_eq!(
            static_status(&stdio, REMOTE),
            Some(Connection::failed(COMMANDS_DISABLED))
        );
        assert_eq!(static_status(&stdio, COMMANDS), None);
    }

    #[test]
    fn remote_servers_are_not_connected_by_this_build() {
        let sse =
            definition(McpServer::remote("http://127.0.0.1/sse").with_transport(McpTransport::Sse));
        assert_eq!(
            static_status(&sse, COMMANDS),
            Some(Connection::failed(SSE_UNSUPPORTED))
        );

        let http = definition(McpServer::remote("http://127.0.0.1/mcp"));
        assert_eq!(
            static_status(&http, COMMANDS),
            Some(Connection::failed(HTTP_PENDING))
        );
    }

    #[test]
    fn an_invalid_server_is_unconnected() {
        let blank = definition(McpServer::command(" ", Vec::<String>::new()));
        assert_eq!(
            static_status(&blank, COMMANDS),
            Some(Connection::failed(UNCONNECTED))
        );
    }
}
