//! Tools the chat agent can call.
//!
//! These are deliberately not `zone_core::tools`: those read and write the
//! filesystem and run shell commands, which is right for a task runner working
//! in a checkout and catastrophic for a multi-tenant API server. Everything
//! here is read-only and scoped to the workspace the chat belongs to, so a tool
//! call can never reach another tenant's data or the manager host.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use zone_core::llm::ToolDefinition;
use zone_core::tools::ToolResult;

use crate::db::{message_embeddings, projects, sources, tasks};
use crate::state::AppState;

/// Hard cap on how much text one tool may feed back into the prompt.
const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;

/// Cap on rows any listing tool returns.
const MAX_TOOL_RESULTS: usize = 25;

/// Default rows when the model does not ask for a specific count.
const DEFAULT_TOOL_RESULTS: usize = 5;

/// Per-call timeout. Tools hit Postgres and the embedding service, so a stall
/// here would otherwise hold the whole agent loop open.
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// Minimum similarity for a chat-history hit to be worth showing.
const CHAT_HISTORY_THRESHOLD: f32 = 0.5;

/// Longest snippet of a single search hit.
const SNIPPET_CHARS: usize = 500;

/// What a tool is allowed to touch during a chat turn.
///
/// The workspace is fixed by the chat being answered, not by anything the model
/// says, which is what keeps tool calls inside the caller's tenant.
#[derive(Clone)]
pub struct ToolContext {
    pub state: AppState,
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
}

/// A tool the chat agent can call.
#[async_trait]
pub trait ChatTool: Send + Sync {
    fn name(&self) -> &'static str;

    fn description(&self) -> &'static str;

    fn parameters_schema(&self) -> Value;

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult;

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(self.name(), self.description(), self.parameters_schema())
    }
}

/// The tools offered for one chat turn.
pub struct ChatToolRegistry {
    tools: Vec<Arc<dyn ChatTool>>,
}

impl ChatToolRegistry {
    /// Build the tool set available given the services this server booted with.
    ///
    /// Search tools are omitted when their backing service is absent rather
    /// than advertised and failed at call time, so the model never burns an
    /// iteration on a tool that cannot work.
    pub fn for_state(state: &AppState) -> Self {
        let mut tools: Vec<Arc<dyn ChatTool>> = Vec::new();

        if state.context_service().is_some() {
            tools.push(Arc::new(SearchKnowledgeTool));
        }
        if state.embedding_service().is_some() {
            tools.push(Arc::new(SearchChatHistoryTool));
        }
        tools.push(Arc::new(ListSourcesTool));
        tools.push(Arc::new(ListProjectsTool));
        tools.push(Arc::new(ListTasksTool));

        Self { tools }
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|t| t.definition()).collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    /// Run a tool by name, converting every failure mode into a `ToolResult`.
    ///
    /// A failed tool is an observation the model can recover from, so nothing
    /// here returns `Err` and aborts the turn.
    pub async fn execute(&self, name: &str, arguments: &str, ctx: &ToolContext) -> ToolResult {
        let Some(tool) = self.tools.iter().find(|t| t.name() == name) else {
            return ToolResult::error(format!(
                "Unknown tool '{}'. Available tools: {}.",
                name,
                self.names().join(", ")
            ));
        };

        // Models routinely emit "" or "null" for a no-argument call.
        let trimmed = arguments.trim();
        let params: Value = if trimmed.is_empty() {
            json!({})
        } else {
            match serde_json::from_str(trimmed) {
                Ok(Value::Null) => json!({}),
                Ok(v) => v,
                Err(e) => {
                    return ToolResult::error(format!("Arguments were not valid JSON: {}", e));
                }
            }
        };

        match tokio::time::timeout(TOOL_TIMEOUT, tool.execute(params, ctx)).await {
            Ok(result) => result,
            Err(_) => ToolResult::error(format!(
                "Tool '{}' timed out after {}s",
                name,
                TOOL_TIMEOUT.as_secs()
            )),
        }
    }
}

