//! Which agents carry a CLI session from one login's home into another, so the next turn resumes
//! the conversation on whichever login it lands on. Moving the session's file is
//! [`abnegate_agent_cli::Session`]'s.

use abnegate_agent_cli::AgentKind;

/// Whether `agent` resumes a session whose file was carried in from another login's home. Set
/// from `agent_session_resume_tests`, which runs each CLI across two homes.
pub fn portable(agent: AgentKind) -> bool {
    matches!(agent, AgentKind::Claude | AgentKind::Codex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_agents_resume_a_carried_session() {
        assert!(AgentKind::ALL.iter().copied().all(portable));
    }
}
