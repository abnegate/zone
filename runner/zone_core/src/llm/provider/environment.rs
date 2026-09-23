//! The environment a spawned coding agent starts from.
//!
//! The server's own environment holds the database URL, the JWT and encryption
//! keys, the LiteLLM key and provider API keys. An agent inherits none of it:
//! it starts from the names below and whatever an operator adds to them, never
//! a credential, and everything else it needs arrives through its settings.

use std::collections::BTreeMap;

/// Names an agent inherits from the server.
pub const INHERITED: &[&str] = &[
    "HOME",
    "PATH",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "LANG",
    "TZ",
    "TMPDIR",
    "XDG_RUNTIME_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
];

/// Locale variables (`LC_ALL`, `LC_CTYPE` and the rest) pass through as a set.
pub const INHERITED_PREFIX: &str = "LC_";

/// Names an operator adds to [`INHERITED`], comma separated.
pub const PASSTHROUGH: &str = "ZONE_AGENT_ENV_PASSTHROUGH";

/// Credentials an agent is handed by Zone alone, for the turn it serves, and
/// never inherits even when an operator names them: a key an operator passed
/// through for their own scripts would otherwise stand in for every
/// organization's own sign-in.
pub const CREDENTIALS: &[&str] = &[
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "ZONE_MCP_TOKEN",
];

/// The part of `variables` an agent may inherit, given the operator's
/// comma-separated `passthrough` list.
pub fn filter(
    variables: impl IntoIterator<Item = (String, String)>,
    passthrough: &str,
) -> BTreeMap<String, String> {
    let named: Vec<&str> = passthrough
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();

    variables
        .into_iter()
        .filter(|(name, _)| {
            !CREDENTIALS.contains(&name.as_str())
                && (INHERITED.contains(&name.as_str())
                    || name.starts_with(INHERITED_PREFIX)
                    || named.contains(&name.as_str()))
        })
        .collect()
}