/// Read an optional positive integer argument, clamped to what we will serve.
fn limit_arg(params: &Value) -> usize {
    params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(1, MAX_TOOL_RESULTS))
        .unwrap_or(DEFAULT_TOOL_RESULTS)
}

/// Read a required non-empty string argument.
fn string_arg<'a>(params: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    match params.get(key).and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => Ok(s.trim()),
        _ => Err(ToolResult::error(format!(
            "Missing required string argument '{}'",
            key
        ))),
    }
}

fn optional_string_arg<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Truncate on a character boundary, marking that we cut.
fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}…", &text[..byte_idx]),
        None => text.to_string(),
    }
}

/// Collapse whitespace so a chunk of prose costs one line of prompt.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Finish a tool's output: empty results are a success with a clear "nothing
/// found", because an error would push the model to retry pointlessly.
fn render(lines: Vec<String>, empty_message: &str) -> ToolResult {
    if lines.is_empty() {
        return ToolResult::success(empty_message.to_string());
    }
    ToolResult::success(truncate(&lines.join("\n"), MAX_TOOL_OUTPUT_CHARS))
}

// =============================================================================
// Knowledge base search
// =============================================================================

struct SearchKnowledgeTool;

#[async_trait]
impl ChatTool for SearchKnowledgeTool {
    fn name(&self) -> &'static str {
        "search_knowledge"
    }

    fn description(&self) -> &'static str {
        "Search the workspace knowledge base (indexed documents, repositories and other connected \
         sources) for passages relevant to a query. Use this whenever the answer may depend on \
         the user's own content rather than general knowledge."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to look for, phrased as a natural language question or topic."
                },
                "limit": {
                    "type": "integer",
                    "description": "How many passages to return (1-25, default 5).",
                    "minimum": 1,
                    "maximum": MAX_TOOL_RESULTS
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult {
        let query = match string_arg(&params, "query") {
            Ok(q) => q,
            Err(e) => return e,
        };
        let limit = limit_arg(&params);

        let Some(context_service) = ctx.state.context_service() else {
            return ToolResult::error("The knowledge base is not available on this server.");
        };

        let filters = zone_context::embeddings::SearchFilters {
            workspace_id: Some(ctx.workspace_id),
            source_ids: None,
            categories: None,
            min_quality: None,
            since: None,
        };

        let results = match context_service
            .search_hybrid(query, limit, Some(filters), None)
            .await
        {
            Ok(results) => results,
            Err(e) => {
                tracing::warn!(
                    "search_knowledge failed for workspace {}: {}",
                    ctx.workspace_id,
                    e
                );
                return ToolResult::error("The knowledge base search failed.");
            }
        };

        let lines = results
            .iter()
            .map(|r| {
                format!(
                    "[{:.0}% match] {} ({})\n{}",
                    r.similarity * 100.0,
                    r.item_title,
                    r.item_uri,
                    truncate(&one_line(&r.chunk_text), SNIPPET_CHARS)
                )
            })
            .collect();

        render(
            lines,
            "No passages in this workspace's knowledge base matched that query.",
        )
    }
}

// =============================================================================
// Chat history search
// =============================================================================

struct SearchChatHistoryTool;

