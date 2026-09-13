//! Tool registry and implementations
//!
//! Tools provide the agent's ability to interact with the environment.

mod beneath;
mod command;
mod file;
pub mod job;
mod reason;
mod sanitize;
pub mod tail;
mod tier;

pub use command::*;
pub use file::*;
pub use reason::{REASON_DESCRIPTION, REASON_PARAM, reason_property};
pub use sanitize::sanitize;
pub use tier::{CONFIRMED_FROM, Tier};

use async_trait::async_trait;
use sanitize::sanitize_owned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

use crate::llm::ToolDefinition;

/// Budget a tool spends on output it pages or trims for itself: a `read_file`
/// page, a captured command log.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;

/// Headroom between a tool's own budget and the transcript cap, for the
/// framing a tool wraps around its output: a pagination footer, an exit-code
/// line, the `Error: ` prefix.
const TOOL_FRAMING_CHARS: usize = 1_000;

/// Last-resort cap on tool text stored in the chat transcript.
///
/// Sits above [`MAX_TOOL_OUTPUT_CHARS`] and the framing around it, so a tool
/// that stayed inside its own budget is never cut here. Setting the two equal
/// meant a full page plus its footer overflowed by a few characters and lost
/// its middle to a second trim, which is the double cut this exists to avoid.
pub const MAX_TOOL_MESSAGE_CHARS: usize = MAX_TOOL_OUTPUT_CHARS + TOOL_FRAMING_CHARS;

/// What [`ToolResult::to_message`] puts in front of a failure.
///
/// A tool trimming to a caller's cap has to reserve this, or the message the
/// model reads is longer than the cap it asked for.
pub(crate) const ERROR_PREFIX: &str = "Error: ";

/// Longest a rendered approval preview may run.
///
/// The reader is deciding, not reading. A preview past a screenful is one
/// nobody finishes, and an unfinished preview is worse than none.
pub const MAX_PREVIEW_CHARS: usize = 400;

const TOOL_TRUNCATION_MARKER: &str = "\n[truncated]";

/// Stands in for a line break that has been collapsed away.
///
/// A shell runs one command per line, so two lines joined by a space read as a
/// single command the reader was never shown. The break survives the collapse
/// as something they can see.
pub const LINE_BREAK: &str = " ⏎ ";

/// Collapse `text` onto one line and cut it to `max_chars`.
///
/// A command, a message body or a patch arrives with newlines and runs of
/// whitespace that would push the part worth reading off the card. Runs of
/// blank space within a line go; a line break becomes [`LINE_BREAK`], because
/// what separates two commands is the part of a preview a reader is deciding
/// on.
pub fn excerpt(text: &str, max_chars: usize) -> String {
    let collapsed = text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<&str>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<String>>()
        .join(LINE_BREAK);
    match collapsed.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}…", &collapsed[..byte_idx]),
        None => collapsed,
    }
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}{TOOL_TRUNCATION_MARKER}", &text[..byte_idx]),
        None => text.to_string(),
    }
}

fn trim_marker(dropped: usize) -> String {
    format!("\n\n[… {dropped} characters trimmed …]\n\n")
}

/// Cap `text` at `max_chars`, dropping the middle rather than the tail.
///
/// The error a build was run for is usually at the end, so a cut that keeps
/// only the head throws away the reason for the call. The marker is paid for
/// out of the budget, which makes a second pass over already-trimmed text a
/// no-op.
pub(crate) fn trim_middle(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    // The widest the marker can get, so head + marker + tail always fits.
    let reserved = trim_marker(chars.len()).chars().count();
    let kept = max_chars.saturating_sub(reserved);
    let head = kept / 2;
    let tail = kept - head;
    format!(
        "{}{}{}",
        chars[..head].iter().collect::<String>(),
        trim_marker(chars.len() - kept),
        chars[chars.len() - tail..].iter().collect::<String>()
    )
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
                "{ERROR_PREFIX}{}",
                self.error.as_deref().unwrap_or("Unknown error")
            )
        };
        trim_middle(&message, MAX_TOOL_MESSAGE_CHARS)
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

