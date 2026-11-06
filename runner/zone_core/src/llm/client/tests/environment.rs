//! What a spawned agent finds in its environment: the allowlist and the
//! operator's passthrough, never the host's own sign-in.

use std::collections::BTreeMap;

use abnegate_secret::REDACTED;

use super::*;
use crate::llm::cli::PASSTHROUGH;

const CORPORATE_CA: (&str, &str) = ("CORPORATE_CA", "/etc/ssl/corporate.pem");
const PROXY: (&str, &str) = ("HTTPS_PROXY", "http://proxy.internal:3128");

/// The host's own sign-in, every value of which says `notreal`, so a leak
/// shows under any name.
const HOST_SIGN_IN: [(&str, &str); 11] = [
    ("ANTHROPIC_API_KEY", "sk-ant-notreal-host"),
    ("ANTHROPIC_AUTH_TOKEN", "notreal-auth-host"),
    (
        "ANTHROPIC_BASE_URL",
        "https://notreal-host-gateway.internal",
    ),
    ("CLAUDE_CODE_OAUTH_TOKEN", "notreal-oauth-host"),
    ("CLAUDE_CODE_OAUTH_REFRESH_TOKEN", "notreal-refresh-host"),
    ("OPENAI_API_KEY", "sk-notreal-openai-host"),
    ("OPENAI_BASE_URL", "https://notreal-host-openai.internal"),
    ("CODEX_API_KEY", "notreal-codex-host"),
    ("AWS_SECRET_ACCESS_KEY", "notreal-aws-host"),
    ("CLAUDE_CONFIG_DIR", "/notreal/host/claude"),
    ("CODEX_HOME", "/notreal/host/codex"),
];

/// Every agent's environment after a turn of it under Zone's confinement.
async fn handed() -> Vec<(AgentKind, BTreeMap<String, String>)> {
    let mut handed = Vec::new();
    for agent in AgentKind::ALL.iter().copied() {
        let stand_in = answering(agent);
        let (_, failure) = asked(&running(agent, standing_in(&stand_in)), agent).await;
        assert!(failure.is_none(), "{agent}: {failure:?}");
        handed.push((agent, stand_in.environment()));
    }
    handed
}

#[tokio::test]
async fn confined_runs_withhold_and_refuse_host_credentials_and_allow_the_passthrough() {
    let mut host = vec![(PASSTHROUGH, CORPORATE_CA.0), CORPORATE_CA, PROXY];
    host.extend(HOST_SIGN_IN);
    if delegated(
        module_path!(),
        "confined_runs_withhold_and_refuse_host_credentials_and_allow_the_passthrough",
        &host,
    )
    .await
    {
        return;
    }

    for (agent, environment) in handed().await {
        for (name, value) in [CORPORATE_CA, PROXY] {
            assert_eq!(
                environment.get(name).map(String::as_str),
                Some(value),
                "{agent} was not handed {name}"
            );
        }
        for (name, value) in HOST_SIGN_IN {
            assert!(
                !environment.contains_key(name),
                "{agent} was handed the host's {name}"
            );
            assert!(
                environment.values().all(|handed| !handed.contains(value)),
                "{agent} was handed the host's {name} under another name"
            );
        }
    }
}

#[tokio::test]
async fn a_passthrough_naming_zone_mcp_token_or_a_credential_never_reaches_the_agent() {
    const HOST_TOKEN: &str = "zone-notreal-host-token";
    const TURN_TOKEN: &str = "zone-turn-notrealtoken";
    let named = format!(
        "{},ANTHROPIC_API_KEY,OPENAI_API_KEY,{}",
        Toolset::TOKEN_VARIABLE,
        CORPORATE_CA.0
    );
    if delegated(
        module_path!(),
        "a_passthrough_naming_zone_mcp_token_or_a_credential_never_reaches_the_agent",
        &[
            (PASSTHROUGH, &named),
            (Toolset::TOKEN_VARIABLE, HOST_TOKEN),
            ("ANTHROPIC_API_KEY", "sk-ant-notreal-host"),
            ("OPENAI_API_KEY", "sk-notreal-openai-host"),
            CORPORATE_CA,
        ],
    )
    .await
    {
        return;
    }

    for agent in AgentKind::ALL.iter().copied() {
        let stand_in = answering(agent);
        let client = running(agent, standing_in(&stand_in)).with_toolset(
            Toolset::new("http://127.0.0.1:8421/mcp", TURN_TOKEN, ["read_file"]),
            BuiltinTools::Withheld,
        );

        let (_, failure) = asked(&client, agent).await;

        assert!(failure.is_none(), "{agent}: {failure:?}");
        let environment = stand_in.environment();
        for name in [
            Toolset::TOKEN_VARIABLE,
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
        ] {
            assert!(
                !environment.contains_key(name),
                "{agent} was handed {name} from the passthrough"
            );
        }
        assert!(
            environment
                .values()
                .all(|value| !value.contains(HOST_TOKEN)),
            "{agent} was handed the host's {}",
            Toolset::TOKEN_VARIABLE
        );
        assert_eq!(
            environment.get(CORPORATE_CA.0).map(String::as_str),
            Some(CORPORATE_CA.1),
            "{agent} lost the rest of the passthrough"
        );
    }
}