#[async_trait]
impl ChatTool for SearchChatHistoryTool {
    fn name(&self) -> &'static str {
        "search_chat_history"
    }

    fn description(&self) -> &'static str {
        "Search earlier messages across this workspace's conversations. Use this to recall what \
         was decided or discussed before, especially when the user refers to a past conversation."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to look for in past messages."
                },
                "this_chat_only": {
                    "type": "boolean",
                    "description": "Restrict the search to the current conversation (default false)."
                },
                "limit": {
                    "type": "integer",
                    "description": "How many messages to return (1-25, default 5).",
                    "minimum": 1,
                    "maximum": MAX_TOOL_RESULTS
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult {
        let query = match string_arg(&params, "query") {
            Ok(q) => q,
            Err(e) => return e,
        };
        let limit = limit_arg(&params);
        let this_chat_only = params
            .get("this_chat_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let Some(embedding_service) = ctx.state.embedding_service() else {
            return ToolResult::error("Message search is not available on this server.");
        };

        let embedding = match embedding_service.embed(query).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("search_chat_history embedding failed: {}", e);
                return ToolResult::error("Could not embed the search query.");
            }
        };

        let scope = this_chat_only.then_some(ctx.chat_id);
        let results = match message_embeddings::search_messages(
            ctx.state.db(),
            &embedding,
            ctx.workspace_id,
            scope,
            limit,
            CHAT_HISTORY_THRESHOLD,
        )
        .await
        {
            Ok(results) => results,
            Err(e) => {
                tracing::warn!("search_chat_history query failed: {}", e);
                return ToolResult::error("The message search failed.");
            }
        };

        let lines = results
            .iter()
            .map(|r| {
                format!(
                    "[{:.0}% match] {} on {}: {}",
                    r.similarity * 100.0,
                    r.role,
                    r.created_at.format("%Y-%m-%d"),
                    truncate(&one_line(&r.content), SNIPPET_CHARS)
                )
            })
            .collect();

        render(lines, "No earlier messages matched that query.")
    }
}

// =============================================================================
// Workspace inventory
// =============================================================================

struct ListSourcesTool;

#[async_trait]
impl ChatTool for ListSourcesTool {
    fn name(&self) -> &'static str {
        "list_sources"
    }

    fn description(&self) -> &'static str {
        "List the content sources connected to this workspace (repositories, folders, documents \
         and so on). Use this to find out what material the knowledge base actually covers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "How many sources to return (1-25, default 5).",
                    "minimum": 1,
                    "maximum": MAX_TOOL_RESULTS
                }
            }
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult {
        let limit = limit_arg(&params);

        let rows = match sources::list_sources(
            ctx.state.db(),
            ctx.workspace_id,
            None,
            None,
            limit as i64,
            0,
        )
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("list_sources failed: {}", e);
                return ToolResult::error("Could not list the workspace's sources.");
            }
        };

        let lines = rows
            .iter()
            .map(|s| {
                let state = if s.is_active.unwrap_or(false) {
                    "active"
                } else {
                    "inactive"
                };
                let mut line = format!("{} [{}, {}]", s.name, s.source_type, state);
                if let Some(url) = &s.url {
                    line.push_str(&format!(" {}", url));
                }
                if let Some(description) = &s.description {
                    line.push_str(&format!(" — {}", truncate(&one_line(description), 200)));
                }
                line
            })
            .collect();

        render(lines, "This workspace has no connected sources.")
    }
}

struct ListProjectsTool;

#[async_trait]
impl ChatTool for ListProjectsTool {
    fn name(&self) -> &'static str {
        "list_projects"
    }

    fn description(&self) -> &'static str {
        "List the projects in this workspace, optionally filtered by status."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Only return projects with this status (for example 'active')."
                },
                "limit": {
                    "type": "integer",
                    "description": "How many projects to return (1-25, default 5).",
                    "minimum": 1,
                    "maximum": MAX_TOOL_RESULTS
                }
            }
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult {
        let limit = limit_arg(&params);
        let status = optional_string_arg(&params, "status");

        let rows = match projects::list_projects(ctx.state.db(), ctx.workspace_id, status).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("list_projects failed: {}", e);
                return ToolResult::error("Could not list the workspace's projects.");
            }
        };

        let lines = rows
            .iter()
            .take(limit)
            .map(|p| {
                let mut line = format!("{} [{}]", p.name, p.status);
                if let Some(url) = &p.github_repo_url {
                    line.push_str(&format!(" {}", url));
                }
                if let Some(description) = &p.description {
                    line.push_str(&format!(" — {}", truncate(&one_line(description), 200)));
                }
                line
            })
            .collect();

        render(lines, "This workspace has no projects.")
    }
}

struct ListTasksTool;

