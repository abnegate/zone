//! How a run is held apart from the host's own configuration, and which of
//! the agent's own tools it keeps.

use abnegate_agent_cli::{CodexSandbox, Session};

use super::*;
use crate::llm::cli::confined;

const ENDPOINT: &str = "http://127.0.0.1:8421/mcp";
const THREAD: &str = "019b2c41-0000-7000-8000-000000000001";

/// What an isolated Claude run is given so it neither updates itself, nor
/// phones home, nor remembers across runs.
const ISOLATION: [(&str, &str); 3] = [
    ("DISABLE_AUTOUPDATER", "1"),
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
];

/// Claude Code's waits on a call to an MCP tool.
const TOOL_TIMEOUTS: [&str; 2] = ["MCP_TOOL_TIMEOUT", "CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT"];

/// Thirty minutes, as Claude Code is told it, in milliseconds.
const THIRTY_MINUTES: &str = "1800000";

fn toolset() -> Toolset {
    Toolset::new(ENDPOINT, "zone-turn-notrealtoken", ["read_file"])
}

/// A turn of `agent` under `settings`, served zone's toolset with
/// `builtin_tools` when given, run to its end by a stand-in that answers it.
/// A turn served no toolset withholds the agent's own tools.
async fn ran(
    agent: AgentKind,
    configure: impl FnOnce(CliSettings) -> CliSettings,
    served: Option<BuiltinTools>,
) -> StandIn {
    let stand_in = answering(agent);
    let client = running(agent, configure(standing_in(&stand_in)));
    let client = match served {
        Some(builtin_tools) => client.with_toolset(toolset(), builtin_tools),
        None => client,
    };
    let (_, failure) = asked(&client, agent).await;
    assert!(failure.is_none(), "{agent}: {failure:?}");
    stand_in
}

/// Every way a turn is served: no toolset, and zone's toolset with the
/// agent's own tools withheld or granted.
const SERVED: [Option<BuiltinTools>; 3] = [
    None,
    Some(BuiltinTools::Withheld),
    Some(BuiltinTools::Granted),
];

fn adjacent(arguments: &[String], pair: [&str; 2]) -> bool {
    arguments.windows(2).any(|window| window == pair)
}

