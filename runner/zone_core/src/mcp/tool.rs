//! MCP tools as [`crate::tools::Tool`] implementations.

use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, JsonObject};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

use super::client::McpSession;
use crate::tools::{Tool, ToolContext, ToolError, ToolResult};

/// One tool advertised by a connected MCP server.
pub struct McpTool {
    qualified_name: String,
    remote_name: String,
    description: String,
    parameters_schema: Value,
    session: Arc<McpSession>,
}

impl McpTool {
    pub(crate) fn new(
        qualified_name: String,
        remote_name: String,
        description: String,
        parameters_schema: Value,
        session: Arc<McpSession>,
    ) -> Self {
        Self {
            qualified_name,
            remote_name,
            description,
            parameters_schema,
            session,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.parameters_schema.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let arguments = json_object(params)?;
        let request =
            CallToolRequestParams::new(self.remote_name.clone()).with_arguments(arguments);

        let call = self.session.call(request);
        let result = match timeout(Duration::from_secs(context.command_timeout), call).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => return Ok(ToolResult::error(error.to_string())),
            Err(_) => {
                return Ok(ToolResult::error(format!(
                    "MCP tool '{}' timed out after {} seconds",
                    self.qualified_name, context.command_timeout
                )));
            }
        };

        let output = format_call_result(&result);
        if result.is_error.unwrap_or(false) {
            Ok(ToolResult::error(output))
        } else {
            Ok(ToolResult::success(output))
        }
    }
}

fn json_object(params: Value) -> Result<JsonObject, ToolError> {
    match params {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(JsonObject::new()),
        other => Err(ToolError::InvalidParams(format!(
            "MCP tool arguments must be a JSON object, got {other}"
        ))),
    }
}

/// `server` + `tool` → a function name safe for OpenAI-style tool calling.
pub fn qualified_tool_name(server: &str, tool: &str) -> String {
    let server = sanitize_ident(server);
    let tool = sanitize_ident(tool);
    if tool == server || tool.starts_with(&format!("{server}_")) {
        tool
    } else {
        format!("{server}_{tool}")
    }
}

fn sanitize_ident(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "tool".to_string()
    } else {
        out
    }
}

/// Flatten an MCP tool result into text the LLM can read.
pub fn format_call_result(result: &CallToolResult) -> String {
    let mut parts = Vec::new();

    if let Some(structured) = &result.structured_content
        && !structured.is_null()
    {
        parts.push(structured.to_string());
    }

    for block in &result.content {
        match block {
            ContentBlock::Text(text) => {
                if !text.text.is_empty() {
                    parts.push(text.text.clone());
                }
            }
            ContentBlock::Image(_) => parts.push("[image]".to_string()),
            ContentBlock::Audio(_) => parts.push("[audio]".to_string()),
            ContentBlock::Resource(resource) => {
                parts.push(format!("[resource {}]", resource_uri(resource)));
            }
            ContentBlock::ResourceLink(link) => {
                parts.push(format!("[resource {}]", link.uri));
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        "(no output)".to_string()
    } else {
        parts.join("\n")
    }
}

fn resource_uri(resource: &rmcp::model::EmbeddedResource) -> String {
    match &resource.resource {
        rmcp::model::ResourceContents::TextResourceContents { uri, .. }
        | rmcp::model::ResourceContents::BlobResourceContents { uri, .. } => uri.clone(),
        _ => "embedded".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ContentBlock;

    #[test]
    fn prefixes_unless_already_namespaced() {
        assert_eq!(
            qualified_tool_name("magents", "spawn_session"),
            "magents_spawn_session"
        );
        assert_eq!(
            qualified_tool_name("magents", "magents_spawn_session"),
            "magents_spawn_session"
        );
        assert_eq!(qualified_tool_name("docs", "docs"), "docs");
    }

    #[test]
    fn sanitizes_odd_characters() {
        assert_eq!(
            qualified_tool_name("my.server", "list/files"),
            "my_server_list_files"
        );
    }

    #[test]
    fn formats_text_and_structured_content() {
        let mut result = CallToolResult::success(vec![ContentBlock::text("hello")]);
        result.structured_content = Some(serde_json::json!({"ok": true}));
        let text = format_call_result(&result);
        assert!(text.contains("hello"));
        assert!(text.contains("ok"));
    }

    #[test]
    fn formats_empty_result() {
        let result = CallToolResult::success(vec![]);
        assert_eq!(format_call_result(&result), "(no output)");
    }
}
