//! Live connections to stdio MCP servers.

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use std::sync::Arc;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::Mutex;

use super::config::{McpConfig, McpServerSpec};
use super::tool::{McpTool, qualified_tool_name};
use crate::tools::Tool as ZoneTool;

/// An MCP client or config failure. Connecting is best-effort: one bad server
/// does not take the others down.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("failed to start MCP server '{server}': {source}")]
    Spawn {
        server: String,
        #[source]
        source: std::io::Error,
    },
    #[error("MCP handshake failed for '{server}': {message}")]
    Handshake { server: String, message: String },
    #[error("MCP tool call failed: {0}")]
    Call(String),
}

/// Connected MCP servers and the Zone tools they exported.
pub struct McpHub {
    sessions: Vec<Arc<McpSession>>,
}

impl McpHub {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }

    /// Connect every server in the process environment config.
    pub async fn connect_from_env() -> Self {
        Self::connect(&McpConfig::from_env()).await
    }

    /// Connect the given servers. Failures are logged and skipped.
    pub async fn connect(config: &McpConfig) -> Self {
        let mut hub = Self::new();
        for spec in &config.servers {
            match McpSession::connect(spec).await {
                Ok(session) => {
                    tracing::info!(
                        server = %spec.name,
                        tools = session.remote_tools.len(),
                        "Connected MCP server"
                    );
                    hub.sessions.push(Arc::new(session));
                }
                Err(error) => {
                    tracing::warn!(
                        server = %spec.name,
                        error = %error,
                        "MCP server unavailable"
                    );
                }
            }
        }
        hub
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn server_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn server_names(&self) -> Vec<String> {
        self.sessions
            .iter()
            .map(|session| session.name.clone())
            .collect()
    }

    /// Zone tools for every advertised remote tool.
    pub fn tools(&self) -> Vec<Arc<dyn ZoneTool>> {
        self.sessions
            .iter()
            .flat_map(|session| session.zone_tools())
            .collect()
    }
}

impl Default for McpHub {
    fn default() -> Self {
        Self::new()
    }
}

/// One live stdio MCP session.
pub(crate) struct McpSession {
    pub name: String,
    pub remote_tools: Vec<Tool>,
    client: Mutex<RunningService<rmcp::RoleClient, ()>>,
}

impl McpSession {
    async fn connect(spec: &McpServerSpec) -> Result<Self, McpError> {
        let mut command = Command::new(&spec.command);
        command.kill_on_drop(true);
        let transport = TokioChildProcess::new(command.configure(|cmd| {
            cmd.args(&spec.args);
            for (key, value) in &spec.env {
                cmd.env(key, value);
            }
            if let Some(cwd) = &spec.cwd {
                cmd.current_dir(cwd);
            }
        }))
        .map_err(|source| McpError::Spawn {
            server: spec.name.clone(),
            source,
        })?;

        let client =
            ().serve(transport)
                .await
                .map_err(|error| McpError::Handshake {
                    server: spec.name.clone(),
                    message: error.to_string(),
                })?;

        let remote_tools = client
            .list_all_tools()
            .await
            .map_err(|error| McpError::Handshake {
                server: spec.name.clone(),
                message: error.to_string(),
            })?;

        Ok(Self {
            name: spec.name.clone(),
            remote_tools,
            client: Mutex::new(client),
        })
    }

    pub(crate) async fn call(
        &self,
        request: CallToolRequestParams,
    ) -> Result<CallToolResult, McpError> {
        let client = self.client.lock().await;
        client
            .call_tool(request)
            .await
            .map_err(|error| McpError::Call(error.to_string()))
    }

    fn zone_tools(self: &Arc<Self>) -> Vec<Arc<dyn ZoneTool>> {
        self.remote_tools
            .iter()
            .map(|tool| {
                let qualified = qualified_tool_name(&self.name, tool.name.as_ref());
                let description = tool
                    .description
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .map(|text| format!("[{}] {text}", self.name))
                    .unwrap_or_else(|| format!("[{}] {}", self.name, tool.name));
                let schema = tool.schema_as_json_value();
                let schema = if schema.is_null() {
                    serde_json::json!({"type": "object", "properties": {}})
                } else {
                    schema
                };
                Arc::new(McpTool::new(
                    qualified,
                    tool.name.to_string(),
                    description,
                    schema,
                    Arc::clone(self),
                )) as Arc<dyn ZoneTool>
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::McpServerSpec;
    use crate::tools::{ToolContext, ToolRegistry};
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::{ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Clone, Default)]
    struct Echo;

    #[derive(Debug, Deserialize, schemars::JsonSchema)]
    struct PingArgs {
        message: String,
    }

    #[tool_router]
    impl Echo {
        #[tool(description = "Echo a message back with a pong prefix")]
        fn ping(&self, Parameters(PingArgs { message }): Parameters<PingArgs>) -> String {
            format!("pong:{message}")
        }
    }

    #[tool_handler]
    impl ServerHandler for Echo {}

    #[tokio::test]
    async fn in_process_client_lists_and_calls_tools() {
        let (client_to_server, server_from_client) = tokio::io::duplex(64 * 1024);
        let (server_to_client, client_from_server) = tokio::io::duplex(64 * 1024);

        let server_task = tokio::spawn(async move {
            let server = Echo
                .serve((server_from_client, server_to_client))
                .await
                .expect("server serve");
            let _ = server.waiting().await;
        });

        let client = ().serve((client_from_server, client_to_server)).await.expect("client serve");

        let remote_tools = client.list_all_tools().await.expect("list tools");
        assert!(
            remote_tools.iter().any(|tool| tool.name == "ping"),
            "expected ping tool, got {:?}",
            remote_tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<Vec<_>>()
        );

        let hub = McpHub {
            sessions: vec![Arc::new(McpSession {
                name: "echo".to_string(),
                remote_tools,
                client: Mutex::new(client),
            })],
        };

        let mut registry = ToolRegistry::new();
        assert_eq!(registry.register_mcp(&hub), 1);
        assert!(registry.has_mcp());
        assert!(registry.get("echo_ping").is_some());

        let result = registry
            .execute(
                "echo_ping",
                serde_json::json!({ "message": "hi" }),
                &ToolContext {
                    command_timeout: 5,
                    ..ToolContext::default()
                },
            )
            .await
            .expect("execute echo_ping");
        assert!(result.success, "{result:?}");
        assert!(
            result.output.as_deref().unwrap_or("").contains("pong:hi"),
            "{result:?}"
        );

        // Tools hold the session; drop them so the client tears down and the
        // server `waiting()` future can finish.
        drop(registry);
        drop(hub);
        server_task.abort();
    }

    #[tokio::test]
    async fn connect_skips_missing_binary() {
        let config = McpConfig {
            servers: vec![McpServerSpec {
                name: "missing".to_string(),
                command: "/definitely/not/a/real/mcp-server-xyz".to_string(),
                args: vec![],
                env: HashMap::new(),
                cwd: Some(PathBuf::from("/tmp")),
                disabled: false,
            }],
        };
        let hub = McpHub::connect(&config).await;
        assert!(hub.is_empty());
    }

    #[tokio::test]
    async fn empty_config_yields_empty_hub() {
        let hub = McpHub::connect(&McpConfig::default()).await;
        assert!(hub.is_empty());
        assert!(hub.tools().is_empty());
        let mut registry = ToolRegistry::new();
        assert_eq!(registry.register_mcp(&hub), 0);
        let _ = ToolContext::default();
    }
}
