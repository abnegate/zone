//! Tool registry and implementations
//!
//! Tools provide the agent's ability to interact with the environment.

mod command;
mod file;
mod sanitize;

pub use command::*;
pub use file::*;
pub use sanitize::sanitize;

use async_trait::async_trait;
use sanitize::sanitize_owned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

use crate::llm::ToolDefinition;

/// Last-resort cap on tool text stored in the chat transcript.
pub const MAX_TOOL_MESSAGE_CHARS: usize = 8_000;

const TOOL_TRUNCATION_MARKER: &str = "\n[truncated]";

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}{TOOL_TRUNCATION_MARKER}", &text[..byte_idx]),
        None => text.to_string(),
    }
}

/// Tool execution error
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Tool not found: {0}")]
    NotFound(String),
}

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Whether the tool executed successfully
    pub success: bool,
    /// The output of the tool (for successful execution)
    pub output: Option<String>,
    /// Error message (for failed execution)
    pub error: Option<String>,
    /// Artifact URLs the loop should surface as generated images.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl ToolResult {
    /// Wrap successful tool output, [`sanitize`]d on the way in.
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: Some(sanitize_owned(output.into())),
            error: None,
            images: Vec::new(),
        }
    }

    /// Wrap a tool failure, [`sanitize`]d on the way in.
    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: None,
            error: Some(sanitize_owned(error.into())),
            images: Vec::new(),
        }
    }

    pub fn with_images(mut self, images: Vec<String>) -> Self {
        self.images = images;
        self
    }

    /// Convert to a string for the LLM
    pub fn to_message(&self) -> String {
        let message = if self.success {
            self.output.clone().unwrap_or_default()
        } else {
            format!(
                "Error: {}",
                self.error.as_deref().unwrap_or("Unknown error")
            )
        };
        truncate_chars(&message, MAX_TOOL_MESSAGE_CHARS)
    }
}

const NON_VISION_EXTENSIONS: &[&str] = &[
    ".flac", ".mp3", ".opus", ".wav", ".ogg", ".m4a", ".aac", ".webm", ".mp4", ".mkv", ".mov",
];

/// Whether a tool-produced artifact URL may be handed to a vision model.
///
/// Audio and video artifacts still reach the client, but a model asked to
/// "look at" one either hallucinates or rejects the request outright, so they
/// must never land in an `LlmMessage`'s images.
pub fn is_vision_url(url: &str) -> bool {
    if let Some((header, _)) = url
        .strip_prefix("data:")
        .and_then(|data| data.split_once(','))
    {
        return header
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .starts_with("image/");
    }
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    !NON_VISION_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
}

/// Context passed to tools during execution
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Current working directory
    pub cwd: std::path::PathBuf,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Maximum file size to read (bytes)
    pub max_file_size: usize,
    /// Command timeout (seconds)
    pub command_timeout: u64,
    /// Whether tools may act outside `cwd`.
    ///
    /// Off by default: file tools stay inside the working directory and
    /// `run_command` is held to its allow-list. On, they address the host
    /// directly and paths are taken at face value. Only turn this on where
    /// the caller has asked for it and knows what it means.
    pub unrestricted: bool,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            env: std::env::vars().collect(),
            max_file_size: 10 * 1024 * 1024, // 10MB
            command_timeout: 300,            // 5 minutes
            unrestricted: false,
        }
    }
}

/// A tool that the agent can use
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name
    fn name(&self) -> &str;

    /// Tool description for the LLM
    fn description(&self) -> &str;

    /// JSON Schema for the tool parameters
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given parameters
    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError>;

    /// How long a caller should let this tool run before abandoning it.
    ///
    /// Tools that shell out enforce their own, finer limit; this is the outer
    /// bound a caller applies so that a wedged tool cannot hold a loop open
    /// indefinitely. The default suits tools that query a database.
    fn timeout(&self, _context: &ToolContext) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    /// Whether this tool changes durable state.
    ///
    /// Read-only tools may run together in one batch. A mutating tool keeps
    /// the whole batch sequential so later reads see earlier writes.
    fn mutating(&self) -> bool {
        false
    }

    /// Convert to an OpenAI tool definition
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition::function(self.name(), self.description(), self.parameters_schema())
    }
}

/// Registry of available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// MCP server names whose tools are in this registry (for prompt guidance).
    mcp_servers: Vec<String>,
}

