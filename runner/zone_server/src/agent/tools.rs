//! The tools a chat agent can call.
//!
//! These implement [`zone_core::tools::Tool`], the same trait the task runner
//! and the CLI use, so there is one tool abstraction in the codebase rather
//! than one per caller. What differs here is scope: each tool carries the
//! workspace and chat it was built for, so a call can never reach another
//! tenant's data no matter what the model puts in the arguments.
//!
//! Every agent chat includes workspace tools and server filesystem and shell tools.
//! File and shell operations run in the server runtime, inside the container
//! for Docker deployments, with the server process permissions.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::receipts::{self, ActionReceipt};
use crate::db::{knowledge, message_embeddings, projects, sources, users};
use crate::state::AppState;

/// Bound legacy search snippets and inventory summaries; full document reads are preserved.
const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;

/// Cap on rows any listing tool returns.
const MAX_TOOL_RESULTS: usize = 25;

/// Default rows when the model does not ask for a specific count.
const DEFAULT_TOOL_RESULTS: usize = 5;

/// Minimum similarity for a chat-history hit to be worth showing.
const CHAT_HISTORY_THRESHOLD: f32 = 0.5;

/// Longest snippet of a single search hit.
const SNIPPET_CHARS: usize = 500;

/// What the workspace tools are allowed to touch.
///
/// Fixed by the chat being answered, not by anything the model says, which is
/// what keeps tool calls inside the caller's tenant. Every workspace tool
/// holds one of these, because `zone_core`'s `ToolContext` describes a working
/// directory and knows nothing about tenants.
#[derive(Clone)]
pub struct WorkspaceScope {
    pub state: AppState,
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
    pub user_id: Uuid,
}

/// Where server tools start when the model gives a relative path.
fn host_root() -> std::path::PathBuf {
    std::env::var_os("ZONE_CHAT_AGENT_CWD")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}

const TASK_SAFE_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "TERM",
    "SHELL",
    "TZ",
    "TMPDIR",
    "XDG_RUNTIME_DIR",
];

fn task_context(cwd: std::path::PathBuf) -> ToolContext {
    ToolContext {
        cwd,
        env: TASK_SAFE_ENV
            .iter()
            .filter_map(|key| std::env::var(key).ok().map(|val| (key.to_string(), val)))
            .collect(),
        max_file_size: 10 * 1024 * 1024,
        command_timeout: 300,
        unrestricted: false,
    }
}

/// Which surface is assembling the shared tool set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProfile {
    Chat,
    Task,
}

/// The tools offered for one turn, and the context they run in.
///
/// Chat and tasks share this builder. Workspace tools stay on chat; tasks
/// get the sandboxed file/shell set plus MCP.
pub struct ChatTools {
    registry: ToolRegistry,
    context: ToolContext,
    scope: Option<WorkspaceScope>,
    workspace: Vec<String>,
    profile: ToolProfile,
}

impl ChatTools {
    /// Build the tool set for a chat.
    ///
    /// Search tools are omitted when their backing service is absent rather
    /// than advertised and failed at call time, so the model never burns an
    /// iteration on a tool that cannot work.
    pub async fn build(scope: WorkspaceScope) -> Self {
        Self::assemble(Some(scope), ToolProfile::Chat, None).await
    }

    /// Sandboxed file/shell tools plus MCP for a background task run.
    pub async fn for_task(state: &AppState, cwd: std::path::PathBuf) -> Self {
        let mut assembled = Self::assemble(None, ToolProfile::Task, Some(cwd)).await;
        let added = assembled.registry.register_mcp(state.mcp_hub().await);
        if added > 0 {
            tracing::info!(tools = added, "Attached MCP tools to task");
        }
        assembled
    }