#[async_trait]
impl ChatTool for ListTasksTool {
    fn name(&self) -> &'static str {
        "list_tasks"
    }

    fn description(&self) -> &'static str {
        "List the tasks in this workspace, optionally filtered by status. Use this to report on \
         what work is queued, running or finished."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Only return tasks with this status (for example 'pending', 'running', 'completed')."
                },
                "limit": {
                    "type": "integer",
                    "description": "How many tasks to return (1-25, default 5).",
                    "minimum": 1,
                    "maximum": MAX_TOOL_RESULTS
                }
            }
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult {
        let limit = limit_arg(&params);
        let status = optional_string_arg(&params, "status");

        let rows = match tasks::list_tasks(ctx.state.db(), ctx.workspace_id, None, status).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("list_tasks failed: {}", e);
                return ToolResult::error("Could not list the workspace's tasks.");
            }
        };

        let lines = rows
            .iter()
            .take(limit)
            .map(|t| {
                let mut line = format!("{} [{}]", t.title, t.status);
                if let Some(url) = &t.pr_url {
                    line.push_str(&format!(" {}", url));
                }
                if !t.description.trim().is_empty() {
                    line.push_str(&format!(" — {}", truncate(&one_line(&t.description), 200)));
                }
                line
            })
            .collect();

        render(lines, "This workspace has no tasks.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_marks_cut_text() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 3), "hel…");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        // Multi-byte characters must not be split, which byte slicing would do.
        let text = "αβγδε";
        assert_eq!(truncate(text, 3), "αβγ…");
        assert_eq!(truncate(text, 99), text);
    }

    #[test]
    fn one_line_collapses_whitespace() {
        assert_eq!(one_line("a\n\n  b\tc  "), "a b c");
    }

    #[test]
    fn limit_arg_clamps_to_supported_range() {
        assert_eq!(limit_arg(&json!({})), DEFAULT_TOOL_RESULTS);
        assert_eq!(limit_arg(&json!({"limit": 3})), 3);
        assert_eq!(limit_arg(&json!({"limit": 0})), 1);
        assert_eq!(limit_arg(&json!({"limit": 9999})), MAX_TOOL_RESULTS);
        assert_eq!(limit_arg(&json!({"limit": "ten"})), DEFAULT_TOOL_RESULTS);
    }

    #[test]
    fn string_arg_rejects_blank_values() {
        assert_eq!(
            string_arg(&json!({"query": " hi "}), "query").unwrap(),
            "hi"
        );
        assert!(string_arg(&json!({"query": "   "}), "query").is_err());
        assert!(string_arg(&json!({}), "query").is_err());
    }

    #[test]
    fn optional_string_arg_treats_blank_as_absent() {
        assert_eq!(
            optional_string_arg(&json!({"status": "active"}), "status"),
            Some("active")
        );
        assert_eq!(optional_string_arg(&json!({"status": " "}), "status"), None);
        assert_eq!(optional_string_arg(&json!({}), "status"), None);
    }

    #[test]
    fn render_reports_no_results_as_success() {
        let result = render(Vec::new(), "nothing here");
        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("nothing here"));
    }

    #[test]
    fn render_caps_total_output() {
        let lines = vec!["x".repeat(MAX_TOOL_OUTPUT_CHARS * 2)];
        let result = render(lines, "nothing here");
        assert!(result.output.unwrap().chars().count() <= MAX_TOOL_OUTPUT_CHARS + 1);
    }

    #[test]
    fn tool_definitions_are_well_formed() {
        let tools: Vec<Arc<dyn ChatTool>> = vec![
            Arc::new(SearchKnowledgeTool),
            Arc::new(SearchChatHistoryTool),
            Arc::new(ListSourcesTool),
            Arc::new(ListProjectsTool),
            Arc::new(ListTasksTool),
        ];

        for tool in tools {
            let definition = tool.definition();
            assert_eq!(definition.tool_type, "function");
            assert_eq!(definition.function.name, tool.name());
            assert!(!definition.function.description.is_empty());
            assert_eq!(definition.function.parameters["type"], "object");
            assert!(definition.function.parameters.get("properties").is_some());
        }
    }
}
