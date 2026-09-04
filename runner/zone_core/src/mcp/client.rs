//! Live connections to stdio MCP servers.

use futures::future::join_all;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::Mutex;
use tool_runner::Proxy;

use super::config::{McpConfig, McpServerSpec};
use super::tool::{McpTool, unique_qualified_tool_name};
use crate::tools::Tool as ZoneTool;

/// Bound for `initialize` and `tools/list` so one silent child cannot stall
/// agent startup. `kill_on_drop` then tears the process down.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

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
        Self::connect_with_timeout(config, CONNECT_TIMEOUT).await
    }

    async fn connect_with_timeout(config: &McpConfig, limit: Duration) -> Self {
        let results =
            join_all(
                config.servers.iter().cloned().map(|spec| async move {
                    McpSession::connect_with_timeout(&spec, limit).await
                }),
            )
            .await;

        let mut hub = Self::new();
        for result in results {
            match result {
                Ok(session) => {
                    tracing::info!(
                        server = %session.name,
                        tools = session.remote_tools.len(),
                        "Connected MCP server"
                    );
                    hub.sessions.push(Arc::new(session));
                }
                Err(error) => {
                    tracing::warn!(error = %error, "MCP server unavailable");
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
        let mut used = HashSet::new();
        self.tools_avoiding(&mut used)
    }

    /// Same as [`Self::tools`], skipping names already in `used` (built-ins).
    pub fn tools_avoiding(&self, used: &mut HashSet<String>) -> Vec<Arc<dyn ZoneTool>> {
        self.sessions
            .iter()
            .flat_map(|session| session.zone_tools(used))
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
    async fn connect_with_timeout(spec: &McpServerSpec, limit: Duration) -> Result<Self, McpError> {
        match tokio::time::timeout(limit, Self::handshake(spec)).await {
            Ok(result) => result,
            Err(_) => Err(McpError::Handshake {
                server: spec.name.clone(),
                message: "handshake timed out".to_string(),
            }),
        }
    }

    async fn handshake(spec: &McpServerSpec) -> Result<Self, McpError> {
        let mut command = Command::new(&spec.command);
        command.kill_on_drop(true);
        // Inherit the runner environment (PATH, HOME, credentials) and overlay
        // spec.env, then apply the process-level proxy policy.
        // Configured MCP servers are trusted local processes.
        let transport = TokioChildProcess::new(command.configure(|cmd| {
            cmd.args(&spec.args);
            for (key, value) in &spec.env {
                cmd.env(key, value);
            }
            Proxy::from_env().apply(cmd);
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

    fn zone_tools(self: &Arc<Self>, used: &mut HashSet<String>) -> Vec<Arc<dyn ZoneTool>> {
        self.remote_tools
            .iter()
            .map(|tool| {
                let qualified = unique_qualified_tool_name(used, &self.name, tool.name.as_ref());
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

    #[tokio::test]
    async fn proxy_overrides_mcp_environment_before_handshake() {
        const NAME: &str = "mcp::client::tests::proxy_overrides_mcp_environment_before_handshake";
        if std::env::var("ZONE_PROXY_TEST_CHILD").as_deref() != Ok(NAME) {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture"])
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("ZONE_PROXY_TEST_CHILD", NAME)
                .env("TOOL_RUNNER_PROXY_URL", "http://127.0.0.1:28888")
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("environment");
        let spec = McpServerSpec {
            name: "environment".to_string(),
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "env > \"$1\"; exec cat > /dev/null".to_string(),
                "sh".to_string(),
                path.to_string_lossy().into_owned(),
            ],
            env: HashMap::from([
                ("HTTPS_PROXY".to_string(), "http://wrong:8888".to_string()),
                ("http_proxy".to_string(), "http://wrong:8888".to_string()),
                ("NO_PROXY".to_string(), "*".to_string()),
                ("no_proxy".to_string(), "*".to_string()),
                ("TOOL_RUNNER_PROXY_URL".to_string(), "".to_string()),
            ]),
            cwd: None,
            disabled: false,
        };
        let result = McpSession::connect_with_timeout(&spec, Duration::from_millis(500)).await;
        assert!(matches!(result, Err(McpError::Handshake { .. })));
        let output = std::fs::read_to_string(path).unwrap();
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            assert!(
                output
                    .lines()
                    .any(|line| line == format!("{key}=http://127.0.0.1:28888")),
                "{output}"
            );
        }
        assert!(
            !output
                .lines()
                .any(|line| line == "NO_PROXY=*" || line == "no_proxy=*")
        );
        assert!(output.contains("NO_PROXY=localhost,127.0.0.1,::1"));
    }

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

    #[cfg(unix)]
    fn sleepy_spec(name: &str) -> McpServerSpec {
        McpServerSpec {
            name: name.to_string(),
            command: "/bin/sleep".to_string(),
            args: vec!["60".to_string()],
            env: HashMap::new(),
            cwd: None,
            disabled: false,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_times_out_unresponsive_server() {
        let config = McpConfig {
            servers: vec![sleepy_spec("sleepy")],
        };
        let started = std::time::Instant::now();
        let hub = McpHub::connect_with_timeout(&config, Duration::from_millis(400)).await;
        assert!(hub.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "handshake timeout should fail fast, took {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_times_out_unresponsive_servers_in_parallel() {
        let config = McpConfig {
            servers: vec![sleepy_spec("a"), sleepy_spec("b")],
        };
        let started = std::time::Instant::now();
        let hub = McpHub::connect_with_timeout(&config, Duration::from_millis(700)).await;
        assert!(hub.is_empty());
        assert!(
            started.elapsed() < Duration::from_millis(1200),
            "silent servers should share one wall-clock budget, took {:?}",
            started.elapsed()
        );
    }
}
