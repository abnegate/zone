/// Whether a spawned agent keeps the tools it ships with.
///
/// The agent runs as the host user and outside [`abnegate_exec`]'s sandbox, so
/// its own file and shell tools reach every file that user can reach, and
/// nothing zone can see gates them. Zone's tools are gated where they run,
/// which is zone. So a turn gets zone's tools alone unless the operator
/// decides otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuiltinTools {
    #[default]
    Withheld,
    Granted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_withholds_the_agents_own_tools_until_an_operator_says_otherwise() {
        assert_eq!(BuiltinTools::default(), BuiltinTools::Withheld);
    }
}
