//! The coding agent CLIs this crate knows how to drive.

use std::fmt;

use serde_json::{Map, Value, json};

use super::event::AgentEvent;
use super::parser;
use super::settings::{BuiltinTools, Toolset};

const MODEL: &str = "--model";
const SETTING_SOURCES: &str = "--setting-sources";
const NO_SETTING_SOURCES: &str = "";
const SETTINGS: &str = "--settings";
const CROSS_SESSION_REFUSED: &str = r#"{"crossSessionInbound":"refuse"}"#;

/// Passed to every claude turn, whoever decides its tools.
///
/// Naming no setting source leaves out claude's settings files, and with them
/// every hook, `CLAUDE.md`, skill and agent that the host user, the working
/// directory or any directory above it would add. Settings given as a flag
/// still apply; these refuse messages from the user's other claude sessions.
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

/// `fable` is known but not offered: a subscriber has to accept its usage
/// credits interactively first, and a headless turn without that falls back
/// to another model or fails.
const CLAUDE_MODELS: &[&str] = &["sonnet", "opus", "haiku"];

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
const MCP_TOOL_PREFIX: &str = "mcp__";

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

    /// The environment variable this agent reads its key from, when a key is
    /// configured at all. An agent already signed in on the host needs none.
    pub fn variable(self) -> &'static str {
        match self {
            Self::Claude => "ANTHROPIC_API_KEY",
            Self::Codex => "OPENAI_API_KEY",
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

    /// A non-interactive invocation that streams newline-delimited JSON, run
    /// with the agent's own tools and none of zone's.
    pub fn arguments(self, model: Option<&str>) -> Vec<String> {
        self.arguments_with(model, None, BuiltinTools::Granted)
    }

    /// The same invocation, told which tools the turn may call.
    ///
    /// Zone's tools arrive over MCP because an agent runs its own loop and
    /// cannot be handed tool schemas over a completions API. They are
    /// allowlisted so the agent raises no second prompt that nobody is there
    /// to answer: the call executes in zone, where the chat's approval policy
    /// decides it first.
    ///
    /// No permission-bypass flag is passed to any agent. These commands run
    /// with the user's own credentials and file access, so whatever the agent
    /// still owns is still the agent's own to ask about.
    pub fn arguments_with(
        self,
        model: Option<&str>,
        toolset: Option<&Toolset>,
        builtin_tools: BuiltinTools,
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

        if self.accepts_toolset() {
            arguments.extend(tool_arguments(toolset, builtin_tools));
        }

        arguments.push(
            match self {
                Self::Claude => "--print",
                Self::Codex => "-",
            }
            .to_string(),
        );

        arguments
    }

    /// Whether zone can decide this agent's tools -- serve its own and
    /// withhold the agent's built-in ones.
    ///
    /// Codex cannot. Its `-c mcp_servers.<name>` override adds a server to the
    /// operator's own rather than replacing them, there is no counterpart to
    /// `--strict-mcp-config`, and its shell cannot be taken away, only
    /// sandboxed. Doing half of it is worse than none: the turn would carry
    /// the operator's servers and an ungated shell alongside zone's gated
    /// tools, and read as confined while being nothing of the sort.
    pub fn accepts_toolset(self) -> bool {
        match self {
            Self::Claude => true,
            Self::Codex => false,
        }
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

/// The flags naming a turn's tools, for an agent that lets zone name them.
fn tool_arguments(toolset: Option<&Toolset>, builtin_tools: BuiltinTools) -> Vec<String> {
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
        .map(|tool| {
            format!(
                "{MCP_TOOL_PREFIX}{server}__{tool}",
                server = Toolset::SERVER
            )
        })
        .collect::<Vec<String>>()
        .join(",")
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
    use crate::llm::provider::DEFAULT_TIMEOUT;

    #[test]
    fn the_prompt_is_never_placed_on_the_command_line() {
        for agent in [AgentKind::Claude, AgentKind::Codex] {
            assert_eq!(agent.delivery(), Delivery::Stdin, "{agent}");
        }
    }

    #[test]
    fn every_agent_is_listed_with_the_key_variable_it_reads() {
        assert!(AgentKind::ALL.contains(&AgentKind::Claude));
        assert!(AgentKind::ALL.contains(&AgentKind::Codex));

        let variables: Vec<&str> = AgentKind::ALL
            .iter()
            .map(|agent| agent.variable())
            .collect();
        assert_eq!(variables, ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]);
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
        assert_eq!(AgentKind::Claude.models(), ["sonnet", "opus", "haiku"]);
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
    fn claude_knows_fable_without_offering_it() {
        assert!(AgentKind::Claude.knows("fable"));
        assert!(!AgentKind::Claude.models().contains(&"fable"));
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
                    let arguments = AgentKind::Claude.arguments_with(model, served, builtin_tools);

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
                let arguments =
                    AgentKind::Codex.arguments_with(Some("gpt-6-sol"), served, builtin_tools);
                assert!(
                    !arguments
                        .iter()
                        .any(|argument| argument == SETTING_SOURCES || argument == SETTINGS),
                    "{arguments:?}"
                );
            }
        }
    }

    #[test]
    fn no_agent_is_ever_asked_to_skip_its_permission_prompts() {
        for agent in [AgentKind::Claude, AgentKind::Codex] {
            let arguments = AgentKind::arguments(agent, Some("model")).join(" ");
            for bypass in [
                "--dangerously-skip-permissions",
                "--full-auto",
                "--always-approve",
                "--yolo",
                "--permission-mode",
            ] {
                assert!(
                    !arguments.contains(bypass),
                    "{agent} was handed {bypass}: {arguments}"
                );
            }
        }
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

    #[test]
    fn claude_is_pointed_at_zones_tools_and_stripped_of_its_own() {
        let toolset = toolset();
        let arguments =
            AgentKind::Claude.arguments_with(Some("opus"), Some(&toolset), BuiltinTools::Withheld);

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

        for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
            let arguments = AgentKind::Claude
                .arguments_with(Some("opus"), Some(&toolset), builtin_tools)
                .join(" ");

            assert!(
                !arguments.contains("zone-turn-notarealtoken"),
                "the token reached argv: {arguments}"
            );
            assert!(arguments.contains("${ZONE_MCP_TOKEN}"), "{arguments}");
        }
    }

    #[test]
    fn granting_the_agent_its_own_tools_stops_them_being_withheld() {
        let toolset = toolset();
        let arguments =
            AgentKind::Claude.arguments_with(Some("opus"), Some(&toolset), BuiltinTools::Granted);

        assert!(
            !arguments.contains(&BUILTIN_TOOLS.to_string()),
            "the agent's own tools were still withheld: {arguments:?}"
        );
        assert!(arguments.contains(&STRICT_MCP_CONFIG.to_string()));
        assert!(arguments.contains(&ALLOWED_TOOLS.to_string()));
    }

    #[test]
    fn a_turn_with_no_tools_of_zones_still_withholds_the_agents_own() {
        let arguments =
            AgentKind::Claude.arguments_with(Some("opus"), None, BuiltinTools::Withheld);

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
    fn codex_is_left_without_an_injected_toolset() {
        let toolset = toolset();

        assert!(!AgentKind::Codex.accepts_toolset());
        assert_eq!(
            AgentKind::Codex.arguments_with(
                Some("gpt-6-sol"),
                Some(&toolset),
                BuiltinTools::Withheld
            ),
            AgentKind::Codex.arguments(Some("gpt-6-sol"))
        );
    }

    #[test]
    fn no_agent_is_ever_asked_to_skip_its_permission_prompts_while_zone_serves_its_tools() {
        let toolset = toolset();

        for agent in AgentKind::ALL {
            for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
                let arguments = agent
                    .arguments_with(Some("model"), Some(&toolset), builtin_tools)
                    .join(" ");
                for bypass in [
                    "--dangerously-skip-permissions",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "--full-auto",
                    "--always-approve",
                    "--yolo",
                    "--permission-mode",
                    "--permission-prompts",
                ] {
                    assert!(
                        !arguments.contains(bypass),
                        "{agent} was handed {bypass}: {arguments}"
                    );
                }
            }
        }
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
