use abnegate_agent::mcp::McpTransport;
use serde::Serialize;

use super::definition::Definition;

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

/// The connection `definition` has whatever its server does, or `None` when
/// only connecting it can tell.
pub fn static_status(definition: &Definition, commands: bool) -> Option<Connection> {
    let server = &definition.server;
    if !definition.enabled || server.disabled {
        return Some(Connection::disabled());
    }
    if server.command.is_some() {
        if !commands {
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

    fn definition(server: McpServer) -> Definition {
        Definition::of("docs", server)
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
    fn a_disabled_definition_is_disabled_before_anything_else() {
        let mut stdio = definition(McpServer::command("python3", ["stub.py"]));
        stdio.enabled = false;
        assert_eq!(static_status(&stdio, false), Some(Connection::disabled()));

        let mut sse =
            definition(McpServer::remote("http://127.0.0.1/sse").with_transport(McpTransport::Sse));
        sse.enabled = false;
        assert_eq!(static_status(&sse, true), Some(Connection::disabled()));
    }

    #[test]
    fn a_command_server_needs_commands_turned_on() {
        let stdio = definition(McpServer::command("python3", ["stub.py"]));
        assert_eq!(
            static_status(&stdio, false),
            Some(Connection::failed(COMMANDS_DISABLED))
        );
        assert_eq!(static_status(&stdio, true), None);
    }

    #[test]
    fn remote_servers_are_not_connected_by_this_build() {
        let sse =
            definition(McpServer::remote("http://127.0.0.1/sse").with_transport(McpTransport::Sse));
        assert_eq!(
            static_status(&sse, true),
            Some(Connection::failed(SSE_UNSUPPORTED))
        );

        let http = definition(McpServer::remote("http://127.0.0.1/mcp"));
        assert_eq!(
            static_status(&http, true),
            Some(Connection::failed(HTTP_PENDING))
        );
    }

    #[test]
    fn an_invalid_server_is_unconnected() {
        let blank = definition(McpServer::command(" ", Vec::<String>::new()));
        assert_eq!(
            static_status(&blank, true),
            Some(Connection::failed(UNCONNECTED))
        );
    }
}
