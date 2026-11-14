//! How zone's tools reach the agent: one MCP server, attached the way each
//! agent attaches one, with the turn's token kept off its command line.

use super::*;

const ENDPOINT: &str = "http://127.0.0.1:8421/mcp";
const TOKEN: &str = "zone-turn-5c1d7a90e3b24f68";
const TOOLS: [&str; 2] = ["read_file", "list_files"];

/// How the agent is told to authenticate to zone's server.
const BEARER: &str = "Bearer ";

/// Where Codex is told to read the authorization header from.
const CODEX_HEADER: &str = r#""env_http_headers" = { "Authorization" = ""#;

fn toolset() -> Toolset {
    Toolset::new(ENDPOINT, TOKEN, TOOLS)
}

/// A turn of `agent` served zone's tools, with `builtin_tools`, run to its
/// end by a stand-in that answers it.
async fn served(agent: AgentKind, builtin_tools: BuiltinTools) -> StandIn {
    let stand_in = answering(agent);
    let client = running(agent, standing_in(&stand_in)).with_toolset(toolset(), builtin_tools);
    let (_, failure) = asked(&client, agent).await;
    assert!(failure.is_none(), "{agent}: {failure:?}");
    stand_in
}

/// The variable a `${NAME}` reference names.
fn referenced(reference: &str) -> &str {
    reference
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or_else(|| panic!("{reference} refers to no variable"))
}

/// The variable Codex reads zone's authorization header from.
fn codex_header(server: &str) -> &str {
    let start = server
        .find(CODEX_HEADER)
        .map(|start| start + CODEX_HEADER.len())
        .unwrap_or_else(|| panic!("zone's server sends no header: {server}"));
    let rest = &server[start..];
    &rest[..rest.find('"').expect("a quoted variable name")]
}

#[tokio::test]
async fn the_toolset_attaches_zone_as_an_exclusive_required_eager_remote_server() {
    let claude = served(AgentKind::Claude, BuiltinTools::Granted).await;
    let configuration = claude
        .mcp_config()
        .expect("claude was pointed at zone's server");
    let servers = configuration["mcpServers"]
        .as_object()
        .expect("a map of servers");
    assert_eq!(
        servers.keys().collect::<Vec<_>>(),
        [Toolset::SERVER],
        "{configuration}"
    );
    let server = &servers[Toolset::SERVER];
    assert_eq!(server["type"], "http", "{server}");
    assert_eq!(server["url"], ENDPOINT, "{server}");
    let header = referenced(
        server["headers"]["Authorization"]
            .as_str()
            .expect("an authorization header"),
    );
    assert_eq!(
        claude.environment().get(header).map(String::as_str),
        Some(format!("{BEARER}{TOKEN}").as_str())
    );
    let arguments = claude.arguments();
    assert!(
        arguments
            .iter()
            .any(|argument| argument == "--strict-mcp-config"),
        "{arguments:?}"
    );
    let allowed: Vec<&str> = arguments
        .windows(2)
        .filter(|pair| pair[0] == "--allowedTools")
        .map(|pair| pair[1].as_str())
        .collect();
    assert_eq!(
        allowed,
        TOOLS.map(|tool| AgentKind::Claude.tool(Toolset::SERVER, tool)),
        "only the turn's own tools, never the whole server"
    );

    let codex = served(AgentKind::Codex, BuiltinTools::Granted).await;
    let server = zones_server(&codex.arguments());
    for setting in [
        format!(r#""url" = "{ENDPOINT}""#),
        r#""enabled_tools" = ["read_file", "list_files"]"#.to_string(),
        r#""required" = true"#.to_string(),
        r#""omit_tools_from" = ["deferred"]"#.to_string(),
    ] {
        assert!(server.contains(&setting), "{setting} is not in {server}");
    }
    assert_eq!(
        codex
            .environment()
            .get(codex_header(&server))
            .map(String::as_str),
        Some(format!("{BEARER}{TOKEN}").as_str())
    );
}

#[tokio::test]
async fn the_mcp_token_never_appears_on_the_agents_command_line() {
    for agent in AgentKind::ALL.iter().copied() {
        for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
            let stand_in = served(agent, builtin_tools).await;

            let arguments = stand_in.arguments();
            assert!(
                arguments.iter().all(|argument| !argument.contains(TOKEN)),
                "{agent}: {arguments:?}"
            );
            if let Some(configuration) = stand_in.mcp_config() {
                assert!(
                    !configuration.to_string().contains(TOKEN),
                    "{agent}: {configuration}"
                );
            }
            assert!(
                stand_in
                    .environment()
                    .values()
                    .any(|value| value == &format!("{BEARER}{TOKEN}")),
                "{agent} was never handed the turn's token at all"
            );
        }
    }
}

#[tokio::test]
async fn a_granted_claude_run_asks_zone_for_permission() {
    let claude = served(AgentKind::Claude, BuiltinTools::Granted).await;

    assert_eq!(
        claude.argument("--permission-prompt-tool"),
        Some(AgentKind::Claude.tool(Toolset::SERVER, Toolset::PERMISSION))
    );
}

#[tokio::test]
async fn codex_is_never_given_a_permission_prompt() {
    for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
        let codex = served(AgentKind::Codex, builtin_tools).await;

        let arguments = codex.arguments();
        assert!(
            arguments
                .iter()
                .all(|argument| !argument.contains("permission")
                    && !argument.contains(Toolset::PERMISSION)),
            "{builtin_tools:?}: {arguments:?}"
        );
    }
}

#[tokio::test]
async fn codex_pre_approves_zones_tools() {
    for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
        let codex = served(AgentKind::Codex, builtin_tools).await;

        let server = zones_server(&codex.arguments());
        assert!(
            server.contains(r#""default_tools_approval_mode" = "approve""#),
            "{builtin_tools:?}: {server}"
        );
    }
}

#[tokio::test]
async fn codex_is_told_zones_server_is_required() {
    for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
        let codex = served(AgentKind::Codex, builtin_tools).await;

        let server = zones_server(&codex.arguments());
        assert!(
            server.contains(r#""required" = true"#),
            "{builtin_tools:?}: {server}"
        );
    }
}