impl ToolRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            mcp_servers: Vec::new(),
        }
    }

    /// Create a registry with all default tools
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // File tools
        registry.register(Arc::new(ReadFileTool));
        registry.register(Arc::new(WriteFileTool));
        registry.register(Arc::new(ApplyPatchTool));
        registry.register(Arc::new(ListFilesTool));
        registry.register(Arc::new(SearchCodeTool));

        // Command tools
        registry.register(Arc::new(RunCommandTool));

        registry
    }

    /// The default tools plus an unrestricted shell.
    ///
    /// Pair this with a [`ToolContext`] that has `unrestricted` set, or the
    /// file tools will still confine themselves to `cwd` while `run_shell`
    /// does not, which is the worst of both.
    pub fn with_host_tools() -> Self {
        let mut registry = Self::with_defaults();
        registry.register(Arc::new(RunShellTool));
        registry
    }

    /// Register a tool
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Get all tool definitions
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions: Vec<ToolDefinition> =
            self.tools.values().map(|t| t.to_definition()).collect();
        // Stable order keeps the tools prefix identical across turns so a
        // local server can reuse its prompt cache.
        definitions.sort_by(|left, right| left.function.name.cmp(&right.function.name));
        definitions
    }

    /// Execute a tool by name
    pub async fn execute(
        &self,
        name: &str,
        params: Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.execute(params, context).await
    }

    /// List all tool names
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Whether a named tool mutates state. Unknown names are treated as writes.
    pub fn mutating(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .map(|tool| tool.mutating())
            .unwrap_or(true)
    }

    /// Take the tools out, for folding one registry into another.
    pub fn into_tools(self) -> Vec<Arc<dyn Tool>> {
        self.tools.into_values().collect()
    }

    /// Attach every tool from a connected MCP hub.
    ///
    /// Names are prefixed with the server name (`magents_spawn_session`).
    /// Collisions after sanitizing are given a numeric suffix so one tool
    /// cannot hide another. The hub can be dropped afterwards: each tool
    /// holds its own session handle.
    pub fn register_mcp(&mut self, hub: &crate::mcp::McpHub) -> usize {
        let mut added = 0;
        for name in hub.server_names() {
            if !self.mcp_servers.contains(&name) {
                self.mcp_servers.push(name);
            }
        }
        let mut used: HashSet<String> = self.tools.keys().cloned().collect();
        for tool in hub.tools_avoiding(&mut used) {
            self.register(tool);
            added += 1;
        }
        added
    }

    /// Whether any MCP server tools are registered.
    pub fn has_mcp(&self) -> bool {
        !self.mcp_servers.is_empty()
    }

    /// Extra system-prompt text for attached MCP tools, if any.
    pub fn mcp_guidance(&self) -> Option<String> {
        if !self.has_mcp() {
            return None;
        }
        let names: Vec<&str> = self.names();
        crate::mcp::guidance_for_tools(&names)
    }
}

