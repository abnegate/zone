//! What a spawned coding agent may spend, and which tools it may call.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::credential::Credential;
use crate::secret::SecretValue;

/// Thirty minutes is the chat turn's own budget, which is what a coding agent
/// driven as a provider has to fit inside. [`tool_runner`]'s five minutes is
/// the budget for one command; an agent spends its turn running a loop of
/// them, so a turn cut off at that mark would report a timeout on work that
/// was still going well.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1800);

/// A coding agent's stream is structured JSON, not build output, so the cap
/// that matters is far below `tool_runner`'s ten megabytes for arbitrary
/// commands.
pub const DEFAULT_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

/// One event is a JSON object holding at most a turn's worth of text.
pub const DEFAULT_LINE_LIMIT: usize = 1024 * 1024;

/// Whether a spawned agent keeps the tools it ships with.
///
/// The agent runs as the host user and outside [`tool_runner`]'s sandbox, so
/// its own file and shell tools reach every file that user can reach, and
/// nothing zone can see gates them. Zone's tools are gated where they run,
/// which is zone. So a turn gets zone's tools alone unless the operator
/// decides otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuiltinTools {
    #[default]
    Withheld,
    Granted,
}

/// Zone's tools, served to a spawned agent over MCP.
///
/// A coding agent runs its own tool loop and cannot be handed zone's schemas
/// over a completions API, so it reaches them the way it reaches any other MCP
/// server. The calls come back to zone, where the chat's approval policy
/// decides each one before it runs.
#[derive(Debug, Clone)]
pub struct Toolset {
    /// Where zone serves MCP for this turn.
    pub endpoint: String,
    /// The bearer token that authenticates this turn, and only this turn.
    pub token: SecretValue,
    /// Zone's tool names, as zone's registry spells them.
    pub tools: Vec<String>,
}

impl Toolset {
    /// The MCP server name zone's tools arrive under. The agent prefixes every
    /// tool it loads with it, so `read_file` reaches the model as
    /// `mcp__zone__read_file`.
    pub const SERVER: &'static str = "zone";

    /// The child's environment variable holding [`Toolset::token`].
    ///
    /// The server definition names this variable rather than carrying the
    /// token, so the token never reaches an argv that every process on the
    /// host can read. An agent that cannot resolve it fails to connect instead
    /// of reaching zone unauthenticated.
    pub const TOKEN_VARIABLE: &'static str = "ZONE_MCP_TOKEN";

    pub fn new(
        endpoint: impl Into<String>,
        token: impl Into<SecretValue>,
        tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            token: token.into(),
            tools: tools
                .into_iter()
                .map(Into::into)
                .filter(|tool| !tool.trim().is_empty())
                .collect(),
        }
    }
}

/// How a [`super::CliProvider`] runs its agent.
#[derive(Debug, Clone)]
pub struct CliSettings {
    /// Overrides the agent's own executable name. A relative name is resolved
    /// on `PATH` by the operating system.
    pub executable: Option<PathBuf>,
    /// The child's working directory. `None` inherits this process's.
    pub working_directory: Option<PathBuf>,
    pub credential: Credential,
    /// Zone's tools, or `None` for a turn that answers in prose alone.
    ///
    /// Shared rather than owned so that cloning these settings -- which the
    /// backend a client holds does on every turn -- neither copies the token
    /// nor carries a toolset's bulk into every value that names one.
    pub toolset: Option<Arc<Toolset>>,
    pub builtin_tools: BuiltinTools,
    pub timeout: Duration,
    /// Bytes of stdout and stderr kept before the run is abandoned.
    pub output_limit: usize,
    /// Bytes one event may occupy before the stream is treated as malformed.
    pub line_limit: usize,
}

impl Default for CliSettings {
    fn default() -> Self {
        Self {
            executable: None,
            working_directory: None,
            credential: Credential::Inherited,
            toolset: None,
            builtin_tools: BuiltinTools::default(),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
            line_limit: DEFAULT_LINE_LIMIT,
        }
    }
}

impl CliSettings {
    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.executable = Some(executable.into());
        self
    }

    pub fn with_working_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }

    pub fn with_credential(mut self, credential: Credential) -> Self {
        self.credential = credential;
        self
    }

    pub fn with_toolset(mut self, toolset: Toolset) -> Self {
        self.toolset = Some(Arc::new(toolset));
        self
    }

    pub fn with_builtin_tools(mut self, tools: BuiltinTools) -> Self {
        self.builtin_tools = tools;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_output_limit(mut self, limit: usize) -> Self {
        self.output_limit = limit;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolset() -> Toolset {
        Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            ["read_file", "run_command"],
        )
    }

    #[test]
    fn defaults_inherit_the_hosts_session_and_directory() {
        let settings = CliSettings::default();

        assert!(matches!(settings.credential, Credential::Inherited));
        assert!(settings.executable.is_none());
        assert!(settings.working_directory.is_none());
        assert_eq!(settings.timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn a_turn_withholds_the_agents_own_tools_until_an_operator_says_otherwise() {
        let settings = CliSettings::default();

        assert!(settings.toolset.is_none());
        assert_eq!(settings.builtin_tools, BuiltinTools::Withheld);

        let granted = CliSettings::default().with_builtin_tools(BuiltinTools::Granted);
        assert_eq!(granted.builtin_tools, BuiltinTools::Granted);
    }

    #[test]
    fn a_toolset_keeps_only_the_tools_it_was_named() {
        let toolset = Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "token",
            ["read_file", "  ", ""],
        );

        assert_eq!(toolset.tools, ["read_file"]);
        assert_eq!(toolset.endpoint, "http://127.0.0.1:8421/mcp");
        assert_eq!(toolset.token.expose(), "token");
    }

    #[test]
    fn debug_never_prints_the_credential() {
        let settings = CliSettings::default()
            .with_credential(Credential::key("ANTHROPIC_API_KEY", "sk-ant-notarealkey"));

        let rendered = format!("{settings:?}");
        assert!(
            !rendered.contains("sk-ant"),
            "credential leaked: {rendered}"
        );
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn debug_never_prints_the_turn_token() {
        let settings = CliSettings::default().with_toolset(toolset());

        let rendered = format!("{settings:?}");
        assert!(
            !rendered.contains("zone-turn-notarealtoken"),
            "turn token leaked: {rendered}"
        );
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("http://127.0.0.1:8421/mcp"));
    }

    #[test]
    fn an_agents_turn_gets_longer_than_a_single_command_does() {
        let command = Duration::from_millis(tool_runner::executor::DEFAULT_TIMEOUT_MS);

        assert!(
            DEFAULT_TIMEOUT > command,
            "an agent runs a loop of commands, so {DEFAULT_TIMEOUT:?} must exceed one command's {command:?}"
        );
    }

    #[test]
    fn builders_replace_only_what_they_name() {
        let settings = CliSettings::default()
            .with_executable("/opt/bin/claude")
            .with_timeout(Duration::from_secs(30));

        assert_eq!(settings.executable, Some(PathBuf::from("/opt/bin/claude")));
        assert_eq!(settings.timeout, Duration::from_secs(30));
        assert_eq!(settings.output_limit, DEFAULT_OUTPUT_LIMIT);
        assert_eq!(settings.line_limit, DEFAULT_LINE_LIMIT);
        assert!(settings.toolset.is_none());
        assert_eq!(settings.builtin_tools, BuiltinTools::Withheld);
    }
}
