//! What keeps a chat's agent inside zone.

use abnegate_agent::tool::Tier;
use abnegate_agent_cli::AgentKind;
use zone_core::llm::{BuiltinTools, LlmBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Confinement {
    pub(super) sandboxed: bool,
    pub(super) offline: bool,
    pub(super) ceiling: Option<Tier>,
}

impl Confinement {
    /// The agent's own tools reach the host and the network without zone gating them, so a
    /// ceiling keeps them only for an agent that asks zone before each one runs, as Claude does
    /// when it is served a toolset.
    pub(super) const fn builtin_tools(self, backend: &LlmBackend) -> BuiltinTools {
        let prompted = matches!(
            backend,
            LlmBackend::Cli {
                agent: AgentKind::Claude,
                ..
            }
        );
        match self.sandboxed || self.offline || (self.ceiling.is_some() && !prompted) {
            true => BuiltinTools::Withheld,
            false => BuiltinTools::Granted,
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::CliSettings;

    use super::*;

    fn agent(kind: AgentKind) -> LlmBackend {
        LlmBackend::cli(kind, CliSettings::default())
    }

    #[test]
    fn only_an_unsandboxed_online_chat_keeps_the_agents_own_tools() {
        for kind in [AgentKind::Claude, AgentKind::Codex] {
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
                    }
                    .builtin_tools(&agent(kind)),
                    kept,
                    "sandboxed {sandboxed}, offline {offline}, {kind:?}: the agent's own tools \
                     reach the network and the machine without zone gating them"
                );
            }
        }
    }

    #[test]
    fn a_ceiling_keeps_the_agents_own_tools_only_when_zone_is_asked_first() {
        for ceiling in [Tier::Read, Tier::Write, Tier::Host, Tier::Outward] {
            for (sandboxed, offline, kind, kept) in [
                (false, false, AgentKind::Codex, BuiltinTools::Withheld),
                (false, false, AgentKind::Claude, BuiltinTools::Granted),
                (true, false, AgentKind::Claude, BuiltinTools::Withheld),
                (false, true, AgentKind::Claude, BuiltinTools::Withheld),
            ] {
                assert_eq!(
                    Confinement {
                        sandboxed,
                        offline,
                        ceiling: Some(ceiling),
                    }
                    .builtin_tools(&agent(kind)),
                    kept,
                    "ceiling {ceiling:?}, sandboxed {sandboxed}, offline {offline}, {kind:?}"
                );
            }
        }
    }
}
