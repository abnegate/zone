//! The environment every child a server tool starts is given.

use abnegate_agent::tool::EnvironmentPolicy;
use zone_core::llm::cli;

/// The only process environment variables a tool may see beyond the
/// allowlist.
///
/// The server's environment also holds the database URL, the JWT and
/// encryption keys, the LiteLLM master key and provider API keys. A shell that
/// inherited those would print them into tool output and into stored messages,
/// so both profiles start from this list and nothing else.
const SAFE_ENV: &[&str] = &[
    "ALL_PROXY",
    "HOME",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "LANG",
    "NO_PROXY",
    "PATH",
    "SHELL",
    "TERM",
    "TMPDIR",
    "TOOL_RUNNER_PROXY_URL",
    "TZ",
    "USER",
    "XDG_RUNTIME_DIR",
    "all_proxy",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// Locale variables (`LC_ALL`, `LC_CTYPE`, and the rest) pass through as a set.
const SAFE_ENV_PREFIX: &str = "LC_";

/// The allowlist, [`SAFE_ENV`], the locale variables `variables` holds, and
/// the names any other child of Zone's may inherit given the operator's
/// `passthrough`, so a sign-in passed through for the operator's own scripts
/// never reaches a tool.
pub fn from(
    variables: impl IntoIterator<Item = (String, String)>,
    passthrough: &[String],
) -> EnvironmentPolicy {
    let locales: Vec<String> = variables
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with(SAFE_ENV_PREFIX))
        .collect();
    EnvironmentPolicy::allowlist()
        .allow(SAFE_ENV.iter().copied())
        .allow(locales)
        .allow(cli::inheritable(passthrough))
}

