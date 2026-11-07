use std::collections::{BTreeMap, BTreeSet};

use abnegate_agent_cli::{AgentKind, CLOUD_CREDENTIALS};
use abnegate_exec::{DEFAULT_ENVIRONMENT, EnvironmentPolicy};

use super::{Toolset, passthrough};

/// What a child that is not an agent, such as a login CLI, `rg` or `git`,
/// inherits beside [`DEFAULT_ENVIRONMENT`]: the proxy it reaches the network
/// through, the user's shell and runtime directory, where the certificate
/// authorities are kept, and the POSIX locale categories.
pub const EXTRAS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SHELL",
    "XDG_RUNTIME_DIR",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "LC_ALL",
    "LC_CTYPE",
    "LC_COLLATE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_ADDRESS",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_NAME",
    "LC_PAPER",
    "LC_TELEPHONE",
];

/// Claude Code's OAuth refresh token, a sign-in no agent lists among its
/// credentials.
const CLAUDE_REFRESH_TOKEN: &str = "CLAUDE_CODE_OAUTH_REFRESH_TOKEN";

/// What a child that is not an agent inherits from this process: the
/// [`inheritable`] names and the operator's [`passthrough`], each when this
/// process has it set. A name or value that is not Unicode is left out
/// rather than mangled.
pub fn inherited() -> BTreeMap<String, String> {
    EnvironmentPolicy::allowlist()
        .allow(inheritable(&passthrough()))
        .inherited()
        .into_iter()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

/// The names a child that is not an agent may inherit, given the operator's
/// `passthrough`: [`DEFAULT_ENVIRONMENT`], the [`EXTRAS`] and the
/// passthrough, never a sign-in, however it is named.
pub fn inheritable(passthrough: &[String]) -> BTreeSet<String> {
    DEFAULT_ENVIRONMENT
        .iter()
        .chain(EXTRAS)
        .map(|name| (*name).to_string())
        .chain(passthrough.iter().cloned())
        .filter(|name| !credential(name))
        .collect()
}

/// Whether `name` holds a sign-in a child is handed by Zone alone, for the
/// turn it serves: a key an operator passed through for their own scripts
/// would otherwise stand in for every organization's own sign-in.
fn credential(name: &str) -> bool {
    AgentKind::ALL
        .iter()
        .flat_map(|agent| agent.credentials())
        .chain(CLOUD_CREDENTIALS)
        .chain(&[CLAUDE_REFRESH_TOKEN, Toolset::TOKEN_VARIABLE])
        .any(|credential| *credential == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::cli::PASSTHROUGH;

    /// Set on the copy of this binary that reads its own environment.
    const COPY: &str = "ZONE_CORE_INHERITED_COPY";

    const KEPT: [(&str, &str); 4] = [
        ("HTTPS_PROXY", "http://proxy.internal:3128"),
        ("NO_PROXY", "localhost,127.0.0.1"),
        ("SSL_CERT_DIR", "/etc/ssl/certs"),
        ("LC_TIME", "en_GB.UTF-8"),
    ];

    const DROPPED: [(&str, &str); 2] = [
        ("LC_SNEAKY", "notreal-locale"),
        ("ANTHROPIC_API_KEY", "sk-ant-notrealkey"),
    ];

    fn none() -> Vec<String> {
        Vec::new()
    }

    fn named(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    /// The copy runs with the variables set and reads them through
    /// [`inherited`], so nothing here calls `set_var` on a process whose
    /// other tests start children.
    #[test]
    fn inherited_keeps_the_proxy_for_jobs_and_login_clis() {
        if std::env::var_os(COPY).is_some() {
            let inherited = inherited();
            for (name, value) in KEPT {
                assert_eq!(
                    inherited.get(name).map(String::as_str),
                    Some(value),
                    "{name} did not reach the child"
                );
            }
            for (name, _) in DROPPED {
                assert!(
                    !inherited.contains_key(name),
                    "{name} reached the child: {:?}",
                    inherited.keys().collect::<Vec<_>>()
                );
            }
            return;
        }

        let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
            .args([
                "--exact",
                "llm::cli::inherited::tests::inherited_keeps_the_proxy_for_jobs_and_login_clis",
                "--nocapture",
            ])
            .env(COPY, "1")
            .envs(KEPT)
            .envs(DROPPED)
            .env(PASSTHROUGH, "ANTHROPIC_API_KEY")
            .output()
            .expect("a copy of this test binary");
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "{printed}");
        assert!(
            printed.contains("1 passed"),
            "the copy ran no test: {printed}"
        );
    }

    #[test]
    fn every_inheritable_name_is_the_allowlist_and_its_extras() {
        let expected: BTreeSet<String> = [
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "TMPDIR",
            "LANG",
            "TERM",
            "TZ",
            "SSL_CERT_FILE",
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
            "SHELL",
            "XDG_RUNTIME_DIR",
            "SSL_CERT_DIR",
            "NODE_EXTRA_CA_CERTS",
            "LC_ALL",
            "LC_CTYPE",
            "LC_COLLATE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();

        assert_eq!(inheritable(&none()), expected);
    }

    #[test]
    fn the_servers_secrets_and_the_agents_own_state_are_never_inherited() {
        let inheritable = inheritable(&none());

        for name in [
            "DATABASE_URL",
            "JWT_SECRET",
            "ENCRYPTION_KEY",
            "LITELLM_KEY",
            "SECURITY_LITELLM_MASTER_KEY",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "LC_SNEAKY",
            Toolset::TOKEN_VARIABLE,
            PASSTHROUGH,
        ] {
            assert!(
                !inheritable.contains(name),
                "{name} must never reach a child: {inheritable:?}"
            );
        }
    }

    #[test]
    fn an_operator_can_name_more_variables_to_pass_through() {
        let inheritable = inheritable(&named(&["CORPORATE_CA", "CLAUDE_CONFIG_DIR"]));

        assert!(inheritable.contains("CORPORATE_CA"));
        assert!(
            inheritable.contains("CLAUDE_CONFIG_DIR"),
            "an instance points claude at a host login kept somewhere else this way"
        );
        assert!(!inheritable.contains("DATABASE_URL"));
    }

    /// An operator who once named a key for their own scripts would otherwise
    /// have every organization's turns run on it rather than on their own
    /// sign-in, with nothing to say so. A base URL counts too, since it
    /// points an agent at whoever answers it.
    #[test]
    fn a_passthrough_naming_a_credential_still_drops_it() {
        let credentials: Vec<String> = AgentKind::ALL
            .iter()
            .flat_map(|agent| agent.credentials())
            .chain(CLOUD_CREDENTIALS)
            .chain(&[CLAUDE_REFRESH_TOKEN, Toolset::TOKEN_VARIABLE])
            .map(|name| (*name).to_string())
            .collect();

        let inheritable = inheritable(&credentials);

        for credential in &credentials {
            assert!(
                !inheritable.contains(credential),
                "{credential} can be passed through"
            );
        }
        assert!(credentials.iter().any(|name| name == "ANTHROPIC_BASE_URL"));
    }
}
