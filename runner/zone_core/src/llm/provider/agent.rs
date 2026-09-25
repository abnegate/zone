//! The coding agent CLIs this crate knows how to drive.

use std::fmt;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::event::AgentEvent;
use super::parser;
use super::settings::{BuiltinTools, CodexSandbox, DEFAULT_TIMEOUT, Toolset};

const MODEL: &str = "--model";
const SETTING_SOURCES: &str = "--setting-sources";
const NO_SETTING_SOURCES: &str = "";
const SETTINGS: &str = "--settings";
const CROSS_SESSION_REFUSED: &str = r#"{"crossSessionInbound":"refuse"}"#;

const CLAUDE_ISOLATION: &[&str] = &[
    SETTING_SOURCES,
    NO_SETTING_SOURCES,
    SETTINGS,
    CROSS_SESSION_REFUSED,
];

/// Zone's own word for letting the model be chosen. An agent would send it to
/// its API as a model name, and the turn would fail there.
const AUTO: &str = "auto";

/// Separates a local model's tag, as in `gpt-oss:20b`. No agent model has one.
const TAG_SEPARATOR: char = ':';

const CLAUDE_MODELS: &[&str] = &["sonnet", "opus", "haiku", "fable"];
const CLAUDE_NAMED_ONLY: &[&str] = &["fable"];

/// Claude's names for its latest models, in any case. `default` is left out:
/// it means what passing no `--model` means.
const CLAUDE_ALIASES: &[&str] = &[
    "sonnet",
    "opus",
    "haiku",
    "fable",
    "best",
    "sonnet[1m]",
    "opus[1m]",
    "fable[1m]",
    "opusplan",
];
const CLAUDE_FAMILY: &str = "claude-";
const LONG_CONTEXT: &str = "[1m]";

const CODEX_MODELS: &[&str] = &[
    "gpt-6-astra",
    "gpt-6-sol",
    "gpt-6-luna",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
];
const CODEX_FAMILY: &str = "gpt-";

/// Both timeouts are [`super::DEFAULT_TIMEOUT`] in milliseconds, because a call
/// to zone's tools can wait that long on an approval.
const CLAUDE_DEFAULTS: &[(&str, &str)] = &[
    ("DISABLE_AUTOUPDATER", "1"),
    ("MCP_TOOL_TIMEOUT", "1800000"),
    ("CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT", "1800000"),
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
];

const STRICT_MCP_CONFIG: &str = "--strict-mcp-config";
const MCP_CONFIG: &str = "--mcp-config";
const ALLOWED_TOOLS: &str = "--allowedTools";
const BUILTIN_TOOLS: &str = "--tools";

const IGNORE_USER_CONFIG: &str = "--ignore-user-config";
const DISABLE: &str = "--disable";
const CONFIG: &str = "-c";
const SANDBOX: &str = "--sandbox";
const READ_ONLY: &str = "read-only";

/// Codex features that bring tools from outside both zone and the host:
/// ChatGPT connectors and plugins.
const EXTERNAL_TOOL_FEATURES: [&str; 2] = ["apps", "plugins"];

const BUILTIN_TOOL_FEATURES: [&str; 5] = [
    "shell_tool",
    "view_image",
    "goals",
    "sleep_tool",
    "image_generation",
];

/// Built-in codex tools that no feature flag names, as TOML settings.
const BUILTIN_TOOL_SETTINGS: [&str; 3] = [
    "agents.enabled=false",
    r#"web_search="disabled""#,
    "tools.experimental_request_user_input.enabled=false",
];

const APPROVE: &str = "approve";

/// Zone's tools are listed to the model up front rather than behind a search.
const DEFERRED_EXPOSURE: &str = "deferred";

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// How a prompt reaches the child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Written to the child's stdin, which is then closed.
    Stdin,
    /// Appended to the argument list.
    Argument,
}