/// [`from`] this process's environment and the operator's passthrough.
pub fn policy() -> EnvironmentPolicy {
    from(std::env::vars(), &cli::passthrough())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use abnegate_agent::tool::job::{JobCommand, Jobs};
    use abnegate_agent::tool::{SearchCodeTool, Session};
    use abnegate_agent::{Tool, ToolContext};
    use abnegate_exec::{PROXY_BYPASS_VARIABLE, PROXY_URL_VARIABLE};
    use serde_json::json;
    use tempfile::tempdir;
    use uuid::Uuid;
    use zone_core::llm::Toolset;

    use super::*;

    /// What a server's environment holds that no child of a tool may be
    /// handed. Every value says `notreal`, so a leak shows under any name.
    const SERVER_SECRETS: [(&str, &str); 8] = [
        (
            "DATABASE_URL",
            "postgres://zone:notrealpassword@postgres/zone",
        ),
        ("JWT_SECRET", "notreal-jwt-secret"),
        ("ENCRYPTION_KEY", "notreal-encryption-key"),
        ("SECURITY_LITELLM_MASTER_KEY", "sk-notreal-master"),
        ("LITELLM_KEY", "sk-notreal-litellm-key"),
        ("POSTGRES_PASSWORD", "notreal-password"),
        ("OPENAI_API_KEY", "sk-notreal-provider"),
        ("ANTHROPIC_API_KEY", "sk-ant-notrealkey"),
    ];

    const LOCALES: [(&str, &str); 2] =
        [("LC_CTYPE", "en_NZ.UTF-8"), ("LC_SNEAKY", "notreal-locale")];

    /// A name the operator adds to what Zone's children inherit.
    const PASSED_THROUGH: (&str, &str) = ("CORPORATE_CA", "/etc/ssl/corporate.pem");

    /// What Zone's own tool environment let through for [`LOCALES`] and
    /// [`PASSED_THROUGH`]: [`SAFE_ENV`], each locale variable set, and the
    /// passthrough.
    const ZONE: [&str; 21] = [
        "ALL_PROXY",
        "HOME",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "LANG",
        "NO_PROXY",
        "PATH",
        "SHELL",
        "TERM",
        "TMPDIR",
        "TOOL_RUNNER_PROXY_URL",
        "TZ",
        "USER",
        "XDG_RUNTIME_DIR",
        "all_proxy",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "LC_CTYPE",
        "LC_SNEAKY",
        "CORPORATE_CA",
    ];

    /// The non-secret names a command gains over [`ZONE`]: from the
    /// allowlist and from what any other child of Zone's inherits.
    const GAINED: [&str; 4] = [
        "LOGNAME",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_DIR",
        "SSL_CERT_FILE",
    ];

    /// The locale categories any other child of Zone's inherits, allowed by
    /// name now; Zone let each through whenever it was set, as the policy does.
    const CATEGORIES: [&str; 12] = [
        "LC_ADDRESS",
        "LC_ALL",
        "LC_COLLATE",
        "LC_IDENTIFICATION",
        "LC_MEASUREMENT",
        "LC_MESSAGES",
        "LC_MONETARY",
        "LC_NAME",
        "LC_NUMERIC",
        "LC_PAPER",
        "LC_TELEPHONE",
        "LC_TIME",
    ];

    /// What `rg` and git gain over what they inherited before, which was
    /// what any other child of Zone's inherits.
    const GAINED_BY_SEARCH: [&str; 2] = ["TOOL_RUNNER_PROXY_URL", "LC_SNEAKY"];

    /// What `sh` sets for itself while it runs a stand-in's script.
    const SHELL_OWN: [&str; 3] = ["PWD", "SHLVL", "_"];

    /// What [`zone_routing::proxy`] lays over every child.
    const ROUTED: [&str; 10] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        PROXY_URL_VARIABLE,
        PROXY_BYPASS_VARIABLE,
    ];

    const PROXY: &str = "http://127.0.0.1:28888";

    /// Set on the copy of this binary the search runs in.
    const COPY: &str = "ZONE_SERVER_TOOL_ENVIRONMENT_COPY";

    /// The file a stand-in records its environment in.
    const RECORD: &str = "environment";

    /// A repository the copy's environment names, which git is never asked
    /// about even when the operator passes it through.
    const REPOSITORY: (&str, &str) = ("GIT_DIR", "/notreal/repository.git");

    fn variables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn named(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    fn server() -> Vec<(String, String)> {
        let mut server = variables(&LOCALES);
        server.extend(variables(&SERVER_SECRETS));
        server.push((PASSED_THROUGH.0.to_string(), PASSED_THROUGH.1.to_string()));
        server
    }

    #[test]
    fn the_tool_environment_is_exactly_the_safe_union() {
        let policy = from(server(), &named(&[PASSED_THROUGH.0]));

        assert_eq!(
            policy,
            EnvironmentPolicy::empty()
                .allow(ZONE.iter().chain(&GAINED).chain(&CATEGORIES).copied())
        );
        assert_eq!(
            policy,
            EnvironmentPolicy::empty()
                .allow(cli::inheritable(&named(&[PASSED_THROUGH.0])))
                .allow(GAINED_BY_SEARCH)
        );
    }

    #[test]
    fn a_credential_never_reaches_a_tool() {
        let credentials = [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "OPENAI_API_KEY",
            Toolset::TOKEN_VARIABLE,
        ];

        let listed = from(server(), &named(&credentials));
        let unlisted = from(variables(&LOCALES), &[]);

        assert_eq!(listed, unlisted);
        for name in credentials
            .into_iter()
            .chain(SERVER_SECRETS.map(|(name, _)| name))
        {
            assert_ne!(
                unlisted.clone().allow([name]),
                unlisted,
                "{name} reaches a tool"
            );
        }
    }

    fn stand_in(directory: &Path, program: &str) {
        let script = directory.join(program);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n/usr/bin/env -0 >> '{}'\nexit 0\n",
                directory.join(RECORD).display()
            ),
        )
        .expect("the stand-in's script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("an executable stand-in");
    }

    fn recorded(directory: &Path) -> Vec<(String, String)> {
        let record = std::fs::read(directory.join(RECORD)).expect("the stand-in ran");
        String::from_utf8_lossy(&record)
            .split_terminator('\0')
            .filter_map(|entry| entry.split_once('='))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn copied(directory: &Path, session: Session) -> ToolContext {
        let checkout = directory.join("checkout");
        std::fs::create_dir(&checkout).expect("the copy's checkout");
        super::super::context::build(super::super::context::Config {
            working_directory: checkout,
            session,
            denied: Vec::new(),
            unrestricted: true,
            maximum_file_size: super::super::MAX_TOOL_FILE_BYTES,
            command_timeout: Duration::from_secs(super::super::TOOL_COMMAND_TIMEOUT_SECS),
            chat_offline: false,
        })
    }

    /// That `program`, as the stand-in recorded it, was handed `context`'s
    /// environment, less `withheld`, and nothing else a tool may not see.
    fn assert_handed_the_tool_environment(
        program: &str,
        directory: &Path,
        context: &ToolContext,
        withheld: &[&str],
    ) {
        let recorded = recorded(directory);
        let names: BTreeSet<&str> = recorded.iter().map(|(name, _)| name.as_str()).collect();
        let policy: BTreeSet<String> = context
            .environment
            .inherited()
            .into_keys()
            .filter_map(|name| name.into_string().ok())
            .collect();
        for name in withheld {
            assert!(policy.contains(*name), "the policy never hands {name}");
        }
        for name in &policy {
            if withheld.contains(&name.as_str()) {
                assert!(
                    !names.contains(name.as_str()),
                    "{program} was handed {name}"
                );
            } else {
                assert!(
                    names.contains(name.as_str()),
                    "{program} was not handed {name}"
                );
            }
        }
        for name in &names {
            assert!(
                policy.contains(*name) || ROUTED.contains(name) || SHELL_OWN.contains(name),
                "{program} was handed {name}, which the policy refuses"
            );
        }
        for (name, _) in SERVER_SECRETS {
            assert!(!names.contains(name), "{program} was handed {name}");
        }
        for name in GAINED_BY_SEARCH.into_iter().chain([PASSED_THROUGH.0]) {
            assert!(names.contains(name), "{program} was not handed {name}");
        }
    }

    /// Run `test` in a copy of this binary whose environment holds the
    /// server's secrets and a repository of its own, with a stand-in for
    /// `program` first on its `PATH`, since setting them here would race
    /// every other test that starts a child.
    async fn in_a_copy(test: &str, program: &str) {
        let directory = tempdir().unwrap();
        stand_in(directory.path(), program);
        let path = std::env::join_paths(std::iter::once(directory.path().to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .expect("a PATH with the stand-in first");
        let output =
            tokio::process::Command::new(std::env::current_exe().expect("this test binary"))
                .args(["--exact", test, "--nocapture"])
                .env(COPY, directory.path())
                .envs(SERVER_SECRETS)
                .envs(LOCALES)
                .env(
                    cli::PASSTHROUGH,
                    format!("{},{}", PASSED_THROUGH.0, REPOSITORY.0),
                )
                .envs([PASSED_THROUGH, REPOSITORY])
                .env(zone_routing::PROXY_URL_VARIABLE, PROXY)
                .env("PATH", path)
                .output()
                .await
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

    #[tokio::test]
    async fn search_hands_rg_the_tool_environment() {
        const TEST: &str = "agent::tools::environment::tests::search_hands_rg_the_tool_environment";
        let Some(directory) = std::env::var_os(COPY).map(PathBuf::from) else {
            return in_a_copy(TEST, "rg").await;
        };
        let context = copied(&directory, Session::Detached);

        SearchCodeTool
            .execute(json!({"pattern": "anything"}), &context)
            .await
            .expect("a search");

        assert_handed_the_tool_environment("rg", &directory, &context, &[]);
    }

    #[tokio::test]
    async fn a_run_asks_git_for_its_exclude_in_the_tool_environment() {
        const TEST: &str = "agent::tools::environment::tests::a_run_asks_git_for_its_exclude_in_the_tool_environment";
        let Some(directory) = std::env::var_os(COPY).map(PathBuf::from) else {
            return in_a_copy(TEST, "git").await;
        };
        let session = Session::Task(Uuid::new_v4());
        let context = copied(&directory, session);

        Jobs::spawn(&JobCommand::new("true", Vec::new()), &context)
            .await
            .expect("a job keyed to the run");
        Jobs::kill_session(session).await;

        assert_handed_the_tool_environment("git", &directory, &context, &[REPOSITORY.0]);
    }

    /// Set on the copy of this binary that reads its own environment.
    const PASSED_THROUGH_COPY: &str = "ZONE_SERVER_TOOL_ENV_COPY";

    const PASSED_THROUGH_CREDENTIALS: [(&str, &str); 2] = [
        ("ANTHROPIC_API_KEY", "sk-ant-notrealkey"),
        (Toolset::TOKEN_VARIABLE, "zone-mcp-notrealtoken"),
    ];

    const SECRETS: &[(&str, &str)] = &[
        ("DATABASE_URL", "postgres://zone:hunter2@postgres:5432/zone"),
        ("JWT_SECRET", "a-signing-key-of-at-least-32-characters"),
        ("ENCRYPTION_KEY", "12345678901234567890123456789012"),
        ("SECURITY_LITELLM_MASTER_KEY", "sk-master"),
        ("SECURITY_MANAGER_API_KEY", "manager-key"),
        ("LITELLM_KEY", "sk-litellm"),
        ("POSTGRES_PASSWORD", "hunter2"),
        ("OPENAI_API_KEY", "sk-provider"),
    ];

    /// Whether `policy` hands a child `name` whenever this process has it set.
    fn allows(policy: &EnvironmentPolicy, name: &str) -> bool {
        policy.clone().allow([name]) == *policy
    }

    #[test]
    fn the_policy_keeps_a_working_shell_and_drops_every_secret() {
        let mut server = variables(&[
            ("PATH", "/usr/local/bin:/usr/bin"),
            ("HOME", "/home/zone"),
            ("USER", "zone"),
            ("SHELL", "/bin/sh"),
            ("LANG", "en_NZ.UTF-8"),
            ("LC_CTYPE", "en_NZ.UTF-8"),
            ("TERM", "xterm-256color"),
            ("TZ", "Pacific/Auckland"),
            ("TMPDIR", "/tmp"),
            ("HTTPS_PROXY", "http://127.0.0.1:28888"),
            ("no_proxy", "localhost,127.0.0.1"),
            ("TOOL_RUNNER_PROXY_URL", "http://127.0.0.1:28888"),
        ]);
        server.extend(variables(SECRETS));

        let policy = from(server, &[]);

        for name in [
            "PATH",
            "HOME",
            "USER",
            "SHELL",
            "LANG",
            "LC_CTYPE",
            "TERM",
            "TZ",
            "TMPDIR",
            "HTTPS_PROXY",
            "no_proxy",
            "TOOL_RUNNER_PROXY_URL",
        ] {
            assert!(allows(&policy, name), "{name} must reach a tool");
        }
        for (name, _) in SECRETS {
            assert!(!allows(&policy, name), "{name} must not reach a tool");
        }
    }

    #[test]
    fn the_policy_honours_the_operator_passthrough() {
        let server = variables(&[
            ("CARGO_HOME", "/opt/cargo"),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("JWT_SECRET", "a-signing-key-of-at-least-32-characters"),
        ]);

        let policy = from(server, &named(&["CARGO_HOME", "SSH_AUTH_SOCK"]));

        assert!(allows(&policy, "CARGO_HOME"));
        assert!(allows(&policy, "SSH_AUTH_SOCK"));
        assert!(!allows(&policy, "JWT_SECRET"));
    }

    #[test]
    fn the_policy_passthrough_is_exact_not_a_prefix() {
        let server = variables(&[("JWT_SECRET", "s"), ("JWT", "s")]);
        let policy = from(server, &named(&["JWT"]));
        assert!(allows(&policy, "JWT"));
        assert!(!allows(&policy, "JWT_SECRET"));
    }

    #[test]
    fn the_policy_drops_a_credential_however_the_operator_names_it() {
        let names: Vec<&str> = PASSED_THROUGH_CREDENTIALS
            .iter()
            .map(|(name, _)| *name)
            .collect();

        let policy = from(variables(&PASSED_THROUGH_CREDENTIALS), &named(&names));

        assert_eq!(policy, from(Vec::new(), &[]));
    }

    /// The copy runs with the variables set and reads them through [`policy`], so nothing
    /// here calls `set_var` on a process whose other tests start children.
    #[test]
    fn a_credential_the_operator_passes_through_never_reaches_a_tool() {
        if std::env::var_os(PASSED_THROUGH_COPY).is_some() {
            let environment = policy().inherited();
            for (name, _) in PASSED_THROUGH_CREDENTIALS {
                assert!(
                    !environment.contains_key(OsStr::new(name)),
                    "{name} reached a tool"
                );
            }
            let (name, value) = PASSED_THROUGH;
            assert_eq!(
                environment
                    .get(OsStr::new(name))
                    .and_then(|passed| passed.to_str()),
                Some(value)
            );
            return;
        }

        let listed: Vec<&str> = PASSED_THROUGH_CREDENTIALS
            .iter()
            .chain([&PASSED_THROUGH])
            .map(|(name, _)| *name)
            .collect();
        let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
            .args([
                "--exact",
                "agent::tools::environment::tests::a_credential_the_operator_passes_through_never_reaches_a_tool",
                "--nocapture",
            ])
            .env(PASSED_THROUGH_COPY, "1")
            .envs(PASSED_THROUGH_CREDENTIALS)
            .envs([PASSED_THROUGH])
            .env(cli::PASSTHROUGH, listed.join(","))
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
}
