//! What keeps a chat's agent inside zone.

use zone_core::llm::BuiltinTools;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Confinement {
    pub(super) sandboxed: bool,
    pub(super) offline: bool,
}

impl Confinement {
    /// The agent's own tools reach the host and the network without zone gating them.
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
