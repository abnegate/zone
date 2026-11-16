//! What keeps a chat's agent inside zone.

use abnegate_agent::tool::Tier;
use zone_core::llm::BuiltinTools;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Confinement {
    pub(super) sandboxed: bool,
    pub(super) offline: bool,
    /// The highest tier the chat's policy lets auto-approve release.
    pub(super) ceiling: Option<Tier>,
    /// Whether the agent asks zone before running its own tools, as Claude
    /// does when it is served a toolset.
    pub(super) prompted: bool,
}

impl Confinement {
    /// The agent's own tools reach the host and the network without zone gating them, so a
    /// ceiling keeps them only when zone is asked before each one runs.
    pub(super) const fn builtin_tools(self) -> BuiltinTools {
        match self.sandboxed || self.offline || (self.ceiling.is_some() && !self.prompted) {
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
        for prompted in [false, true] {
            for (sandboxed, offline, kept) in [
                (false, false, BuiltinTools::Granted),
                (true, false, BuiltinTools::Withheld),
                (false, true, BuiltinTools::Withheld),
                (true, true, BuiltinTools::Withheld),
            ] {
                assert_eq!(
                    Confinement {
                        sandboxed,
                        offline,
                        ceiling: None,
                        prompted,
                    }
                    .builtin_tools(),
                    kept,
                    "sandboxed {sandboxed}, offline {offline}, prompted {prompted}: the agent's \
                     own tools reach the network and the machine without zone gating them"
                );
            }
        }
    }

    #[test]
    fn a_ceiling_keeps_the_agents_own_tools_only_when_zone_is_asked_first() {
        for ceiling in [Tier::Read, Tier::Write, Tier::Host, Tier::Outward] {
            for (sandboxed, offline, prompted, kept) in [
                (false, false, false, BuiltinTools::Withheld),
                (false, false, true, BuiltinTools::Granted),
                (true, false, true, BuiltinTools::Withheld),
                (false, true, true, BuiltinTools::Withheld),
            ] {
                assert_eq!(
                    Confinement {
                        sandboxed,
                        offline,
                        ceiling: Some(ceiling),
                        prompted,
                    }
                    .builtin_tools(),
                    kept,
                    "ceiling {ceiling:?}, sandboxed {sandboxed}, offline {offline}, prompted \
                     {prompted}"
                );
            }
        }
    }

    /// Codex runs its own tools without asking zone, so a `host` ceiling takes
    /// them away; Claude with a toolset asks first, so it keeps them.
    #[test]
    fn codex_loses_its_own_tools_under_a_ceiling_and_claude_with_a_toolset_keeps_them() {
        let confined = |prompted| Confinement {
            sandboxed: false,
            offline: false,
            ceiling: Some(Tier::Host),
            prompted,
        };
        assert_eq!(confined(false).builtin_tools(), BuiltinTools::Withheld);
        assert_eq!(confined(true).builtin_tools(), BuiltinTools::Granted);
    }
}
