//! The environment a conflict repair inherits, and everything it does not.
//!
//! The server process holds the database URL, the JWT and encryption keys, the
//! LiteLLM master key, provider API keys and whatever GitHub token the deployment
//! runs with. A repair agent editing somebody's source file has no use for any of
//! it, and a repair that leaked one into a resolved file would be a credential
//! disclosure committed to a branch.
//!
//! This follows the same shape as the agent tool allowlist — a fixed set of names
//! and nothing else — but keeps a shorter list, because a repair needs less than a
//! task run does: no proxies, no runtime directory, and a `HOME` of its own rather
//! than the operator's. Everything a tool might use to find credentials on disk is
//! pointed at the throwaway directory instead.

use std::collections::HashMap;
use std::path::Path;

/// The only names a conflict repair inherits from the server process.
const INHERITED: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "PATH",
    "SHELL",
    "TERM",
    "TZ",
    "__CF_USER_TEXT_ENCODING",
];

/// Names pointed at the throwaway directory so nothing reads the real ones.
const REDIRECTED: &[&str] = &[
    "HOME",
    "TEMP",
    "TMP",
    "TMPDIR",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
];

/// Names given a fixed value so a repair cannot read config or prompt for a login.
const PINNED: &[(&str, &str)] = &[
    ("GIT_ASKPASS", ""),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_SYSTEM", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

/// Build the environment for one repair, isolated under `directory`.
pub fn restricted(
    variables: impl IntoIterator<Item = (String, String)>,
    directory: &Path,
) -> HashMap<String, String> {
    let mut environment: HashMap<String, String> = variables
        .into_iter()
        .filter(|(name, value)| INHERITED.contains(&name.as_str()) && !value.contains('\0'))
        .collect();

    let isolation = directory.to_string_lossy().to_string();
    for name in REDIRECTED {
        environment.insert((*name).to_string(), isolation.clone());
    }
    for (name, value) in PINNED {
        environment.insert((*name).to_string(), (*value).to_string());
    }

    environment
}

/// The environment for one repair, taken from this process.
pub fn from_process(directory: &Path) -> HashMap<String, String> {
    restricted(std::env::vars(), directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn credentials_never_reach_the_repair() {
        let environment = restricted(
            variables(&[
                ("PATH", "/usr/bin"),
                ("GITHUB_TOKEN", "ghp_secret"),
                ("GITHUB_ACCESS_TOKEN", "ghp_secret"),
                ("DATABASE_URL", "postgres://user:password@host/db"),
                ("JWT_SECRET", "signing-key"),
                ("ZONE_ENCRYPTION_KEY", "encryption-key"),
                ("LITELLM_MASTER_KEY", "sk-master"),
                ("ANTHROPIC_API_KEY", "sk-ant"),
                ("OPENAI_API_KEY", "sk-openai"),
                ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
                ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
                ("NPM_TOKEN", "npm-secret"),
            ]),
            Path::new("/isolated"),
        );

        for refused in [
            "GITHUB_TOKEN",
            "GITHUB_ACCESS_TOKEN",
            "DATABASE_URL",
            "JWT_SECRET",
            "ZONE_ENCRYPTION_KEY",
            "LITELLM_MASTER_KEY",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "SSH_AUTH_SOCK",
            "NPM_TOKEN",
        ] {
            assert!(
                !environment.contains_key(refused),
                "{refused} must never reach a conflict repair"
            );
        }

        assert!(
            !environment.values().any(|value| value.contains("secret")
                || value.contains("password")
                || value.starts_with("sk-")),
            "no inherited value may carry a credential: {environment:?}"
        );
        assert_eq!(
            environment.get("PATH").map(String::as_str),
            Some("/usr/bin")
        );
    }

    #[test]
    fn the_operators_home_is_replaced_by_the_throwaway_directory() {
        let environment = restricted(
            variables(&[("HOME", "/Users/operator"), ("TMPDIR", "/var/folders/real")]),
            Path::new("/isolated"),
        );

        for name in REDIRECTED {
            assert_eq!(
                environment.get(*name).map(String::as_str),
                Some("/isolated"),
                "{name} must point at the throwaway directory, not the operator's"
            );
        }
    }

    #[test]
    fn git_cannot_read_the_hosts_configuration_or_ask_for_a_login() {
        let environment = restricted(Vec::new(), Path::new("/isolated"));
        assert_eq!(
            environment.get("GIT_CONFIG_GLOBAL").map(String::as_str),
            Some("/dev/null")
        );
        assert_eq!(
            environment.get("GIT_CONFIG_SYSTEM").map(String::as_str),
            Some("/dev/null")
        );
        assert_eq!(
            environment.get("GIT_TERMINAL_PROMPT").map(String::as_str),
            Some("0")
        );
    }

    #[test]
    fn an_inherited_value_carrying_a_null_byte_is_dropped() {
        let environment = restricted(
            variables(&[("PATH", "/usr/bin"), ("TERM", "xterm\0extra")]),
            Path::new("/isolated"),
        );
        assert!(environment.contains_key("PATH"));
        assert!(
            !environment.contains_key("TERM"),
            "a value that cannot survive a process boundary intact must not be passed"
        );
    }

    #[test]
    fn nothing_outside_the_three_lists_survives() {
        let environment = restricted(
            variables(&[("PATH", "/usr/bin"), ("SOMETHING_ELSE", "value")]),
            Path::new("/isolated"),
        );

        let permitted: Vec<&str> = INHERITED
            .iter()
            .chain(REDIRECTED.iter())
            .copied()
            .chain(PINNED.iter().map(|(name, _)| *name))
            .collect();

        for name in environment.keys() {
            assert!(
                permitted.contains(&name.as_str()),
                "{name} is not on any list and must not have survived"
            );
        }
    }
}