    async fn assemble(
        scope: Option<WorkspaceScope>,
        profile: ToolProfile,
        task_cwd: Option<std::path::PathBuf>,
    ) -> Self {
        let mut registry = ToolRegistry::new();
        let mut workspace = Vec::new();

        if let Some(scope) = &scope {
            if scope.state.context_service().is_some() {
                registry.register(Arc::new(SearchKnowledgeTool(scope.clone())));
            }
            if scope.state.embedding_service().is_some() {
                registry.register(Arc::new(SearchChatHistoryTool(scope.clone())));
            }
            registry.register(Arc::new(ListSourcesTool(scope.clone())));
            registry.register(Arc::new(ListProjectsTool(scope.clone())));
            super::actions::register(&mut registry, scope);
            super::documents::register(&mut registry, scope);
            super::integrations::register(&mut registry, scope);
            super::images::register(&mut registry, scope);
            super::monitoring::register(&mut registry, scope);
            workspace = registry
                .names()
                .iter()
                .map(|name| name.to_string())
                .collect();
            super::web::register(&mut registry, scope);
        }

        match profile {
            ToolProfile::Chat => {
                for tool in ToolRegistry::with_host_tools().into_tools() {
                    registry.register(tool);
                }
            }
            ToolProfile::Task => {
                for tool in ToolRegistry::with_defaults().into_tools() {
                    registry.register(tool);
                }
            }
        }

        if let Some(scope) = &scope {
            let added = registry.register_mcp(scope.state.mcp_hub().await);
            if added > 0 {
                tracing::info!(tools = added, "Attached MCP tools to chat");
            }
        }

        let context = match profile {
            ToolProfile::Chat => ToolContext {
                cwd: host_root(),
                env: std::env::vars().collect(),
                unrestricted: true,
                ..Default::default()
            },
            ToolProfile::Task => task_context(task_cwd.unwrap_or_else(host_root)),
        };

        Self {
            registry,
            context,
            scope,
            workspace,
            profile,
        }
    }

    pub fn profile(&self) -> ToolProfile {
        self.profile
    }

    pub fn is_empty(&self) -> bool {
        self.registry.names().is_empty()
    }

    pub fn definitions(&self) -> Vec<zone_core::llm::ToolDefinition> {
        self.registry.definitions()
    }

