//! The coding agent CLIs this crate knows how to drive.

use std::fmt;

use serde_json::{Map, Value, json};

use super::event::AgentEvent;
use super::parser;
use super::settings::{BuiltinTools, Toolset};

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
    /// A caller that must act on all of them -- clearing their keys out of a
    /// child environment, for one -- reads this instead of repeating the list
    /// and going stale when an agent is added.
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

        if let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) {
            arguments.push("--model".to_string());
            arguments.push(model.to_string());
        }

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

#[cfg(test)]
mod tests {
    use super::*;

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
                "--print",
            ]
        );
    }

    #[test]
    fn codex_streams_json_and_reads_its_prompt_from_stdin() {
        let arguments = AgentKind::Codex.arguments(Some("o3"));

        assert_eq!(
            arguments,
            [
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--model",
                "o3",
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
            AgentKind::Codex.arguments_with(Some("o3"), Some(&toolset), BuiltinTools::Withheld),
            AgentKind::Codex.arguments(Some("o3"))
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
