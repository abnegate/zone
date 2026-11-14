use abnegate_agent_cli::{AgentKind, CliSettings};

use super::{BuiltinTools, Toolset, passthrough};

/// The gateways an agent signs in through, which the crate counts among its
/// credentials. One an operator names in the passthrough is set on the run
/// explicitly, since a passthrough never hands an agent a credential.
pub const BASE_URLS: &[&str] = &["ANTHROPIC_BASE_URL", "OPENAI_BASE_URL"];

/// `settings` confined as every run Zone starts is: none of the host's own
/// sign-in, only the operator's passthrough beside the allowlist, apart from
/// whatever the host configured for the agent, with Zone's `toolset`
/// attached and the agent's own tools kept only when `builtin_tools` grants
/// them.
pub fn confined(
    agent: AgentKind,
    settings: CliSettings,
    toolset: Option<&Toolset>,
    builtin_tools: BuiltinTools,
) -> CliSettings {
    confine(
        agent,
        settings,
        toolset,
        builtin_tools,
        &passthrough(),
        &|name| std::env::var(name).ok(),
    )
}

fn confine(
    agent: AgentKind,
    settings: CliSettings,
    toolset: Option<&Toolset>,
    builtin_tools: BuiltinTools,
    passthrough: &[String],
    host: &dyn Fn(&str) -> Option<String>,
) -> CliSettings {
    let mut settings = settings
        .withhold_host_credentials()
        .refuse_credentials()
        .with_proxy_variables()
        .allow(passthrough.iter().cloned())
        .isolate();

    for name in passthrough
        .iter()
        .filter(|name| BASE_URLS.contains(&name.as_str()))
    {
        if let Some(value) = host(name) {
            settings = settings.with_variable(name.as_str(), value);
        }
    }

    if let Some(toolset) = toolset {
        settings = settings.with_mcp_server(Toolset::SERVER, toolset.server());
    }

    match builtin_tools {
        BuiltinTools::Withheld => {
            settings.sandbox = None;
            settings.withhold_tools()
        }
        BuiltinTools::Granted if agent == AgentKind::Claude && toolset.is_some() => {
            settings.with_permission_prompt(Toolset::SERVER, Toolset::PERMISSION)
        }
        BuiltinTools::Granted => settings,
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::CodexSandbox;

    use super::*;
    use crate::llm::cli::settings as zone;

    const GATEWAY: &str = "https://gateway.internal";

    fn toolset() -> Toolset {
        Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            ["read_file"],
        )
    }

    fn confined_with(
        agent: AgentKind,
        settings: CliSettings,
        toolset: Option<&Toolset>,
        builtin_tools: BuiltinTools,
        passthrough: &[&str],
    ) -> CliSettings {
        let passthrough: Vec<String> = passthrough.iter().map(|name| (*name).to_string()).collect();
        confine(
            agent,
            settings,
            toolset,
            builtin_tools,
            &passthrough,
            &|name| BASE_URLS.contains(&name).then(|| GATEWAY.to_string()),
        )
    }

    #[test]
    fn every_run_withholds_the_hosts_sign_in_and_allows_only_the_passthrough() {
        let settings = confined_with(
            AgentKind::Claude,
            zone(),
            None,
            BuiltinTools::Granted,
            &["CORPORATE_CA"],
        );

        assert!(settings.withhold_host_credentials);
        assert!(settings.refuse_credentials);
        assert!(settings.isolated);
        assert!(settings.allowed.contains("CORPORATE_CA"));
        assert!(settings.allowed.contains("HTTPS_PROXY"));
        assert!(!settings.inherit_environment);
    }

    #[test]
    fn a_base_url_in_the_passthrough_is_set_on_the_run() {
        let settings = confined_with(
            AgentKind::Claude,
            zone(),
            None,
            BuiltinTools::Granted,
            &["ANTHROPIC_BASE_URL", "CORPORATE_CA"],
        );

        assert_eq!(
            settings
                .variables
                .get("ANTHROPIC_BASE_URL")
                .map(String::as_str),
            Some(GATEWAY)
        );
        assert!(!settings.variables.contains_key("CORPORATE_CA"));
    }

    #[test]
    fn a_base_url_left_out_of_the_passthrough_is_not_set_on_the_run() {
        for agent in AgentKind::ALL.iter().copied() {
            let settings = confined_with(
                agent,
                zone(),
                None,
                BuiltinTools::Granted,
                &["ANTHROPIC_BASE_URL"],
            );

            assert_eq!(
                settings
                    .variables
                    .get("ANTHROPIC_BASE_URL")
                    .map(String::as_str),
                Some(GATEWAY),
                "{agent}"
            );
            assert!(
                !settings.variables.contains_key("OPENAI_BASE_URL"),
                "{agent}"
            );
            assert!(!settings.allowed.contains("OPENAI_BASE_URL"), "{agent}");
        }
    }

    #[test]
    fn a_withheld_run_drops_the_sandbox_and_asks_no_permission() {
        for agent in AgentKind::ALL.iter().copied() {
            let settings = confined_with(
                agent,
                zone().with_sandbox(CodexSandbox::DangerFullAccess),
                Some(&toolset()),
                BuiltinTools::Withheld,
                &[],
            );

            assert!(settings.withhold_tools, "{agent}");
            assert_eq!(settings.sandbox, None, "{agent}");
            assert_eq!(settings.permission_prompt, None, "{agent}");
        }
    }

    #[test]
    fn a_granted_run_keeps_its_sandbox_and_only_claude_with_zones_tools_asks_zone() {
        let granted = |agent, toolset: Option<&Toolset>| {
            confined_with(
                agent,
                zone().with_sandbox(CodexSandbox::WorkspaceWrite),
                toolset,
                BuiltinTools::Granted,
                &[],
            )
        };

        let claude = granted(AgentKind::Claude, Some(&toolset()));
        assert!(!claude.withhold_tools);
        assert_eq!(
            claude.permission_prompt,
            Some(AgentKind::Claude.tool(Toolset::SERVER, Toolset::PERMISSION))
        );
        assert_eq!(granted(AgentKind::Claude, None).permission_prompt, None);

        let codex = granted(AgentKind::Codex, Some(&toolset()));
        assert_eq!(codex.sandbox, Some(CodexSandbox::WorkspaceWrite));
        assert_eq!(codex.permission_prompt, None);
    }
}
