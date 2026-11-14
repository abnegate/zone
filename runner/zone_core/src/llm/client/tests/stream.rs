//! How an agent's events become the chunks the agent loop reads, and who
//! else is told of them.

use abnegate_agent_cli::DEFAULT_TOOL_OUTPUT_LIMIT;
use abnegate_secret::REDACTED;

use super::logs::captured_logs;
use super::*;

const SUBAGENTS: &str = include_str!("../fixtures/claude/subagents.jsonl");

/// The most the line saying how much of a tool's output was left out takes.
const OMISSION_NOTE: usize = 256;

/// What zone logs when a turn runs on usage credits.
const ON_CREDITS: &str = "the turn runs on usage credits past the plan's limit";

/// A Claude stand-in that replays [`SUBAGENTS`].
fn delegating() -> StandIn {
    StandIn::replaying(SUBAGENTS, "")
}

/// Whether `chunk` carries anything the agent loop reads.
fn carries(chunk: &ChatStreamChunk) -> bool {
    chunk.usage.is_some()
        || chunk.window.is_some()
        || chunk.session.is_some()
        || chunk.choices.iter().any(|choice| {
            choice.delta.content.is_some()
                || choice.delta.reasoning_content.is_some()
                || choice.finish_reason.is_some()
        })
}

#[tokio::test]
async fn every_new_event_maps_or_is_ignored() {
    let claude = StandIn::new(
        r#"
echo '{"type":"system","subtype":"init","session_id":"6f1"}'
echo '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"five_hour","utilization":0.2,"isUsingOverage":true}}'
echo '{"type":"assistant","message":{"model":"claude-sonnet-4-6","role":"assistant","content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/w/a.rs"}}],"usage":{"input_tokens":3,"output_tokens":9}},"parent_tool_use_id":null}'
echo '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"fn main() {}"}]},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Looking.","num_turns":2,"total_cost_usd":0.04,"duration_api_ms":7980,"structured_output":{"ok":true},"usage":{"input_tokens":3,"output_tokens":9},"modelUsage":{"claude-sonnet-4-6":{"contextWindow":200000}}}'
"#,
    );
    let codex = StandIn::new(&format!(
        r#"echo '{{"type":"thread.started","thread_id":"t1"}}'
echo '{{"type":"error","message":"Reconnecting... 1/5"}}'
{CODEX_ANSWER}"#
    ));

    for (agent, stand_in, said) in [
        (AgentKind::Claude, claude, "Looking."),
        (AgentKind::Codex, codex, "Done."),
    ] {
        let (delivered, failure) = asked(&running(agent, standing_in(&stand_in)), agent).await;

        assert!(failure.is_none(), "{agent}: {failure:?}");
        let empty: Vec<&ChatStreamChunk> =
            delivered.iter().filter(|chunk| !carries(chunk)).collect();
        assert!(
            empty.is_empty(),
            "{agent} chunked an event zone ignores: {empty:?}"
        );
        assert_eq!(spoken(&delivered), said, "{agent}");
        assert_eq!(reasons(&delivered), [STOP], "{agent}");
        assert_eq!(
            delivered
                .iter()
                .filter(|chunk| chunk.session.is_some())
                .count(),
            1,
            "{agent}"
        );
    }
}

#[tokio::test]
async fn an_observer_is_handed_every_output_and_subagent_event() {
    let recording = Arc::new(Recording::default());
    let stand_in = delegating();
    let claude =
        running(AgentKind::Claude, standing_in(&stand_in)).with_observer(recording.clone());

    let (delivered, failure) = asked(&claude, AgentKind::Claude).await;

    assert!(failure.is_none(), "{failure:?}");
    let outputs = recording.outputs.lock().expect("outputs").clone();
    assert!(
        outputs
            .iter()
            .any(|output| output == "Command running in background with ID: c3d4e5f6a."),
        "{outputs:?}"
    );
    let subagents = recording.subagents.lock().expect("subagents").clone();
    for started in ["toolu_01Survey", "toolu_04Review"] {
        assert!(subagents.iter().any(|id| id == started), "{subagents:?}");
    }
    assert!(!reasoned(&delivered).contains("pub mod"));

    let recording = Arc::new(Recording::default());
    let stand_in = StandIn::new(&format!(
        r#"echo '{{"type":"item.completed","item":{{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"a.rs","exit_code":0,"status":"completed"}}}}'
{CODEX_ANSWER}"#
    ));
    let codex = running(AgentKind::Codex, standing_in(&stand_in)).with_observer(recording.clone());

    let (_, failure) = asked(&codex, AgentKind::Codex).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(*recording.outputs.lock().expect("outputs"), ["a.rs"]);
}

