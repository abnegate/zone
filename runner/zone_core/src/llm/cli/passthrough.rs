use super::Toolset;

/// Names an operator adds to what Zone's children inherit, comma separated.
pub const PASSTHROUGH: &str = "ZONE_AGENT_ENV_PASSTHROUGH";

const SEPARATOR: char = ',';

/// The names the operator listed in [`PASSTHROUGH`].
pub fn passthrough() -> Vec<String> {
    listed(&std::env::var(PASSTHROUGH).unwrap_or_default())
}

/// The names in the comma-separated `list`, less blanks and the variable a
/// turn's own MCP token is bound to, which no operator hands a child.
pub(crate) fn listed(list: &str) -> Vec<String> {
    list.split(SEPARATOR)
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != Toolset::TOKEN_VARIABLE)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operator_lists_names_separated_by_commas() {
        assert_eq!(
            listed(" ANTHROPIC_BASE_URL , ,CORPORATE_CA,"),
            ["ANTHROPIC_BASE_URL", "CORPORATE_CA"]
        );
    }

    #[test]
    fn an_empty_entry_in_the_passthrough_list_names_nothing() {
        assert!(listed(", ,").is_empty());
        assert!(listed("").is_empty());
    }

    #[test]
    fn a_passthrough_never_names_the_turns_own_token() {
        assert_eq!(
            listed(&format!("{},CORPORATE_CA", Toolset::TOKEN_VARIABLE)),
            ["CORPORATE_CA"]
        );
    }
}
