//! What keeps a chat's agent inside zone.

use zone_core::llm::BuiltinTools;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Confinement {
    /// `chats.agent_sandboxed`: the agent gets zone's tools and nothing else.
    pub(super) sandboxed: bool,
    /// `chats.offline`: the agent reaches no network on the chat's behalf.
    pub(super) offline: bool,
}

impl Confinement {
    /// An unsandboxed chat leaves the agent the file and shell tools it ships with, which run
    /// outside zone. Claude asks zone before running one, under the chat's approval policy;
    /// codex's are confined by its sandbox alone. An offline chat withholds them too: they reach
    /// the network without zone seeing it.
    pub(super) const fn builtin_tools(self) -> BuiltinTools {
        match self.sandboxed || self.offline {
            true => BuiltinTools::Withheld,
            false => BuiltinTools::Granted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zone's tools reach a spawned coding agent over MCP (see `crate::mcp`), and the agent's own
    /// tools survive beside them only in a chat that is neither sandboxed nor offline.
    #[test]
    fn only_an_unsandboxed_online_chat_keeps_the_agents_own_tools() {
        for (sandboxed, offline, kept) in [
            (false, false, BuiltinTools::Granted),
            (true, false, BuiltinTools::Withheld),
            (false, true, BuiltinTools::Withheld),
            (true, true, BuiltinTools::Withheld),
        ] {
            assert_eq!(
                Confinement { sandboxed, offline }.builtin_tools(),
                kept,
                "sandboxed {sandboxed}, offline {offline}: the agent's own tools reach the \
                 network and the machine without zone gating them"
            );
        }
    }
}