#[tokio::test]
async fn every_run_is_isolated() {
    for served in SERVED {
        let stand_in = ran(AgentKind::Claude, |settings| settings, served).await;

        let arguments = stand_in.arguments();
        assert!(
            adjacent(&arguments, ["--setting-sources", ""]),
            "{served:?}: {arguments:?}"
        );
        assert!(
            adjacent(
                &arguments,
                ["--settings", r#"{"crossSessionInbound":"refuse"}"#]
            ),
            "{served:?}: {arguments:?}"
        );
        let environment = stand_in.environment();
        for (name, value) in ISOLATION {
            assert_eq!(
                environment.get(name).map(String::as_str),
                Some(value),
                "{served:?}: {name}"
            );
        }
    }
}

#[tokio::test]
async fn a_run_without_a_toolset_sets_no_mcp_tool_timeout() {
    let claude = ran(AgentKind::Claude, |settings| settings, None).await;
    let environment = claude.environment();
    for name in TOOL_TIMEOUTS {
        assert!(
            !environment.contains_key(name),
            "{name} was set on a run with no MCP server"
        );
    }

    let codex = ran(AgentKind::Codex, |settings| settings, None).await;
    let arguments = codex.arguments();
    assert!(
        arguments
            .iter()
            .all(|argument| !argument.contains("tool_timeout_sec")),
        "{arguments:?}"
    );
}

#[tokio::test]
async fn a_toolset_run_waits_thirty_minutes_on_a_tool() {
    for builtin_tools in [BuiltinTools::Withheld, BuiltinTools::Granted] {
        let claude = ran(AgentKind::Claude, |settings| settings, Some(builtin_tools)).await;
        let environment = claude.environment();
        for name in TOOL_TIMEOUTS {
            assert_eq!(
                environment.get(name).map(String::as_str),
                Some(THIRTY_MINUTES),
                "{builtin_tools:?}: {name}"
            );
        }

        let codex = ran(AgentKind::Codex, |settings| settings, Some(builtin_tools)).await;
        let server = zones_server(&codex.arguments());
        assert!(server.contains(r#""tool_timeout_sec" = 1800"#), "{server}");
        assert!(server.contains(r#""startup_timeout_sec" = 30"#), "{server}");
    }
}

#[tokio::test]
async fn a_withheld_codex_run_is_read_only_fresh_and_resumed() {
    for served in [None, Some(BuiltinTools::Withheld)] {
        let fresh = ran(
            AgentKind::Codex,
            |settings| settings.with_sandbox(CodexSandbox::DangerFullAccess),
            served,
        )
        .await;
        let arguments = fresh.arguments();
        assert_eq!(
            fresh.argument("--sandbox").as_deref(),
            Some("read-only"),
            "{served:?}: {arguments:?}"
        );
        assert!(
            arguments
                .iter()
                .all(|argument| !argument.contains("danger-full-access")),
            "{served:?}: {arguments:?}"
        );

        let resumed = ran(
            AgentKind::Codex,
            |settings| {
                settings
                    .with_sandbox(CodexSandbox::DangerFullAccess)
                    .with_session(Session::resumed(THREAD))
            },
            served,
        )
        .await;
        let arguments = resumed.arguments();
        assert!(
            adjacent(&arguments, ["-c", r#"sandbox_mode="read-only""#]),
            "{served:?}: {arguments:?}"
        );
        assert!(adjacent(&arguments, ["resume", THREAD]), "{arguments:?}");
        assert!(
            !arguments.iter().any(|argument| argument == "--sandbox"),
            "codex exec resume takes no --sandbox: {arguments:?}"
        );
    }
}

#[tokio::test]
async fn a_granted_codex_run_uses_the_configured_sandbox() {
    for sandbox in [CodexSandbox::WorkspaceWrite, CodexSandbox::DangerFullAccess] {
        let codex = ran(
            AgentKind::Codex,
            |settings| settings.with_sandbox(sandbox),
            Some(BuiltinTools::Granted),
        )
        .await;

        let arguments = codex.arguments();
        assert_eq!(
            codex.argument("--sandbox"),
            Some(sandbox.to_string()),
            "{arguments:?}"
        );
        assert!(
            !adjacent(&arguments, ["--disable", "shell_tool"]),
            "a granted run lost codex's own tools: {arguments:?}"
        );
    }
}

#[tokio::test]
async fn a_withheld_run_asks_no_permission_prompt() {
    for agent in AgentKind::ALL.iter().copied() {
        for served in [None, Some(BuiltinTools::Withheld)] {
            let stand_in = ran(agent, |settings| settings, served).await;

            let arguments = stand_in.arguments();
            assert!(
                !arguments
                    .iter()
                    .any(|argument| argument == "--permission-prompt-tool"),
                "{agent} {served:?}: {arguments:?}"
            );
            if agent == AgentKind::Claude {
                assert!(adjacent(&arguments, ["--tools", ""]), "{arguments:?}");
            }
        }
    }
}

/// No turn through the client is granted the agent's own tools without
/// zone's, so the run is confined here as the client confines it, and
/// started as it starts one: Claude refuses a prompt no attached server
/// can answer, so asking one would fail the turn outright.
#[tokio::test]
async fn a_granted_claude_run_without_a_toolset_asks_no_permission_prompt() {
    let stand_in = answering(AgentKind::Claude);
    let provider = CliProvider::agent(
        AgentKind::Claude,
        confined(
            AgentKind::Claude,
            standing_in(&stand_in),
            None,
            BuiltinTools::Granted,
        ),
    );

    let events = provider
        .stream(abnegate_llm::CompletionRequest::new(
            "sonnet",
            &[Message::user("What does a.rs do?")],
            RequestOptions::new(512),
        ))
        .await
        .expect("a granted run without zone's tools to start");
    let (delivered, failure) = collected(
        Turn::new(
            AgentKind::Claude,
            SignIn::Instance,
            None,
            None,
            Reported::default(),
        )
        .chunks(events),
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(reasons(&delivered), [STOP]);
    let arguments = stand_in.arguments();
    assert!(
        !arguments
            .iter()
            .any(|argument| argument == "--permission-prompt-tool"),
        "{arguments:?}"
    );
    assert!(
        !adjacent(&arguments, ["--tools", ""]),
        "a granted run lost claude's own tools: {arguments:?}"
    );
}

#[tokio::test]
async fn zone_passes_the_agent_no_arguments_of_its_own() {
    const RENDERED: &str = "<rendered>";
    let streaming = ["--verbose", "--output-format", "stream-json"];
    let isolated = [
        "--setting-sources",
        "",
        "--settings",
        r#"{"crossSessionInbound":"refuse"}"#,
    ];
    let prompted = ["--model", "sonnet", "--print"];
    let zones_tool = AgentKind::Claude.tool(Toolset::SERVER, "read_file");
    let permission_tool = AgentKind::Claude.tool(Toolset::SERVER, Toolset::PERMISSION);
    let attached = ["--mcp-config", RENDERED, "--strict-mcp-config"];

    for (served, expected) in [
        (
            None,
            [
                &streaming[..],
                &["--strict-mcp-config"],
                &isolated,
                &["--tools", ""],
                &prompted,
            ]
            .concat(),
        ),
        (
            Some(BuiltinTools::Withheld),
            [
                &streaming[..],
                &attached,
                &isolated,
                &["--tools", ""],
                &["--allowedTools", &zones_tool],
                &prompted,
            ]
            .concat(),
        ),
        (
            Some(BuiltinTools::Granted),
            [
                &streaming[..],
                &attached,
                &isolated,
                &["--allowedTools", &zones_tool],
                &["--permission-prompt-tool", &permission_tool],
                &prompted,
            ]
            .concat(),
        ),
    ] {
        let stand_in = ran(AgentKind::Claude, |settings| settings, served).await;

        let mut arguments = stand_in.arguments();
        if let Some(index) = arguments
            .iter()
            .position(|argument| argument == "--mcp-config")
        {
            arguments[index + 1] = RENDERED.to_string();
        }
        assert_eq!(arguments, expected, "{served:?}");
    }
}