#[tokio::test]
async fn a_subagents_calls_are_reasoning_only_when_nothing_observes() {
    let (unwatched_agent, watched_agent) = (delegating(), delegating());
    let unobserved = running(AgentKind::Claude, standing_in(&unwatched_agent));
    let observed = running(AgentKind::Claude, standing_in(&watched_agent))
        .with_observer(Arc::new(Recording::default()));

    let (unwatched, failure) = asked(&unobserved, AgentKind::Claude).await;
    assert!(failure.is_none(), "{failure:?}");
    let (watched, failure) = asked(&observed, AgentKind::Claude).await;
    assert!(failure.is_none(), "{failure:?}");

    let subagents_call = "- Grep `pub mod`\n";
    let main_agents_call = "- Bash `cargo build`\n";
    assert_eq!(
        reasoned(&unwatched),
        [
            "- Agent `Survey the crate`\n",
            subagents_call,
            "- Read `/w/src/missing.rs`\n",
            "- Agent `Review the change`\n",
            main_agents_call,
            "- Bash `cargo test -p zone_core`\n",
        ]
        .concat()
    );
    assert!(
        !reasoned(&watched).contains(subagents_call),
        "{}",
        reasoned(&watched)
    );
    for delivered in [&unwatched, &watched] {
        assert!(
            reasoned(delivered).contains(main_agents_call),
            "{}",
            reasoned(delivered)
        );
    }
}

#[tokio::test]
async fn tool_output_is_capped_at_the_default_tool_output_limit() {
    const TOKEN: &str = "zone-turn-9e1f2a3b4c5d6e7f";
    const END: &str = "the-end-of-the-output";
    let printed = format!(
        "{TOKEN} {} {END}",
        "a".repeat(DEFAULT_TOOL_OUTPUT_LIMIT * 2)
    );
    let stream = [
        serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "id": "toolu_01", "name": "Bash", "input": {"command": "cat big"}}]},
            "parent_tool_use_id": null,
        }),
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_01", "content": printed}]},
            "parent_tool_use_id": null,
        }),
    ]
    .map(|line| format!("{line}\n"))
    .concat();
    let stand_in = StandIn::replaying(&stream, CLAUDE_ANSWER);
    let recording = Arc::new(Recording::default());
    let client = running(AgentKind::Claude, standing_in(&stand_in))
        .with_toolset(
            Toolset::new("http://127.0.0.1:8421/mcp", TOKEN, ["read_file"]),
            BuiltinTools::Withheld,
        )
        .with_observer(recording.clone());

    let (_, failure) = asked(&client, AgentKind::Claude).await;

    assert!(failure.is_none(), "{failure:?}");
    let outputs = recording.outputs.lock().expect("outputs").clone();
    let [output] = outputs.as_slice() else {
        panic!("expected one output, got {} of them", outputs.len());
    };
    assert!(
        output.len() <= DEFAULT_TOOL_OUTPUT_LIMIT + OMISSION_NOTE,
        "{} bytes reached the observer",
        output.len()
    );
    assert!(
        !output.contains(TOKEN),
        "the turn's token reached the observer"
    );
    assert!(output.starts_with(REDACTED), "{}", &output[..64]);
    assert!(output.ends_with(END), "the output's end was lost");
}

#[tokio::test]
async fn a_claude_answer_already_streamed_is_not_repeated() {
    let stand_in = StandIn::new(
        r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"It is empty."}]},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"It is empty."}'
"#,
    );

    let (delivered, failure) = asked(
        &running(AgentKind::Claude, standing_in(&stand_in)),
        AgentKind::Claude,
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(spoken(&delivered), "It is empty.");
}

#[tokio::test]
async fn an_answer_with_no_streamed_prose_is_the_content() {
    let stand_in = StandIn::new(
        r#"echo '{"type":"result","subtype":"success","is_error":false,"result":"Two."}'"#,
    );

    let (delivered, failure) = asked(
        &running(AgentKind::Claude, standing_in(&stand_in)),
        AgentKind::Claude,
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(spoken(&delivered), "Two.");
    assert_eq!(reasons(&delivered), [STOP]);
}

#[tokio::test]
async fn a_turn_on_usage_credits_is_logged_once_with_its_window_and_sign_in() {
    let on_credits = r#"echo '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"five_hour","isUsingOverage":true,"overageStatus":"allowed"}}'"#;
    let stand_in = StandIn::new(&format!("{on_credits}\n{on_credits}\n{CLAUDE_ANSWER}"));
    let client = LlmClient::new(LlmConfig::default()).with_backend(
        LlmBackend::cli(AgentKind::Claude, standing_in(&stand_in))
            .with_sign_in(SignIn::Organization),
    );

    let ((_, failure), logged) = captured_logs(asked(&client, AgentKind::Claude)).await;

    assert!(failure.is_none(), "{failure:?}");
    let lines: Vec<&str> = logged
        .lines()
        .filter(|line| line.contains(ON_CREDITS))
        .collect();
    let [line] = lines.as_slice() else {
        panic!("expected zone's line once, got {lines:?}\n{logged}");
    };
    for field in [
        "provider=claude",
        "sign_in=Organization",
        "window=five_hour",
    ] {
        assert!(line.contains(field), "{field} is missing from {line}");
    }
}
