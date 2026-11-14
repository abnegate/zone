//! A turn read whole: the events a streamed turn reads, folded into one
//! answer, with everyone a streamed turn tells told of them too.

use super::logs::captured_logs;
use super::*;

/// What zone logs when a turn runs on usage credits.
const ON_CREDITS: &str = "the turn runs on usage credits past the plan's limit";

/// A buffered turn of `client` on `agent`'s own model asking what a file
/// does.
async fn answered(client: &LlmClient, agent: AgentKind) -> Result<ChatResponse, Error> {
    client
        .chat_with_options(
            model(agent),
            &[Message::user("What does a.rs do?")],
            None,
            RequestOptions::new(512),
        )
        .await
}

#[tokio::test]
async fn an_observer_on_a_buffered_turn_is_handed_every_output_and_subagent() {
    let recording = Arc::new(Recording::default());
    let stand_in = StandIn::replaying(include_str!("../fixtures/claude/subagents.jsonl"), "");
    let claude =
        running(AgentKind::Claude, standing_in(&stand_in)).with_observer(recording.clone());

    answered(&claude, AgentKind::Claude)
        .await
        .expect("an answer");

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

    let recording = Arc::new(Recording::default());
    let stand_in = StandIn::new(&format!(
        r#"echo '{{"type":"item.completed","item":{{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"a.rs","exit_code":0,"status":"completed"}}}}'
{CODEX_ANSWER}"#
    ));
    let codex = running(AgentKind::Codex, standing_in(&stand_in)).with_observer(recording.clone());

    answered(&codex, AgentKind::Codex).await.expect("an answer");

    assert_eq!(*recording.outputs.lock().expect("outputs"), ["a.rs"]);
}

#[tokio::test]
async fn a_buffered_turn_on_usage_credits_is_logged_with_its_window_and_sign_in() {
    let on_credits = r#"echo '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"five_hour","isUsingOverage":true,"overageStatus":"allowed"}}'"#;
    let stand_in = StandIn::new(&format!("{on_credits}\n{CLAUDE_ANSWER}"));
    let client = LlmClient::new(LlmConfig::default()).with_backend(
        LlmBackend::cli(AgentKind::Claude, standing_in(&stand_in))
            .with_sign_in(SignIn::Organization),
    );

    let (answer, logged) = captured_logs(answered(&client, AgentKind::Claude)).await;

    answer.expect("an answer");
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

/// The streamed prose is what the reader watched arrive; a buffered turn
/// answers with the final answer the agent settled on instead.
#[tokio::test]
async fn a_buffered_turn_answers_with_the_agents_final_answer_and_its_usage() {
    let stand_in = StandIn::new(
        r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Looking now."}]},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"It is empty.","usage":{"input_tokens":9,"cache_read_input_tokens":1600,"output_tokens":24}}'
"#,
    );
    let client = running(AgentKind::Claude, standing_in(&stand_in));

    let response = answered(&client, AgentKind::Claude)
        .await
        .expect("an answer");

    let choice = response.choices.first().expect("one choice");
    assert_eq!(choice.message.content.as_deref(), Some("It is empty."));
    assert_eq!(choice.finish_reason.as_deref(), Some(STOP));
    assert_eq!(
        response.usage.as_ref().map(|usage| usage.prompt_tokens),
        Some(9 + 1600)
    );
    assert!(response.id.starts_with("claude-"), "{}", response.id);
    assert_eq!(response.model, model(AgentKind::Claude));
}