/// What an agent inherits from this process.
///
/// A name or value that is not Unicode is left out rather than mangled.
pub fn inherited() -> BTreeMap<String, String> {
    let passthrough = std::env::var(PASSTHROUGH).unwrap_or_default();
    let variables = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)));

    filter(variables, &passthrough)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{AgentKind, Toolset};

    fn variables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn the_servers_secrets_and_the_agents_own_state_are_never_inherited() {
        let refused = [
            ("DATABASE_URL", "postgres://zone:password@postgres/zone"),
            ("JWT_SECRET", "jwt-notarealsecret"),
            ("ENCRYPTION_KEY", "encryption-notarealkey"),
            ("LITELLM_KEY", "sk-litellm-notarealkey"),
            ("SECURITY_LITELLM_MASTER_KEY", "sk-litellm-notarealkey"),
            ("ANTHROPIC_API_KEY", "sk-ant-notarealkey"),
            ("OPENAI_API_KEY", "sk-notarealkey"),
            ("CLAUDE_CONFIG_DIR", "/home/zone/.claude-server"),
            ("CODEX_HOME", "/home/zone/.codex-server"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            ("ZONE_MCP_TOKEN", "zone-turn-leftover"),
            (PASSTHROUGH, ""),
        ];

        let inherited = filter(
            variables(&refused)
                .into_iter()
                .chain(variables(&[("PATH", "/usr/bin")])),
            "",
        );

        for (name, _) in refused {
            assert!(
                !inherited.contains_key(name),
                "{name} must never reach a spawned agent: {inherited:?}"
            );
        }
        assert_eq!(inherited.get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn what_an_agent_needs_to_run_is_inherited_unchanged() {
        let kept = [
            ("HOME", "/home/zone"),
            ("PATH", "/usr/local/bin:/usr/bin:/bin"),
            ("LANG", "C.UTF-8"),
            ("LC_ALL", "C.UTF-8"),
            ("LC_CTYPE", "UTF-8"),
            ("HTTPS_PROXY", "http://proxy:3128"),
            ("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt"),
            ("NODE_EXTRA_CA_CERTS", "/etc/ssl/certs/extra.pem"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ];

        let inherited = filter(variables(&kept), "");

        for (name, value) in kept {
            assert_eq!(
                inherited.get(name).map(String::as_str),
                Some(value),
                "{name} did not reach the agent as it was"
            );
        }
    }

    #[test]
    fn every_inherited_name_passes_and_nothing_else_does() {
        let mut pairs: Vec<(String, String)> = INHERITED
            .iter()
            .map(|name| ((*name).to_string(), "value".to_string()))
            .collect();
        pairs.push(("SOMETHING_ELSE".to_string(), "value".to_string()));
        pairs.push((
            "CARGO_MANIFEST_DIR".to_string(),
            "/src/zone_core".to_string(),
        ));

        let inherited = filter(pairs, "");

        assert_eq!(inherited.keys().map(String::as_str).collect::<Vec<_>>(), {
            let mut names = INHERITED.to_vec();
            names.sort_unstable();
            names
        });
    }

    #[test]
    fn an_operator_can_name_more_variables_to_pass_through() {
        let inherited = filter(
            variables(&[
                ("ANTHROPIC_BASE_URL", "https://gateway.internal"),
                ("CORPORATE_CA", "/etc/ssl/corporate.pem"),
                ("DATABASE_URL", "postgres://zone:password@postgres/zone"),
            ]),
            " ANTHROPIC_BASE_URL , ,CORPORATE_CA,",
        );

        assert_eq!(
            inherited.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some("https://gateway.internal")
        );
        assert_eq!(
            inherited.get("CORPORATE_CA").map(String::as_str),
            Some("/etc/ssl/corporate.pem")
        );
        assert!(
            !inherited.contains_key("DATABASE_URL"),
            "a name the operator did not list passed through: {inherited:?}"
        );
    }

    /// An operator who once named a key for their own scripts would otherwise
    /// have every organization's turns run on it rather than on their own
    /// sign-in, with nothing to say so.
    #[test]
    fn a_passthrough_naming_a_credential_still_drops_it() {
        let inherited = filter(
            CREDENTIALS.iter().map(|name| {
                (
                    (*name).to_string(),
                    "notreal-operator-credential".to_string(),
                )
            }),
            &CREDENTIALS.join(","),
        );

        assert!(
            inherited.is_empty(),
            "a credential was passed through: {:?}",
            inherited.keys().collect::<Vec<_>>()
        );
    }

    /// The refusal is of those names alone. Whatever else an operator names
    /// beside them still passes, `CLAUDE_CONFIG_DIR` included, which is how
    /// an instance points claude at a host login kept somewhere else.
    #[test]
    fn a_passthrough_naming_a_harmless_variable_still_passes_it() {
        let inherited = filter(
            variables(&[
                ("ANTHROPIC_BASE_URL", "https://gateway.internal"),
                ("CLAUDE_CONFIG_DIR", "/home/zone/.claude-host"),
            ]),
            &format!(
                "ANTHROPIC_BASE_URL,CLAUDE_CONFIG_DIR,{}",
                CREDENTIALS.join(",")
            ),
        );

        assert_eq!(
            inherited.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some("https://gateway.internal")
        );
        assert_eq!(
            inherited.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/home/zone/.claude-host")
        );
    }

    /// Every variable Zone hands an agent a credential or a turn's token in is
    /// on the list, so an agent's credential cannot be passed through from the
    /// server by naming it.
    #[test]
    fn every_variable_zone_hands_a_credential_in_is_refused() {
        let handed = AgentKind::ALL
            .into_iter()
            .flat_map(|agent| [Some(agent.variable()), agent.token()])
            .flatten()
            .chain([Toolset::TOKEN_VARIABLE]);

        for name in handed {
            assert!(CREDENTIALS.contains(&name), "{name} can be passed through");
        }
    }

    #[test]
    fn an_empty_entry_in_the_passthrough_list_names_nothing() {
        let inherited = filter(variables(&[("", "empty"), ("JWT_SECRET", "x")]), ", ,");

        assert!(inherited.is_empty(), "{inherited:?}");
    }
}