/// A coding agent CLI.
///
/// Every agent here takes its prompt on stdin. `argv` has a hard size limit
/// that a conversation reaches long before a context window does, and the
/// failure mode when it does is `E2BIG` from `execve` rather than anything the
/// agent can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    /// Every agent this crate drives.
    ///
    /// A caller that must act on all of them -- finding one by name, for one --
    /// reads this instead of repeating the list and going stale when an agent
    /// is added.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The command looked up on `PATH` unless the settings override it.
    pub fn executable(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn delivery(self) -> Delivery {
        match self {
            Self::Claude | Self::Codex => Delivery::Stdin,
        }
    }

    /// The agent whose [`AgentKind::as_str`] is `name`.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.as_str() == name)
    }

    /// The models offered for this agent, in the order the agent ranks them.
    pub fn models(self) -> &'static [&'static str] {
        match self {
            Self::Claude => CLAUDE_MODELS,
            Self::Codex => CODEX_MODELS,
        }
    }

    /// The offered models Zone runs only when a person names one.
    pub fn named_only(self) -> &'static [&'static str] {
        match self {
            Self::Claude => CLAUDE_NAMED_ONLY,
            Self::Codex => &[],
        }
    }

    /// Whether this agent can run `model`.
    ///
    /// An agent sends a name it does not recognise to its API unchanged, and
    /// the turn fails there, so only a name this accepts is ever passed on.
    pub fn knows(self, model: &str) -> bool {
        let model = model.trim();
        if model.is_empty()
            || model.eq_ignore_ascii_case(AUTO)
            || model.contains(TAG_SEPARATOR)
            || model.contains(char::is_whitespace)
        {
            return false;
        }

        match self {
            Self::Claude => {
                CLAUDE_ALIASES.contains(&model.to_ascii_lowercase().as_str()) || claude_model(model)
            }
            Self::Codex => CODEX_MODELS.contains(&model) || codex_model(model),
        }
    }

    /// The variable naming the directory this agent keeps its settings,
    /// sessions and login in.
    pub fn home(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CONFIG_DIR",
            Self::Codex => "CODEX_HOME",
        }
    }

    /// The variable this agent reads a subscription login's token from. Codex
    /// has none: its login is a file in its home.
    pub fn token(self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("CLAUDE_CODE_OAUTH_TOKEN"),
            Self::Codex => None,
        }
    }

    /// The variables every turn of this agent runs with.
    pub fn defaults(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Claude => CLAUDE_DEFAULTS,
            Self::Codex => &[],
        }
    }

    /// A non-interactive invocation that streams newline-delimited JSON, told
    /// which tools the turn may call.
    ///
    /// Zone's tools arrive over MCP because an agent runs its own loop and
    /// cannot be handed tool schemas over a completions API. Each agent lets
    /// every call to them through -- claude's `--allowedTools`, codex's
    /// `default_tools_approval_mode` -- and zone decides each call where it
    /// runs, under the chat's approval policy.
    ///
    /// Nothing else is approved in advance, and no agent is passed a
    /// permission-bypass flag, so claude's own tools keep claude's own
    /// permission checks. Codex's do not: `codex exec` never asks for
    /// approval, so only its sandbox confines them -- `read-only` while they
    /// are withheld, and whichever sandbox a turn that grants them is given,
    /// of which `danger-full-access` confines nothing.
    pub fn arguments_with(
        self,
        model: Option<&str>,
        toolset: Option<&Toolset>,
        builtin_tools: BuiltinTools,
        sandbox: CodexSandbox,
    ) -> Vec<String> {
        let mut arguments: Vec<String> = match self {
            Self::Claude => ["--verbose", "--output-format", "stream-json"],
            Self::Codex => ["exec", "--json", "--skip-git-repo-check"],
        }
        .iter()
        .map(|argument| (*argument).to_string())
        .collect();

        match model.map(str::trim).filter(|model| !model.is_empty()) {
            Some(model) if self.knows(model) => {
                arguments.extend([MODEL.to_string(), model.to_string()]);
            }
            Some(model) => {
                tracing::debug!(
                    agent = %self,
                    model,
                    "the agent does not know this model, so it chooses its own"
                );
            }
            None => {}
        }

        arguments.extend(
            match self {
                Self::Claude => CLAUDE_ISOLATION,
                Self::Codex => &[],
            }
            .iter()
            .map(|argument| (*argument).to_string()),
        );

        arguments.extend(match self {
            Self::Claude => claude_tool_arguments(toolset, builtin_tools),
            Self::Codex => codex_tool_arguments(toolset, builtin_tools, sandbox),
        });

        arguments.push(
            match self {
                Self::Claude => "--print",
                Self::Codex => "-",
            }
            .to_string(),
        );

        arguments
    }

    /// Translate one output line, appending whatever it means.
    ///
    /// A line this agent has no opinion about appends nothing rather than
    /// failing: agents add event types between releases, and a stream that
    /// aborted on the first unrecognised line would lose the whole answer.
    pub fn interpret(self, line: &str, events: &mut Vec<AgentEvent>) {
        match self {
            Self::Claude => parser::claude::interpret(line, events),
            Self::Codex => parser::codex::interpret(line, events),
        }
    }
}