    /// Tool names, sorted so the system prompt is stable between turns.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .registry
            .names()
            .iter()
            .map(|n| n.to_string())
            .collect();
        names.sort();
        names
    }

    pub fn mcp_guidance(&self) -> Option<String> {
        self.registry.mcp_guidance()
    }

    pub fn mutating(&self, name: &str) -> bool {
        self.registry.mutating(name)
    }

    /// Run a tool by name, turning every failure mode into a `ToolResult`.
    ///
    /// A failed tool is an observation the model can recover from, so nothing
    /// here aborts the turn.
    pub async fn execute(&self, name: &str, arguments: &str) -> ToolResult {
        let Some(tool) = self.registry.get(name) else {
            let mut available = self.names();
            available.sort();
            return ToolResult::error(format!(
                "Unknown tool '{}'. Available tools: {}.",
                name,
                available.join(", ")
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

        if self.workspace.iter().any(|registered| registered == name) {
            let Some(scope) = self.scope.as_ref() else {
                return ToolResult::error("Workspace access denied.");
            };
            match sqlx::query_scalar::<_, bool>(
                "SELECT check_workspace_membership($1, $2) AND EXISTS(SELECT 1 FROM chats WHERE id = $3 AND workspace_id = $2)"
            ).bind(scope.user_id).bind(scope.workspace_id).bind(scope.chat_id).fetch_one(scope.state.db()).await
            {
                Ok(true) => {}
                Ok(false) => return ToolResult::error("Workspace access denied."),
                Err(error) => {
                    tracing::warn!(%error, "Workspace authorization failed");
                    return ToolResult::error("Could not verify workspace access.");
                }
            }
        }
        let limit = tool.timeout(&self.context);
        match tokio::time::timeout(limit, tool.execute(params, &self.context)).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => ToolResult::error(e.to_string()),
            Err(_) => ToolResult::error(format!(
                "Tool '{}' timed out after {}s",
                name,
                limit.as_secs()
            )),
        }
    }

    /// Mint a durable receipt after a workspace write. Read tools and host
    /// tools return `None` so they stay in the quiet tool trace.
    pub async fn write_receipt(
        &self,
        id: &str,
        name: &str,
        arguments: &str,
        result: &ToolResult,
    ) -> Option<ActionReceipt> {
        if !receipts::is_write_tool(name) {
            return None;
        }
        let scope = self.scope.as_ref()?;
        let actor_name = match users::get_user_by_id(scope.state.db(), scope.user_id).await {
            Ok(Some(user)) => user
                .display_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(user.email),
            _ => scope.user_id.to_string(),
        };
        receipts::from_write(
            id,
            name,
            arguments,
            result,
            scope.user_id,
            &actor_name,
            chrono::Utc::now(),
        )
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
pub(super) fn string_arg<'a>(params: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    match params.get(key).and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => Ok(s.trim()),
        _ => Err(ToolResult::error(format!(
            "Missing required string argument '{}'",
            key
        ))),
    }
}

pub(super) fn optional_string_arg<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Truncate on a character boundary, marking that we cut.
pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
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

fn match_label(
    semantic: Option<f32>,
    keyword: Option<f32>,
    rrf: Option<f32>,
    fallback: f32,
) -> String {
    if let Some(score) = semantic {
        return format!("{:.0}% semantic", score * 100.0);
    }
    if let Some(score) = keyword {
        return format!("{:.2} keyword", score);
    }
    if rrf.is_some() {
        return format!("{:.3} rrf", fallback);
    }
    format!("{:.0}%", fallback * 100.0)
}

// Knowledge base search

struct SearchKnowledgeTool(WorkspaceScope);

#[async_trait]
impl Tool for SearchKnowledgeTool {
    fn name(&self) -> &str {
        "search_knowledge"
    }

    fn description(&self) -> &str {
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

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(self.run(params).await)
    }
}

impl SearchKnowledgeTool {
    /// The body is written to return a `ToolResult` rather than an error,
    /// because a tool that fails is an observation the model can act on.
    async fn run(&self, params: Value) -> ToolResult {
        let ctx = &self.0;
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

        let mut lines = Vec::new();

        if let Some(embedding_service) = ctx.state.embedding_service() {
            match embedding_service
                .embed(&zone_context::embed_query_text(
                    embedding_service.model(),
                    query,
                ))
                .await
            {
                Ok(query_embedding) => {
                    match knowledge::search_knowledge_entries(
                        ctx.state.db(),
                        &query_embedding,
                        ctx.workspace_id,
                        limit as i64,
                        0.5,
                    )
                    .await
                    {
                        Ok(hits) => {
                            for hit in hits {
                                lines.push(format!(
                                    "[{:.0}% match] {} (knowledge) [entry_id: {}]\n{}",
                                    hit.similarity * 100.0,
                                    hit.title,
                                    hit.entry_id,
                                    truncate(&one_line(&hit.content), SNIPPET_CHARS)
                                ));
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "knowledge entry search failed for workspace {}: {}",
                                ctx.workspace_id,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("knowledge query embed failed: {}", e);
                }
            }
        }

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

        lines.extend(results.iter().map(|r| {
            format!(
                "[{}] {} ({}) [document_id: {}]\n{}",
                match_label(r.semantic_score, r.keyword_score, r.rrf_score, r.similarity),
                r.item_title,
                r.item_uri,
                r.content_item_id,
                truncate(&one_line(&r.chunk_text), SNIPPET_CHARS)
            )
        }));

        render(
            lines,
            "No passages in this workspace's knowledge base matched that query.",
        )
    }
}

// Chat history search

struct SearchChatHistoryTool(WorkspaceScope);

#[async_trait]
impl Tool for SearchChatHistoryTool {
    fn name(&self) -> &str {
        "search_chat_history"
    }

    fn description(&self) -> &str {
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

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(self.run(params).await)
    }
}

impl SearchChatHistoryTool {
    /// The body is written to return a `ToolResult` rather than an error,
    /// because a tool that fails is an observation the model can act on.
    async fn run(&self, params: Value) -> ToolResult {
        let ctx = &self.0;
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

        let embedding = match embedding_service
            .embed(&zone_context::embed_query_text(
                embedding_service.model(),
                query,
            ))
            .await
        {
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

// Workspace inventory

struct ListSourcesTool(WorkspaceScope);

#[async_trait]
impl Tool for ListSourcesTool {
    fn name(&self) -> &str {
        "list_sources"
    }

    fn description(&self) -> &str {
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

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(self.run(params).await)
    }
}

impl ListSourcesTool {
    /// The body is written to return a `ToolResult` rather than an error,
    /// because a tool that fails is an observation the model can act on.
    async fn run(&self, params: Value) -> ToolResult {
        let ctx = &self.0;
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
                let mut line = format!(
                    "{} [{}, {}] [source_id: {}]",
                    s.name, s.source_type, state, s.id
                );
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

struct ListProjectsTool(WorkspaceScope);

#[async_trait]
impl Tool for ListProjectsTool {
    fn name(&self) -> &str {
        "list_projects"
    }

    fn description(&self) -> &str {
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

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(self.run(params).await)
    }
}

impl ListProjectsTool {
    /// The body is written to return a `ToolResult` rather than an error,
    /// because a tool that fails is an observation the model can act on.
    async fn run(&self, params: Value) -> ToolResult {
        let ctx = &self.0;
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

    fn scope() -> WorkspaceScope {
        WorkspaceScope {
            state: AppState::for_tests(),
            user_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
        }
    }

    #[tokio::test]
    async fn tool_definitions_are_well_formed() {
        let scope = scope();
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(SearchKnowledgeTool(scope.clone())),
            Arc::new(SearchChatHistoryTool(scope.clone())),
            Arc::new(ListSourcesTool(scope.clone())),
            Arc::new(ListProjectsTool(scope.clone())),
        ];

        for tool in tools {
            let definition = tool.to_definition();
            assert_eq!(definition.tool_type, "function");
            assert_eq!(definition.function.name, tool.name());
            assert!(!definition.function.description.is_empty());
            assert_eq!(definition.function.parameters["type"], "object");
            assert!(definition.function.parameters.get("properties").is_some());
        }
    }

    #[tokio::test]
    async fn agent_chats_always_get_server_tools() {
        let tools = ChatTools::build(scope()).await;
        assert!(tools.names().contains(&"list_projects".to_string()));
        for name in [
            "run_shell",
            "run_command",
            "write_file",
            "apply_patch",
            "read_file",
            "list_files",
            "search_code",
            "start_task",
            "get_task_run",
            "tail_task_log",
        ] {
            assert!(
                tools.names().contains(&name.to_string()),
                "{name} must be available"
            );
        }
        assert!(tools.context.unrestricted);
        assert_eq!(tools.profile(), ToolProfile::Chat);
        assert!(!tools.mutating("list_projects"));
        assert!(!tools.mutating("read_file"));
        assert!(tools.mutating("write_file"));
        assert!(tools.mutating("apply_patch"));
        assert!(tools.mutating("start_task"));
        assert!(!tools.mutating("get_task_run"));
        let required = crate::agent::system_prompt(&tools, false);
        assert!(required.contains("wait for the user to approve"));
        let auto = crate::agent::system_prompt(&tools, true);
        assert!(auto.contains("without waiting for confirmation"));
        assert!(!auto.contains("wait for the user to approve"));
    }

    #[tokio::test]
    async fn task_tools_are_sandboxed_and_omit_workspace_catalog() {
        let tools = ChatTools::for_task(&AppState::for_tests(), std::env::temp_dir()).await;
        assert_eq!(tools.profile(), ToolProfile::Task);
        assert!(!tools.context.unrestricted);
        assert!(tools.names().contains(&"read_file".to_string()));
        assert!(tools.names().contains(&"apply_patch".to_string()));
        assert!(!tools.names().contains(&"run_shell".to_string()));
        assert!(!tools.names().contains(&"list_projects".to_string()));
        assert!(!tools.names().contains(&"start_task".to_string()));
        assert!(!tools.names().contains(&"generate_image".to_string()));
        assert!(!tools.names().contains(&"query_prometheus".to_string()));
    }

    #[tokio::test]
    async fn an_unknown_tool_lists_what_is_available() {
        let tools = ChatTools::build(scope()).await;
        let result = tools.execute("definitely_not_a_tool", "{}").await;

        assert!(!result.success);
        let error = result.error.unwrap();
        assert!(error.contains("Unknown tool"), "{error}");
        assert!(error.contains("list_projects"), "{error}");
    }

    #[tokio::test]
    async fn malformed_arguments_come_back_as_a_readable_failure() {
        let tools = ChatTools::build(scope()).await;
        let result = tools.execute("list_projects", "{not json").await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("not valid JSON"));
    }

    #[tokio::test]
    async fn server_shell_runs_for_an_agent_chat() {
        let tools = ChatTools::build(scope()).await;
        let result = tools
            .execute("run_shell", r#"{"command":"echo agentic"}"#)
            .await;

        assert!(result.success, "{:?}", result.error);
        assert!(result.output.unwrap().contains("agentic"));
    }

    #[tokio::test]
    async fn server_files_can_be_written_and_read() {
        let tools = ChatTools::build(scope()).await;
        let path = std::env::temp_dir().join(format!("zone-agent-{}.txt", Uuid::new_v4()));
        let written = tools
            .execute(
                "write_file",
                &json!({"path": path, "content": "agent file round trip"}).to_string(),
            )
            .await;
        let read = tools
            .execute("read_file", &json!({"path": path}).to_string())
            .await;
        let contents = std::fs::read_to_string(&path);
        let cleanup = std::fs::remove_file(&path);
        assert!(written.success, "{:?}", written.error);
        assert!(read.success, "{:?}", read.error);
        assert!(read.output.unwrap().contains("agent file round trip"));
        assert_eq!(contents.unwrap(), "agent file round trip");
        cleanup.unwrap();
    }

    #[tokio::test]
    async fn apply_patch_edits_without_rewriting_the_file() {
        let tools = ChatTools::build(scope()).await;
        let path = std::env::temp_dir().join(format!("zone-patch-{}.txt", Uuid::new_v4()));
        std::fs::write(&path, "alpha\nkeep\n").unwrap();
        let patched = tools
            .execute(
                "apply_patch",
                &json!({"path": path, "old_string": "alpha", "new_string": "beta"}).to_string(),
            )
            .await;
        let contents = std::fs::read_to_string(&path);
        let cleanup = std::fs::remove_file(&path);
        assert!(patched.success, "{:?}", patched.error);
        assert_eq!(contents.unwrap(), "beta\nkeep\n");
        cleanup.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
    async fn workspace_tools_recheck_actor_and_chat_scope() {
        use crate::db::{organizations, users, workspace_members, workspaces};
        use crate::state::test_config;
        let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
            .await
            .unwrap();
        let user = users::create_user(
            &pool,
            &format!("{}@example.com", Uuid::new_v4()),
            "hash",
            None,
            false,
        )
        .await
        .unwrap();
        let organization = organizations::create_organization(
            &pool,
            "Scope tests",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Scope tests",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        workspace_members::add_member(
            &pool,
            workspace.id,
            user.id,
            workspace_members::WorkspaceRole::Viewer,
            None,
        )
        .await
        .unwrap();
        let chat: Uuid = sqlx::query_scalar("INSERT INTO chats (workspace_id, title, model_name) VALUES ($1, 'Scope tests', 'test') RETURNING id").bind(workspace.id).fetch_one(&pool).await.unwrap();
        let state = AppState::new(test_config(), pool.clone(), None);
        state.disable_mcp();
        let scope = WorkspaceScope {
            state,
            workspace_id: workspace.id,
            chat_id: chat,
            user_id: user.id,
        };
        let tools = ChatTools::build(scope.clone()).await;
        for name in [
            "list_projects",
            "list_sources",
            "list_tasks",
            "list_documents",
            "list_members",
            "list_chats",
            "list_reminders",
        ] {
            let result = tools.execute(name, "{}").await;
            assert!(result.success, "{name}: {:?}", result.error);
        }
        let denied = tools.execute("create_document", &json!({"title":"Denied", "content":"Denied", "user_id":Uuid::new_v4(), "workspace_id":workspace.id}).to_string()).await;
        assert!(!denied.success);
        let invalid = ChatTools::build(WorkspaceScope {
            chat_id: Uuid::new_v4(),
            ..scope
        })
        .await;
        assert!(!invalid.execute("list_sources", "{}").await.success);
        workspace_members::remove_member(&pool, workspace.id, user.id)
            .await
            .unwrap();
        for name in [
            "list_projects",
            "list_sources",
            "list_tasks",
            "list_documents",
            "list_members",
            "list_chats",
            "list_reminders",
            "get_build_status",
        ] {
            let result = tools.execute(name, "{}").await;
            assert!(!result.success, "{name} must deny revoked membership");
            assert_eq!(result.error.as_deref(), Some("Workspace access denied."));
        }
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