/// Which conversation or task run a tool call belongs to.
///
/// Background jobs and waits are keyed on it: a job started by one session is
/// unreadable from another, and a detached context can start neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Session {
    Detached,
    Chat(Uuid),
    Task(Uuid),
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
    /// Which chat or task run this tool call belongs to.
    pub session: Session,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            env: std::env::vars().collect(),
            max_file_size: 10 * 1024 * 1024, // 10MB
            command_timeout: 300,            // 5 minutes
            unrestricted: false,
            session: Session::Detached,
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

    /// What a call to this tool costs if it turns out to be the wrong one.
    ///
    /// Batching and confirmation both read this, so a tool declares its
    /// consequences once and every caller agrees about them.
    fn tier(&self) -> Tier {
        Tier::Read
    }

    /// Whether the assistant's turn ends the moment this tool is called.
    ///
    /// The loop stops after it: nothing queued behind it runs, and no further
    /// model round follows, because what comes next is the user's reply rather
    /// than anything the model could say now.
    fn ends_turn(&self) -> bool {
        false
    }

    /// What this specific call will do, for the reader deciding whether to
    /// allow it.
    ///
    /// Rendered from the call's own arguments, so the reader weighs the action
    /// rather than the model's account of it. Tools whose tier is never
    /// confirmed have nobody to render for and leave this alone.
    fn preview(&self, _params: &Value) -> Option<String> {
        None
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
        registry.register(Arc::new(tail::TailJobTool));

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

    /// A named tool's tier, or nothing when the catalog has no such tool.
    pub fn tier(&self, name: &str) -> Option<Tier> {
        self.tools.get(name).map(|tool| tool.tier())
    }

    /// Whether a named tool ends the turn, or nothing when the catalog has no
    /// such tool.
    pub fn ends_turn(&self, name: &str) -> Option<bool> {
        self.tools.get(name).map(|tool| tool.ends_turn())
    }

    /// Whether a named tool mutates state. Unknown names are treated as writes.
    pub fn mutating(&self, name: &str) -> bool {
        self.tier(name).is_none_or(Tier::mutating)
    }

    /// What a named call will do, bounded so one enormous argument cannot turn
    /// an approval card into a wall of text.
    pub fn preview(&self, name: &str, arguments: &str) -> Option<String> {
        let params: Value = serde_json::from_str(arguments).ok()?;
        let rendered = self.tools.get(name)?.preview(&params)?;
        Some(excerpt(&rendered, MAX_PREVIEW_CHARS))
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
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::future::Future;
    use std::io;
    use std::sync::{Arc, Mutex, Once};
    use tracing_subscriber::fmt::MakeWriter;

    /// One subscriber for the whole binary, because a scoped one is not
    /// reliable here: `tracing` caches each callsite's interest globally, and a
    /// test running in parallel with no subscriber of its own caches
    /// `Interest::never` for a callsite another test is about to read.
    static INSTALLED: Once = Once::new();

    thread_local! {
        static SINK: RefCell<Option<Arc<Mutex<Vec<u8>>>>> = const { RefCell::new(None) };
    }

    /// Routes each line to whichever buffer the emitting thread is collecting
    /// into, and drops it when that thread is not collecting.
    struct Sink;

    impl io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            SINK.with(|sink| {
                if let Some(buffer) = sink.borrow().as_ref() {
                    buffer.lock().expect("log buffer").extend_from_slice(bytes);
                }
            });
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for Sink {
        type Writer = Self;

        fn make_writer(&'writer self) -> Self::Writer {
            Sink
        }
    }

    /// One collector at a time.
    ///
    /// The sink is per-thread but the subscriber and `tracing`'s interest
    /// cache are not, and two tests collecting at once have found an empty
    /// buffer. Serialising here rather than at each call site means a test
    /// added later cannot forget to.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Run `work` and return it with everything it logged on this thread.
    ///
    /// `zone_server` turns `zone_core=debug` on by default, so a debug field is
    /// a production log line. This is how a test reads one back.
    pub(crate) async fn captured_logs<T>(work: impl Future<Output = T>) -> (T, String) {
        let _collecting = SERIAL.lock().await;
        INSTALLED.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_ansi(false)
                .with_writer(Sink)
                .try_init();
        });

        let buffer = Arc::new(Mutex::new(Vec::new()));
        SINK.with(|sink| *sink.borrow_mut() = Some(buffer.clone()));
        let value = work.await;
        SINK.with(|sink| sink.borrow_mut().take());

        let logged =
            String::from_utf8(buffer.lock().expect("log buffer").clone()).expect("logs are utf-8");
        (value, logged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A preview a reader approves has to say what will run. `sh -c` runs one
    /// command per line, so two lines joined by a space showed them a single
    /// command that was never going to run, and hid the one that was.
    #[test]
    fn a_multiline_shell_preview_keeps_the_commands_apart() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(RunShellTool));

        let preview = registry
            .preview(
                "run_shell",
                &serde_json::json!({"command": "cat config.toml\nrm -rf /srv/zone"}).to_string(),
            )
            .expect("a shell call previews the line it will run");

        assert!(
            preview.contains(&format!("cat config.toml{LINE_BREAK}rm -rf /srv/zone")),
            "the second command stays a second command: {preview}"
        );
    }

    /// Blank space inside one line is noise that pushes the readable part off
    /// the card, so it still collapses.
    #[test]
    fn blank_space_inside_a_line_still_collapses() {
        assert_eq!(excerpt("cargo    test   --all", 400), "cargo test --all");
        assert_eq!(
            excerpt("  one\n\n\ntwo  ", 400),
            format!("one{LINE_BREAK}two")
        );
    }

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
    fn to_message_keeps_the_head_and_tail_of_huge_success_output() {
        let body = format!("HEAD_MARKER{}TAIL_MARKER", "x".repeat(20_000));
        let message = ToolResult::success(body).to_message();
        assert!(message.starts_with("HEAD_MARKER"), "{message}");
        assert!(message.ends_with("TAIL_MARKER"), "{message}");
        assert!(message.contains("characters trimmed"), "{message}");
        assert!(
            message.chars().count() <= MAX_TOOL_MESSAGE_CHARS,
            "{message}"
        );
    }

    #[test]
    fn to_message_caps_huge_error_on_character_boundary() {
        let result = ToolResult::error("é".repeat(20_000));
        let message = result.to_message();
        assert!(message.starts_with("Error: é"), "{message}");
        assert!(message.ends_with('é'), "{message}");
        assert!(message.contains("characters trimmed"), "{message}");
        assert!(!message.contains('\u{fffd}'), "{message}");
        assert!(
            message.chars().count() <= MAX_TOOL_MESSAGE_CHARS,
            "{message}"
        );
    }

    /// A tool that pages itself already returns the right amount; the
    /// transcript cap is there for one that does not. When the two budgets
    /// were equal, a full `read_file` page plus its pagination footer
    /// overflowed by the length of the footer and lost its middle here, so the
    /// model received the first and last halves of a page with the body gone.
    #[test]
    fn a_full_page_and_its_framing_are_not_cut_a_second_time() {
        let page = "p".repeat(MAX_TOOL_OUTPUT_CHARS);
        let framed =
            format!("{page}\n[truncated; total=99999 offset=0 next={MAX_TOOL_OUTPUT_CHARS}]");
        assert!(
            framed.chars().count() > MAX_TOOL_OUTPUT_CHARS,
            "the framing has to overflow the tool budget for this to be a test"
        );

        let message = ToolResult::success(framed.clone()).to_message();

        assert_eq!(message, framed, "a page that fits its budget was trimmed");
        assert!(!message.contains("characters trimmed"), "{message}");
    }

    #[test]
    fn trim_middle_keeps_head_and_tail_inside_the_budget() {
        let text = format!("HEAD{}TAIL", "x".repeat(40_000));
        let trimmed = trim_middle(&text, MAX_TOOL_MESSAGE_CHARS);
        assert!(trimmed.starts_with("HEAD"), "{trimmed}");
        assert!(trimmed.ends_with("TAIL"), "{trimmed}");
        assert!(trimmed.contains("characters trimmed"), "{trimmed}");
        assert!(trimmed.chars().count() <= MAX_TOOL_MESSAGE_CHARS);
        assert_eq!(trim_middle("hello", MAX_TOOL_MESSAGE_CHARS), "hello");
        assert_eq!(
            trim_middle(&trimmed, MAX_TOOL_MESSAGE_CHARS),
            trimmed,
            "trimming an already trimmed string must not cut it again"
        );
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
        assert!(names.contains(&job::TAIL_JOB));
        assert_eq!(names.len(), 7);
    }

    /// `with_host_tools` is `with_defaults` plus a shell, so one registration
    /// is what puts a background job's log within reach of a chat and a task
    /// run alike, and registering it twice would be redundant.
    #[test]
    fn tail_job_is_registered_once_and_reaches_both_profiles() {
        let defaults = ToolRegistry::with_defaults();
        let host = ToolRegistry::with_host_tools();

        assert!(defaults.get(job::TAIL_JOB).is_some());
        assert!(host.get(job::TAIL_JOB).is_some());

        let mut added: Vec<&str> = host
            .names()
            .into_iter()
            .filter(|name| !defaults.names().contains(name))
            .collect();
        added.sort_unstable();
        assert_eq!(
            added,
            vec!["run_shell"],
            "the host profile adds only a shell"
        );
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

        assert_eq!(definitions.len(), 7);

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

    fn side_effecting_tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(RunCommandTool),
            Arc::new(RunShellTool),
            Arc::new(WriteFileTool),
            Arc::new(ApplyPatchTool),
        ]
    }

    #[test]
    fn every_side_effecting_schema_lists_reason_as_a_property_and_as_required() {
        for tool in side_effecting_tools() {
            let schema = tool.parameters_schema();
            let property = &schema["properties"][REASON_PARAM];
            assert_eq!(property["type"], "string", "{}", tool.name());
            assert_eq!(
                property["description"],
                REASON_DESCRIPTION,
                "{}",
                tool.name()
            );

            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{} has no required array", tool.name()));
            assert!(
                required
                    .iter()
                    .any(|name| name.as_str() == Some(REASON_PARAM)),
                "{} does not require {REASON_PARAM}",
                tool.name()
            );
        }
    }

    #[test]
    fn one_reason_description_is_shared_by_every_side_effecting_schema() {
        let descriptions: HashSet<String> = side_effecting_tools()
            .iter()
            .map(|tool| {
                tool.parameters_schema()["properties"][REASON_PARAM]["description"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{} has no reason description", tool.name()))
                    .to_string()
            })
            .collect();

        assert_eq!(
            descriptions,
            HashSet::from([REASON_DESCRIPTION.to_string()]),
            "reason descriptions have forked"
        );
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