#[tokio::test]
async fn only_the_posix_locale_categories_reach_the_agent() {
    const LOCALES: [(&str, &str); 4] = [
        ("LC_TIME", "en_GB.UTF-8"),
        ("LC_MESSAGES", "en_GB.UTF-8"),
        ("LC_SNEAKY", "notreal-locale"),
        ("LC_TOKEN", "notreal-token"),
    ];
    const POSIX: [&str; 13] = [
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
    if delegated(
        module_path!(),
        "only_the_posix_locale_categories_reach_the_agent",
        &LOCALES,
    )
    .await
    {
        return;
    }

    for (agent, environment) in handed().await {
        let locales: Vec<&str> = environment
            .keys()
            .map(String::as_str)
            .filter(|name| name.starts_with("LC_"))
            .collect();

        assert!(
            locales.iter().all(|name| POSIX.contains(name)),
            "{agent} was handed {locales:?}"
        );
        for (name, value) in &LOCALES[..2] {
            assert_eq!(
                environment.get(*name).map(String::as_str),
                Some(*value),
                "{agent} lost {name}"
            );
        }
    }
}

#[tokio::test]
async fn a_passed_through_value_is_scrubbed_from_the_agents_failure() {
    const SECRET: (&str, &str) = ("CORPORATE_TOKEN", "corp-notreal-4f9a2c71");
    if delegated(
        module_path!(),
        "a_passed_through_value_is_scrubbed_from_the_agents_failure",
        &[(PASSTHROUGH, SECRET.0), SECRET],
    )
    .await
    {
        return;
    }
    let stand_in = StandIn::new(&format!(
        r#"echo "the proxy refused ${name}" >&2
printf '{{"type":"result","subtype":"error_during_execution","is_error":true,"result":"the gateway refused %s"}}\n' "${name}"
exit 1"#,
        name = SECRET.0
    ));

    let (_, failure) = asked(
        &running(AgentKind::Claude, standing_in(&stand_in)),
        AgentKind::Claude,
    )
    .await;

    assert_eq!(
        stand_in.environment().get(SECRET.0).map(String::as_str),
        Some(SECRET.1),
        "the agent was never handed the passthrough"
    );
    let failure = failure.expect("the turn to fail").to_string();
    assert!(!failure.contains(SECRET.1), "{failure}");
    assert!(failure.contains("the gateway refused"), "{failure}");
    assert!(failure.contains("the proxy refused"), "{failure}");
    assert!(failure.contains(REDACTED), "{failure}");
}

#[tokio::test]
async fn a_passed_through_base_url_is_set_as_a_variable() {
    const GATEWAYS: [(&str, &str); 2] = [
        ("ANTHROPIC_BASE_URL", "https://gateway.internal/anthropic"),
        ("OPENAI_BASE_URL", "https://gateway.internal/openai"),
    ];
    let named = format!("{},{}", GATEWAYS[0].0, GATEWAYS[1].0);
    let mut host = vec![(PASSTHROUGH, named.as_str())];
    host.extend(GATEWAYS);
    if delegated(
        module_path!(),
        "a_passed_through_base_url_is_set_as_a_variable",
        &host,
    )
    .await
    {
        return;
    }

    for (agent, environment) in handed().await {
        let (name, value) = match agent {
            AgentKind::Codex => GATEWAYS[1],
            _ => GATEWAYS[0],
        };
        assert_eq!(
            environment.get(name).map(String::as_str),
            Some(value),
            "{agent} was not pointed at the operator's gateway"
        );
    }
}

#[tokio::test]
async fn config_dir_named_in_the_passthrough_reaches_a_host_run() {
    const HOMES: [(&str, &str); 2] = [
        ("CLAUDE_CONFIG_DIR", "/srv/zone/host/claude"),
        ("CODEX_HOME", "/srv/zone/host/codex"),
    ];
    let named = format!("{},{}", HOMES[0].0, HOMES[1].0);
    let mut host = vec![(PASSTHROUGH, named.as_str())];
    host.extend(HOMES);
    if delegated(
        module_path!(),
        "config_dir_named_in_the_passthrough_reaches_a_host_run",
        &host,
    )
    .await
    {
        return;
    }

    for agent in AgentKind::ALL.iter().copied() {
        let stand_in = answering(agent);
        let client = LlmClient::new(LlmConfig::default()).with_backend(
            LlmBackend::cli(agent, standing_in(&stand_in)).with_sign_in(SignIn::Host),
        );

        let (_, failure) = asked(&client, agent).await;

        assert!(failure.is_none(), "{agent}: {failure:?}");
        let (name, value) = match agent {
            AgentKind::Codex => HOMES[1],
            _ => HOMES[0],
        };
        assert_eq!(
            stand_in.environment().get(name).map(String::as_str),
            Some(value),
            "{agent} was not pointed at the host's login"
        );
    }
}
