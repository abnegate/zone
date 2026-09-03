//! Tool registry and implementations
//!
//! Tools provide the agent's ability to interact with the environment.

mod command;
mod file;

pub use command::*;
pub use file::*;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use crate::llm::ToolDefinition;

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
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: Some(output.into()),
            error: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: None,
            error: Some(error.into()),
        }
    }

    /// Convert to a string for the LLM
    pub fn to_message(&self) -> String {
        if self.success {
            self.output.clone().unwrap_or_default()
        } else {
            format!(
                "Error: {}",
                self.error.as_deref().unwrap_or("Unknown error")
            )
        }
    }
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

    /// Convert to an OpenAI tool definition
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition::function(self.name(), self.description(), self.parameters_schema())
    }
}

/// Registry of available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Create a registry with all default tools
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // File tools
        registry.register(Arc::new(ReadFileTool));
        registry.register(Arc::new(WriteFileTool));
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
        self.tools.values().map(|t| t.to_definition()).collect()
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

    /// Take the tools out, for folding one registry into another.
    pub fn into_tools(self) -> Vec<Arc<dyn Tool>> {
        self.tools.into_values().collect()
    }
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
        assert!(names.contains(&"list_files"));
        assert!(names.contains(&"search_code"));
        assert!(names.contains(&"run_command"));
        assert_eq!(names.len(), 5);
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

        assert_eq!(definitions.len(), 5);

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