/// Claude's flags naming a turn's tools.
fn claude_tool_arguments(toolset: Option<&Toolset>, builtin_tools: BuiltinTools) -> Vec<String> {
    let withheld = builtin_tools == BuiltinTools::Withheld;
    if toolset.is_none() && !withheld {
        return Vec::new();
    }

    // Without this the turn also loads whatever MCP servers the host user
    // configured for themselves, which no chat message asked for.
    let mut arguments = vec![STRICT_MCP_CONFIG.to_string()];

    if let Some(toolset) = toolset {
        arguments.push(MCP_CONFIG.to_string());
        arguments.push(server_definition(toolset).to_string());

        let allowed = allowed_tools(toolset);
        if !allowed.is_empty() {
            arguments.push(ALLOWED_TOOLS.to_string());
            arguments.push(allowed);
        }
    }

    if withheld {
        // An empty list is how the agent is told to bring none of its own
        // tools. What MCP serves it is untouched by it.
        arguments.push(BUILTIN_TOOLS.to_string());
        arguments.push(String::new());
    }

    arguments
}

/// Zone's MCP server, as the agent's `--mcp-config` expects to read it.
///
/// The token is named, not spelled: an agent expands `${VARIABLE}` out of its
/// own environment, and a turn whose token sat in `argv` would be readable by
/// every process on the host for as long as the turn ran.
fn server_definition(toolset: &Toolset) -> Value {
    let mut servers = Map::new();
    servers.insert(
        Toolset::SERVER.to_string(),
        json!({
            "type": "http",
            "url": toolset.endpoint,
            "headers": {
                "Authorization": format!("Bearer ${{{}}}", Toolset::TOKEN_VARIABLE),
            },
        }),
    );

    json!({ "mcpServers": servers })
}

fn allowed_tools(toolset: &Toolset) -> String {
    toolset
        .tools
        .iter()
        .map(|tool| Toolset::qualified(Toolset::SERVER, tool))
        .collect::<Vec<String>>()
        .join(",")
}

/// Codex's flags naming a turn's tools.
fn codex_tool_arguments(
    toolset: Option<&Toolset>,
    builtin_tools: BuiltinTools,
    sandbox: CodexSandbox,
) -> Vec<String> {
    let withheld = builtin_tools == BuiltinTools::Withheld;
    let mut arguments = Vec::new();

    if toolset.is_some() || withheld {
        arguments.push(IGNORE_USER_CONFIG.to_string());
        arguments.extend(repeated(DISABLE, EXTERNAL_TOOL_FEATURES));
    }

    if withheld {
        arguments.extend(repeated(DISABLE, BUILTIN_TOOL_FEATURES));
        arguments.extend(repeated(CONFIG, BUILTIN_TOOL_SETTINGS));
    }

    arguments.push(SANDBOX.to_string());
    arguments.push(
        match builtin_tools {
            BuiltinTools::Withheld => READ_ONLY,
            BuiltinTools::Granted => sandbox.as_str(),
        }
        .to_string(),
    );

    if let Some(toolset) = toolset {
        arguments.extend(repeated(CONFIG, server_settings(toolset)));
    }

    arguments
}

/// `flag value` for each value, which is how codex takes a repeated flag.
fn repeated(flag: &str, values: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    values
        .into_iter()
        .flat_map(|value| [flag.to_string(), value.into()])
        .collect()
}

/// Zone's MCP server as codex's `-c` settings, whose values are TOML. The
/// token is named, not spelled, for the reason [`server_definition`] gives.
fn server_settings(toolset: &Toolset) -> Vec<String> {
    let server = format!("mcp_servers.{}", Toolset::SERVER);
    [
        ("url", toml_string(&toolset.endpoint)),
        ("bearer_token_env_var", toml_string(Toolset::TOKEN_VARIABLE)),
        ("enabled_tools", toml_array(&toolset.tools)),
        ("tool_timeout_sec", DEFAULT_TIMEOUT.as_secs().to_string()),
        ("startup_timeout_sec", STARTUP_TIMEOUT.as_secs().to_string()),
        ("default_tools_approval_mode", toml_string(APPROVE)),
        ("required", true.to_string()),
        ("omit_tools_from", toml_array(&[DEFERRED_EXPOSURE])),
    ]
    .into_iter()
    .map(|(key, value)| format!("{server}.{key}={value}"))
    .collect()
}