/// Default file/shell tools plus any MCP servers configured in the environment.
///
/// Magents is attached automatically when it is on `PATH` and no other MCP
/// config is required. Failures to start a server are logged and skipped.
pub async fn with_defaults_and_mcp() -> ToolRegistry {
    let mut registry = ToolRegistry::with_defaults();
    let hub = crate::mcp::McpHub::connect_from_env().await;
    let added = registry.register_mcp(&hub);
    if added > 0 {
        tracing::info!(
            tools = added,
            servers = hub.server_count(),
            "Attached MCP tools"
        );
    }
    registry
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("Operation completed");
        assert!(result.success);
        assert_eq!(result.output, Some("Operation completed".to_string()));
        assert!(result.error.is_none());
        assert_eq!(result.to_message(), "Operation completed");
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("Something went wrong");
        assert!(!result.success);
        assert!(result.output.is_none());
        assert_eq!(result.error, Some("Something went wrong".to_string()));
        assert_eq!(result.to_message(), "Error: Something went wrong");
    }

    #[test]
    fn success_redacts_a_credential_in_the_output() {
        let result = ToolResult::success("printenv\nGITHUB_TOKEN=ghp_0123456789abcdefghij\n");
        assert_eq!(
            result.output.as_deref(),
            Some("printenv\nGITHUB_TOKEN=[REDACTED]\n")
        );
        assert_eq!(result.to_message(), "printenv\nGITHUB_TOKEN=[REDACTED]\n");
    }

    #[test]
    fn error_strips_terminal_control_sequences() {
        let result = ToolResult::error("\u{1b}]0;stolen title\u{7}command not found\r\n");
        assert_eq!(result.error.as_deref(), Some("command not found\n"));
        assert_eq!(result.to_message(), "Error: command not found\n");
    }

    #[test]
    fn to_message_caps_huge_success_output() {
        let result = ToolResult::success("x".repeat(20_000));
        let message = result.to_message();
        assert!(message.contains("[truncated]"));
        assert!(message.starts_with('x'));
        assert_eq!(
            message.chars().count(),
            MAX_TOOL_MESSAGE_CHARS + TOOL_TRUNCATION_MARKER.chars().count()
        );
        let prefix = message
            .strip_suffix(TOOL_TRUNCATION_MARKER)
            .expect("truncation marker");
        assert_eq!(prefix.chars().count(), MAX_TOOL_MESSAGE_CHARS);
        assert!(prefix.chars().all(|ch| ch == 'x'));
    }

    #[test]
    fn to_message_caps_huge_error_on_character_boundary() {
        let result = ToolResult::error("é".repeat(20_000));
        let message = result.to_message();
        assert!(message.starts_with("Error: "));
        assert!(message.contains("[truncated]"));
        assert_eq!(
            message.chars().count(),
            MAX_TOOL_MESSAGE_CHARS + TOOL_TRUNCATION_MARKER.chars().count()
        );
        let prefix = message
            .strip_suffix(TOOL_TRUNCATION_MARKER)
            .expect("truncation marker");
        assert!(std::str::from_utf8(prefix.as_bytes()).is_ok());
        assert_eq!(prefix.chars().count(), MAX_TOOL_MESSAGE_CHARS);
    }

    #[test]
    fn test_tool_context_default() {
        let context = ToolContext::default();
        assert!(context.cwd.exists() || context.cwd.as_os_str().is_empty());
        assert_eq!(context.max_file_size, 10 * 1024 * 1024);
        assert_eq!(context.command_timeout, 300);
    }

    #[test]
    fn test_tool_registry_new() {
        let registry = ToolRegistry::new();
        assert!(registry.names().is_empty());
    }

    #[test]
    fn test_tool_registry_with_defaults() {
        let registry = ToolRegistry::with_defaults();
        let names = registry.names();

        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"apply_patch"));
        assert!(names.contains(&"list_files"));
        assert!(names.contains(&"search_code"));
        assert!(names.contains(&"run_command"));
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn test_tool_registry_get() {
        let registry = ToolRegistry::with_defaults();

        assert!(registry.get("read_file").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_tool_registry_definitions() {
        let registry = ToolRegistry::with_defaults();
        let definitions = registry.definitions();

        assert_eq!(definitions.len(), 6);

        // All definitions should be function type
        for def in &definitions {
            assert_eq!(def.tool_type, "function");
            assert!(!def.function.name.is_empty());
            assert!(!def.function.description.is_empty());
        }
    }

    #[tokio::test]
    async fn test_tool_registry_execute_not_found() {
        let registry = ToolRegistry::new();
        let context = ToolContext::default();

        let result = registry
            .execute("nonexistent", serde_json::json!({}), &context)
            .await;
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[test]
    fn register_mcp_on_empty_hub_is_noop() {
        let mut registry = ToolRegistry::new();
        let hub = crate::mcp::McpHub::new();
        assert_eq!(registry.register_mcp(&hub), 0);
        assert!(!registry.has_mcp());
        assert!(registry.mcp_guidance().is_none());
    }

    #[test]
    fn vision_urls_exclude_audio_and_video() {
        assert!(is_vision_url("/api/artifacts/w/c/m/a.png"));
        assert!(is_vision_url("https://example.com/a.JPG"));
        assert!(is_vision_url("data:image/png;base64,AAAA"));
        assert!(is_vision_url("/api/artifacts/w/c/m/a.png?v=2"));
        for excluded in [
            "/api/artifacts/w/c/m/a.flac",
            "/api/artifacts/w/c/m/a.MP3",
            "/api/artifacts/w/c/m/a.opus",
            "/api/artifacts/w/c/m/a.wav",
            "/api/artifacts/w/c/m/a.webm",
            "/api/artifacts/w/c/m/a.mp4",
            "/api/artifacts/w/c/m/a.mkv",
            "data:audio/flac;base64,AAAA",
            "data:video/mp4;base64,AAAA",
            "data:,plain",
        ] {
            assert!(!is_vision_url(excluded), "{excluded}");
        }
    }

    #[test]
    fn test_tool_result_serialization() {
        let success = ToolResult::success("done");
        let json = serde_json::to_string(&success).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"output\":\"done\""));

        let error = ToolResult::error("failed");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"error\":\"failed\""));
    }
}
