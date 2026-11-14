//! Where a coding agent CLI is told to keep its settings, sessions and sign-in.

use abnegate_agent_cli::AgentKind;

/// The variable Claude Code reads an organization's subscription token from.
pub const CLAUDE_CODE_OAUTH_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// The variable naming the directory `agent` keeps its settings, sessions and sign-in in, or
/// `None` for an agent that names none.
pub fn variable(agent: AgentKind) -> Option<&'static str> {
    agent.configuration().first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_agent_names_the_directory_it_keeps_its_state_in() {
        assert_eq!(variable(AgentKind::Claude), Some("CLAUDE_CONFIG_DIR"));
        assert_eq!(variable(AgentKind::Codex), Some("CODEX_HOME"));
    }
}