/// A TOML basic string. Codex reads a `-c` value it cannot parse as a literal
/// string instead of refusing it, so a quoting mistake would pass silently.
fn toml_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str(r#"\""#),
            '\\' => quoted.push_str(r"\\"),
            '\n' => quoted.push_str(r"\n"),
            '\r' => quoted.push_str(r"\r"),
            '\t' => quoted.push_str(r"\t"),
            control if control.is_control() => {
                quoted.push_str(&format!(r"\u{:04X}", u32::from(control)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn toml_array(values: &[impl AsRef<str>]) -> String {
    let items: Vec<String> = values
        .iter()
        .map(|value| toml_string(value.as_ref()))
        .collect();
    format!("[{}]", items.join(","))
}

impl fmt::Display for AgentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A full claude model name: `^claude-[a-z0-9-]+(\[1m\])?$`.
fn claude_model(model: &str) -> bool {
    model
        .strip_suffix(LONG_CONTEXT)
        .unwrap_or(model)
        .strip_prefix(CLAUDE_FAMILY)
        .is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|byte| lowercase_or_digit(byte) || byte == b'-')
        })
}

/// A model of codex's family: `^gpt-[a-z0-9][a-z0-9.-]*$`.
fn codex_model(model: &str) -> bool {
    let Some(rest) = model.strip_prefix(CODEX_FAMILY) else {
        return false;
    };
    let mut bytes = rest.bytes();

    bytes.next().is_some_and(lowercase_or_digit)
        && bytes.all(|byte| lowercase_or_digit(byte) || byte == b'.' || byte == b'-')
}

fn lowercase_or_digit(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    impl AgentKind {
        /// A turn run with the agent's own tools and none of zone's.
        fn arguments(self, model: Option<&str>) -> Vec<String> {
            self.arguments_with(model, None, BuiltinTools::Granted, CodexSandbox::default())
        }
    }

    #[test]
    fn the_prompt_is_never_placed_on_the_command_line() {
        for agent in [AgentKind::Claude, AgentKind::Codex] {
            assert_eq!(agent.delivery(), Delivery::Stdin, "{agent}");
        }
    }

    #[test]
    fn every_agent_is_listed() {
        assert_eq!(AgentKind::ALL, [AgentKind::Claude, AgentKind::Codex]);
    }

    #[test]
    fn claude_streams_json_and_reads_its_prompt_from_stdin() {
        let arguments = AgentKind::Claude.arguments(Some("opus"));

        assert_eq!(
            arguments,
            [
                "--verbose",
                "--output-format",
                "stream-json",
                "--model",
                "opus",
                "--setting-sources",
                "",
                "--settings",
                r#"{"crossSessionInbound":"refuse"}"#,
                "--print",
            ]
        );
    }

    #[test]
    fn codex_streams_json_and_reads_its_prompt_from_stdin() {
        let arguments = AgentKind::Codex.arguments(Some("gpt-6-sol"));

        assert_eq!(
            arguments,
            [
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--model",
                "gpt-6-sol",
                "--sandbox",
                "workspace-write",
                "-"
            ]
        );
    }

    #[test]
    fn an_absent_or_empty_model_is_left_to_the_agent() {
        for model in [None, Some(""), Some("   ")] {
            let arguments = AgentKind::Claude.arguments(model);
            assert!(
                !arguments.contains(&"--model".to_string()),
                "sent --model for {model:?}"
            );
        }
    }

    #[test]
    fn a_model_the_agent_does_not_know_is_left_to_the_agent() {
        for (agent, model) in [
            (AgentKind::Claude, "llama3.2:3b"),
            (AgentKind::Claude, "auto"),
            (AgentKind::Claude, "gpt-6-sol"),
            (AgentKind::Claude, "default"),
            (AgentKind::Codex, "gpt-oss:20b"),
            (AgentKind::Codex, "AUTO"),
            (AgentKind::Codex, "o3"),
            (AgentKind::Codex, "opus"),
        ] {
            let arguments = agent.arguments(Some(model));
            assert!(
                !arguments.contains(&MODEL.to_string()),
                "{agent} was handed a model it does not know, {model}: {arguments:?}"
            );
        }
    }

    #[test]
    fn a_model_the_agent_knows_is_passed_on_trimmed() {
        for (agent, model) in [
            (AgentKind::Claude, "opus"),
            (AgentKind::Claude, " claude-opus-4-8 "),
            (AgentKind::Codex, "gpt-6-sol"),
            (AgentKind::Codex, "gpt-5.5\n"),
        ] {
            let arguments = agent.arguments(Some(model));
            assert_eq!(
                value_after(&arguments, MODEL),
                Some(model.trim()),
                "{agent}: {arguments:?}"
            );
        }
    }

    #[test]
    fn no_agent_knows_a_local_model_zones_own_auto_or_a_name_with_spaces() {
        for agent in AgentKind::ALL {
            for model in [
                "gpt-oss:20b",
                "llama3.2:3b",
                "",
                "   ",
                "auto",
                "AUTO",
                "Auto",
                "gpt-6 sol",
                "claude-opus 5",
                "son net",
                "opus\tplan",
            ] {
                assert!(!agent.knows(model), "{agent} claims to know {model:?}");
            }
        }
    }

    #[test]
    fn every_model_an_agent_offers_is_one_it_knows() {
        for agent in AgentKind::ALL {
            assert!(!agent.models().is_empty(), "{agent} offers no models");
            for model in agent.models() {
                assert!(
                    agent.knows(model),
                    "{agent} offers {model} without knowing it"
                );
            }
        }
    }

    #[test]
    fn each_agent_offers_its_models_in_the_order_it_ranks_them() {
        assert_eq!(
            AgentKind::Claude.models(),
            ["sonnet", "opus", "haiku", "fable"]
        );
        assert_eq!(
            AgentKind::Codex.models(),
            [
                "gpt-6-astra",
                "gpt-6-sol",
                "gpt-6-luna",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "gpt-5.5",
            ]
        );
    }

    #[test]
    fn only_claudes_fable_runs_only_when_named_and_is_still_offered() {
        assert_eq!(AgentKind::Claude.named_only(), ["fable"]);
        assert!(AgentKind::Codex.named_only().is_empty());

        for agent in AgentKind::ALL {
            for model in agent.named_only() {
                assert!(
                    agent.models().contains(model),
                    "{agent} runs {model} only when named without offering it"
                );
            }
        }
    }

    #[test]
    fn claude_knows_its_aliases_in_any_case_and_its_full_model_names() {
        for model in [
            "sonnet",
            "opus",
            "haiku",
            "fable",
            "best",
            "sonnet[1m]",
            "opus[1m]",
            "fable[1m]",
            "opusplan",
            "Opus",
            "SONNET",
            "Opus[1M]",
            " haiku ",
            "claude-opus-4-8",
            "claude-sonnet-4-6[1m]",
            "claude-haiku-4-5-20251001",
            "claude-opus-6",
        ] {
            assert!(AgentKind::Claude.knows(model), "{model:?}");
        }

        for model in [
            "default",
            "Default",
            "Claude-Opus-5",
            "claude-",
            "claude-[1m]",
            "claude-opus-5[2m]",
            "claude_opus",
            "us.anthropic.claude-opus-5",
            "claude-3-5-haiku-20241022-v1:0",
            "opus[1m][1m]",
            "gpt-6-sol",
            "llama3.2",
        ] {
            assert!(!AgentKind::Claude.knows(model), "{model:?}");
        }
    }

    #[test]
    fn codex_knows_its_presets_and_the_gpt_family() {
        for model in [
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-5.6-terra",
            "gpt-5.5",
            "gpt-5.4",
            "gpt-7",
        ] {
            assert!(AgentKind::Codex.knows(model), "{model:?}");
        }

        for model in [
            "o3",
            "o4-mini",
            "openai/gpt-oss-20b",
            "codex-auto-review",
            "GPT-6-Sol",
            "gpt-",
            "gpt-.5",
            "opus",
            "claude-opus-4-8",
        ] {
            assert!(!AgentKind::Codex.knows(model), "{model:?}");
        }
    }

    #[test]
    fn an_agent_is_found_by_the_name_it_goes_by_and_no_other() {
        for agent in AgentKind::ALL {
            assert_eq!(AgentKind::named(agent.as_str()), Some(agent));
        }

        for name in ["gemini", "", "Claude", "CODEX", " claude", "claude_code"] {
            assert_eq!(AgentKind::named(name), None, "{name:?}");
        }
    }

    #[test]
    fn each_agent_names_where_it_keeps_its_state_and_how_it_takes_a_login() {
        assert_eq!(AgentKind::Claude.home(), "CLAUDE_CONFIG_DIR");
        assert_eq!(AgentKind::Codex.home(), "CODEX_HOME");
        assert_eq!(AgentKind::Claude.token(), Some("CLAUDE_CODE_OAUTH_TOKEN"));
        assert_eq!(AgentKind::Codex.token(), None);
    }

    #[test]
    fn claude_runs_without_updating_itself_phoning_home_or_remembering_across_turns() {
        assert_eq!(
            AgentKind::Claude.defaults(),
            [
                ("DISABLE_AUTOUPDATER", "1"),
                ("MCP_TOOL_TIMEOUT", "1800000"),
                ("CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT", "1800000"),
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
                ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
            ]
        );
        assert!(AgentKind::Codex.defaults().is_empty());
    }

    #[test]
    fn claude_waits_on_zones_tools_for_as_long_as_a_turn_may_run() {
        let budget = DEFAULT_TIMEOUT.as_millis().to_string();

        for name in ["MCP_TOOL_TIMEOUT", "CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT"] {
            let value = AgentKind::Claude
                .defaults()
                .iter()
                .find(|(variable, _)| *variable == name)
                .map(|(_, value)| *value);
            assert_eq!(value, Some(budget.as_str()), "{name}");
        }
    }

    #[test]
    fn every_claude_turn_loads_no_settings_file_and_refuses_other_sessions() {
        let toolset = toolset();

        for model in [Some("opus"), None, Some("llama3.2:3b")] {
            for served in [None, Some(&toolset)] {
                for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
                    let arguments = AgentKind::Claude.arguments_with(
                        model,
                        served,
                        builtin_tools,
                        CodexSandbox::default(),
                    );

                    assert_eq!(
                        value_after(&arguments, SETTING_SOURCES),
                        Some(""),
                        "{arguments:?}"
                    );
                    let settings: Value = serde_json::from_str(
                        value_after(&arguments, SETTINGS).expect("zone's own settings"),
                    )
                    .expect("the settings to be JSON");
                    assert_eq!(settings, json!({ "crossSessionInbound": "refuse" }));
                    assert_eq!(arguments.last().map(String::as_str), Some("--print"));
                }
            }
        }
    }

    #[test]
    fn codex_is_never_handed_claudes_settings_flags() {
        let toolset = toolset();

        for served in [None, Some(&toolset)] {
            for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
                let arguments = AgentKind::Codex.arguments_with(
                    Some("gpt-6-sol"),
                    served,
                    builtin_tools,
                    CodexSandbox::default(),
                );
                assert!(
                    !arguments
                        .iter()
                        .any(|argument| argument == SETTING_SOURCES || argument == SETTINGS),
                    "{arguments:?}"
                );
            }
        }
    }

    const PERMISSION_BYPASSES: [&str; 8] = [
        "--dangerously-skip-permissions",
        "--dangerously-bypass-approvals-and-sandbox",
        "--full-auto",
        "--always-approve",
        "--yolo",
        "--permission-mode",
        "--permission-prompts",
        "--ask-for-approval",
    ];

    #[test]
    fn only_zones_tools_are_let_through_and_only_its_sandbox_confines_codex() {
        let toolset = toolset();
        let mut unconfined = Vec::new();

        for served in [None, Some(&toolset)] {
            for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
                for sandbox in CodexSandbox::ALL {
                    let claude = AgentKind::Claude.arguments_with(
                        Some("opus"),
                        served,
                        builtin_tools,
                        sandbox,
                    );
                    let codex = codex(served, builtin_tools, sandbox);
                    for arguments in [&claude, &codex] {
                        let joined = arguments.join(" ");
                        for bypass in PERMISSION_BYPASSES {
                            assert!(!joined.contains(bypass), "{bypass}: {joined}");
                        }
                    }

                    assert_eq!(
                        value_after(&claude, ALLOWED_TOOLS),
                        served.map(|_| "mcp__zone__read_file,mcp__zone__run_command"),
                        "{claude:?}"
                    );
                    assert_eq!(
                        values_after(&codex, CONFIG)
                            .contains(&r#"mcp_servers.zone.default_tools_approval_mode="approve""#),
                        served.is_some(),
                        "{codex:?}"
                    );

                    let confinement = values_after(&codex, SANDBOX);
                    let expected = match builtin_tools {
                        BuiltinTools::Withheld => READ_ONLY,
                        BuiltinTools::Granted => sandbox.as_str(),
                    };
                    assert_eq!(confinement, [expected], "{codex:?}");
                    if expected == CodexSandbox::DangerFullAccess.as_str() {
                        unconfined.push((served.is_some(), builtin_tools, sandbox));
                    }
                }
            }
        }

        assert_eq!(
            unconfined,
            [
                (false, BuiltinTools::Granted, CodexSandbox::DangerFullAccess),
                (true, BuiltinTools::Granted, CodexSandbox::DangerFullAccess),
            ],
            "codex's own tools run unconfined exactly when a turn grants them danger-full-access"
        );
    }

    fn toolset() -> Toolset {
        Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            ["read_file", "run_command"],
        )
    }

    fn value_after<'a>(arguments: &'a [String], flag: &str) -> Option<&'a str> {
        let index = arguments.iter().position(|argument| argument == flag)?;
        arguments.get(index + 1).map(String::as_str)
    }

    fn values_after<'a>(arguments: &'a [String], flag: &str) -> Vec<&'a str> {
        arguments
            .windows(2)
            .filter(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
            .collect()
    }

    fn codex(
        toolset: Option<&Toolset>,
        builtin_tools: BuiltinTools,
        sandbox: CodexSandbox,
    ) -> Vec<String> {
        AgentKind::Codex.arguments_with(Some("gpt-6-sol"), toolset, builtin_tools, sandbox)
    }

    #[test]
    fn claude_is_pointed_at_zones_tools_and_stripped_of_its_own() {
        let toolset = toolset();
        let arguments = AgentKind::Claude.arguments_with(
            Some("opus"),
            Some(&toolset),
            BuiltinTools::Withheld,
            CodexSandbox::default(),
        );

        assert!(
            arguments.contains(&STRICT_MCP_CONFIG.to_string()),
            "the host's own MCP servers were left in: {arguments:?}"
        );
        assert_eq!(
            value_after(&arguments, ALLOWED_TOOLS),
            Some("mcp__zone__read_file,mcp__zone__run_command")
        );
        assert_eq!(value_after(&arguments, BUILTIN_TOOLS), Some(""));
        assert_eq!(arguments.last().map(String::as_str), Some("--print"));

        let definition: Value =
            serde_json::from_str(value_after(&arguments, MCP_CONFIG).expect("a server definition"))
                .expect("the server definition to be JSON");
        let server = &definition["mcpServers"][Toolset::SERVER];
        assert_eq!(server["type"], json!("http"));
        assert_eq!(server["url"], json!("http://127.0.0.1:8421/mcp"));
        assert_eq!(
            server["headers"]["Authorization"],
            json!("Bearer ${ZONE_MCP_TOKEN}")
        );
    }

    #[test]
    fn the_turns_token_never_reaches_the_command_line() {
        let toolset = toolset();

        for agent in AgentKind::ALL {
            for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
                for sandbox in CodexSandbox::ALL {
                    let arguments = agent
                        .arguments_with(Some("model"), Some(&toolset), builtin_tools, sandbox)
                        .join(" ");

                    assert!(
                        !arguments.contains("zone-turn-notarealtoken"),
                        "{agent} was handed the token in argv: {arguments}"
                    );
                    assert!(
                        arguments.contains(Toolset::TOKEN_VARIABLE),
                        "{agent} is not told which variable holds the token: {arguments}"
                    );
                }
            }
        }
    }

    #[test]
    fn granting_the_agent_its_own_tools_stops_them_being_withheld() {
        let toolset = toolset();
        let arguments = AgentKind::Claude.arguments_with(
            Some("opus"),
            Some(&toolset),
            BuiltinTools::Granted,
            CodexSandbox::default(),
        );

        assert!(
            !arguments.contains(&BUILTIN_TOOLS.to_string()),
            "the agent's own tools were still withheld: {arguments:?}"
        );
        assert!(arguments.contains(&STRICT_MCP_CONFIG.to_string()));
        assert!(arguments.contains(&ALLOWED_TOOLS.to_string()));
    }

    #[test]
    fn a_turn_with_no_tools_of_zones_still_withholds_the_agents_own() {
        let arguments = AgentKind::Claude.arguments_with(
            Some("opus"),
            None,
            BuiltinTools::Withheld,
            CodexSandbox::default(),
        );

        assert!(
            arguments.contains(&BUILTIN_TOOLS.to_string()),
            "{arguments:?}"
        );
        assert!(
            arguments.contains(&STRICT_MCP_CONFIG.to_string()),
            "{arguments:?}"
        );
        assert!(
            !arguments.contains(&MCP_CONFIG.to_string()),
            "{arguments:?}"
        );
    }

    #[test]
    fn codex_is_pointed_at_zones_tools_and_stripped_of_its_own() {
        let toolset = toolset();

        assert_eq!(
            codex(
                Some(&toolset),
                BuiltinTools::Withheld,
                CodexSandbox::default()
            ),
            [
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--model",
                "gpt-6-sol",
                "--ignore-user-config",
                "--disable",
                "apps",
                "--disable",
                "plugins",
                "--disable",
                "shell_tool",
                "--disable",
                "view_image",
                "--disable",
                "goals",
                "--disable",
                "sleep_tool",
                "--disable",
                "image_generation",
                "-c",
                "agents.enabled=false",
                "-c",
                r#"web_search="disabled""#,
                "-c",
                "tools.experimental_request_user_input.enabled=false",
                "--sandbox",
                "read-only",
                "-c",
                r#"mcp_servers.zone.url="http://127.0.0.1:8421/mcp""#,
                "-c",
                r#"mcp_servers.zone.bearer_token_env_var="ZONE_MCP_TOKEN""#,
                "-c",
                r#"mcp_servers.zone.enabled_tools=["read_file","run_command"]"#,
                "-c",
                "mcp_servers.zone.tool_timeout_sec=1800",
                "-c",
                "mcp_servers.zone.startup_timeout_sec=30",
                "-c",
                r#"mcp_servers.zone.default_tools_approval_mode="approve""#,
                "-c",
                "mcp_servers.zone.required=true",
                "-c",
                r#"mcp_servers.zone.omit_tools_from=["deferred"]"#,
                "-",
            ]
        );
    }

    #[test]
    fn a_granted_codex_turn_keeps_its_own_tools_in_the_sandbox_it_was_given() {
        let toolset = toolset();

        for (sandbox, expected) in [
            (CodexSandbox::WorkspaceWrite, "workspace-write"),
            (CodexSandbox::DangerFullAccess, "danger-full-access"),
        ] {
            let arguments = codex(Some(&toolset), BuiltinTools::Granted, sandbox);

            assert_eq!(
                values_after(&arguments, "--sandbox"),
                [expected],
                "{arguments:?}"
            );
            assert_eq!(
                values_after(&arguments, "--disable"),
                ["apps", "plugins"],
                "a granted turn keeps codex's own tools: {arguments:?}"
            );
            assert!(
                arguments.contains(&"--ignore-user-config".to_string()),
                "the operator's own MCP servers were left in beside zone's: {arguments:?}"
            );
            assert!(
                values_after(&arguments, "-c")
                    .contains(&r#"mcp_servers.zone.url="http://127.0.0.1:8421/mcp""#),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn a_codex_turn_withholding_its_own_tools_is_read_only_whatever_sandbox_was_chosen() {
        let toolset = toolset();

        for toolset in [None, Some(&toolset)] {
            for sandbox in CodexSandbox::ALL {
                let arguments = codex(toolset, BuiltinTools::Withheld, sandbox);
                assert_eq!(
                    values_after(&arguments, "--sandbox"),
                    ["read-only"],
                    "{arguments:?}"
                );
            }
        }
    }

    #[test]
    fn a_codex_turn_with_no_tools_of_zones_still_withholds_its_own() {
        assert_eq!(
            codex(None, BuiltinTools::Withheld, CodexSandbox::default()),
            [
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--model",
                "gpt-6-sol",
                "--ignore-user-config",
                "--disable",
                "apps",
                "--disable",
                "plugins",
                "--disable",
                "shell_tool",
                "--disable",
                "view_image",
                "--disable",
                "goals",
                "--disable",
                "sleep_tool",
                "--disable",
                "image_generation",
                "-c",
                "agents.enabled=false",
                "-c",
                r#"web_search="disabled""#,
                "-c",
                "tools.experimental_request_user_input.enabled=false",
                "--sandbox",
                "read-only",
                "-",
            ]
        );
    }

    #[test]
    fn codex_keeps_no_shell_once_its_own_tools_are_withheld() {
        assert!(
            values_after(
                &codex(None, BuiltinTools::Withheld, CodexSandbox::default()),
                "--disable"
            )
            .contains(&"shell_tool")
        );
    }

    #[test]
    fn every_codex_turn_reads_its_prompt_from_stdin_and_keeps_its_session() {
        let toolset = toolset();

        for toolset in [None, Some(&toolset)] {
            for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
                for sandbox in CodexSandbox::ALL {
                    let arguments = codex(toolset, builtin_tools, sandbox);
                    assert_eq!(
                        arguments.last().map(String::as_str),
                        Some("-"),
                        "{arguments:?}"
                    );
                    assert!(
                        !arguments.contains(&"--ephemeral".to_string()),
                        "{arguments:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn zones_settings_reach_codex_as_toml_it_cannot_misread() {
        let toolset = Toolset::new(
            "http://127.0.0.1:8421/mcp\"\\\n\u{7f}",
            "zone-turn-notarealtoken",
            ["read_file", "run_command"],
        );

        let arguments = codex(
            Some(&toolset),
            BuiltinTools::Withheld,
            CodexSandbox::default(),
        );
        let settings = values_after(&arguments, "-c");

        assert!(
            settings.contains(&r#"mcp_servers.zone.url="http://127.0.0.1:8421/mcp\"\\\n\u007F""#),
            "{settings:?}"
        );
        assert!(
            settings.contains(&r#"mcp_servers.zone.enabled_tools=["read_file","run_command"]"#),
            "{settings:?}"
        );
    }

    #[test]
    fn a_toolset_naming_no_tools_lets_codex_call_none_of_the_servers() {
        let toolset = Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            Vec::<String>::new(),
        );

        let arguments = codex(
            Some(&toolset),
            BuiltinTools::Withheld,
            CodexSandbox::default(),
        );

        assert!(
            values_after(&arguments, "-c").contains(&"mcp_servers.zone.enabled_tools=[]"),
            "codex offers every tool a server lists unless it is told otherwise: {arguments:?}"
        );
    }

    #[test]
    fn an_unrecognised_line_is_ignored_rather_than_fatal() {
        for agent in [AgentKind::Claude, AgentKind::Codex] {
            let mut events = Vec::new();
            agent.interpret(r#"{"type":"something_added_next_release"}"#, &mut events);
            agent.interpret("not json at all", &mut events);
            agent.interpret("", &mut events);
            assert!(events.is_empty(), "{agent} reacted to noise");
        }
    }
}
