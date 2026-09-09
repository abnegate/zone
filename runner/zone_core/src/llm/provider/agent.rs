//! The coding agent CLIs this crate knows how to drive.

use std::fmt;

use super::event::AgentEvent;
use super::parser;

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

    /// A non-interactive invocation that streams newline-delimited JSON.
    ///
    /// No permission-bypass flag is passed to any agent. These commands run
    /// with the user's own credentials and file access, so the agent's own
    /// approval behaviour is left exactly as the user configured it.
    pub fn arguments(self, model: Option<&str>) -> Vec<String> {
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
