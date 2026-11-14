//! What a turn says, and how it ends when the agent cannot finish it.

use abnegate_agent_cli::{CREDITS_REQUIRED, DEFAULT_LINE_LIMIT, PROSE_EXCEEDED};
use abnegate_llm::WindowKind;

use super::*;
use crate::llm::window::SEVEN_DAYS_OPUS;

/// When the refused window lifts, as Claude reports it.
const RESETS_AT: i64 = 1_790_208_000;

/// Claude's words when a model needs usage credits the account lacks.
const REQUIRES_CREDITS: &str =
    "Fable 5.1 requires usage credits. Switch to another model to continue.";

/// Claude's words when a window refused the turn.
const LIMIT_HIT: &str = "You have hit your session limit. It resets at 5pm.";

/// An output limit small enough for a test's prose to pass it.
const SMALL_OUTPUT_LIMIT: usize = 64;

/// How much of an overlong line is looked for in what the reader was told.
const PROSE_SAMPLE: usize = 64;

/// A Claude stand-in that reports `window` refusing the turn, then fails it.
fn refused_on(window: &str) -> StandIn {
    StandIn::new(&format!(
        r#"echo '{{"type":"rate_limit_event","rate_limit_info":{{"status":"rejected","rateLimitType":"{window}","resetsAt":{RESETS_AT}}}}}'
echo '{{"type":"assistant","message":{{"model":"<synthetic>","role":"assistant","content":[{{"type":"text","text":"{LIMIT_HIT}"}}]}},"parent_tool_use_id":null,"is_api_error_message":true}}'
echo '{{"type":"result","subtype":"success","is_error":true,"api_error_status":429,"result":"{LIMIT_HIT}"}}'
exit 1"#
    ))
}

/// The window a turn's limit refused it on, failing unless it ended limited.
fn refused_window(failure: Option<Error>) -> Window {
    let Some(Error::Limited { limit, .. }) = failure else {
        panic!("expected a limit, got {failure:?}");
    };
    assert!(limit.message.contains(LIMIT_HIT), "{}", limit.message);
    limit.window.expect("the window that refused the turn")
}

fn claude(stand_in: &StandIn) -> LlmClient {
    running(AgentKind::Claude, standing_in(stand_in))
}

#[tokio::test]
async fn a_fable_refusal_without_its_api_error_is_not_a_credits_refusal() {
    let refusal = |api_error: &str| {
        StandIn::new(&format!(
            r#"echo '{{"type":"assistant","message":{{"model":"<synthetic>","role":"assistant","content":[{{"type":"text","text":"{REQUIRES_CREDITS}"}}]}},"parent_tool_use_id":null,"is_api_error_message":true{api_error}}}'
echo '{{"type":"result","subtype":"success","is_error":true,"api_error_status":400,"result":"{REQUIRES_CREDITS}"}}'
exit 1"#
        ))
    };

    let worded = refusal("");
    let (_, failure) = asked(&claude(&worded), AgentKind::Claude).await;
    let failure = failure.expect("the refusal to fail the turn");
    assert!(matches!(failure, Error::Agent(_)), "{failure:?}");
    let rendered = failure.to_string();
    assert!(rendered.contains(REQUIRES_CREDITS), "{rendered}");
    assert!(!rendered.contains(CREDITS_REQUIRED), "{rendered}");

    let typed = refusal(r#","api_error":"model_requires_usage_credits""#);
    let (_, failure) = asked(&claude(&typed), AgentKind::Claude).await;
    let Some(Error::Limited { limit, .. }) = failure else {
        panic!("a typed refusal is a credits refusal, got {failure:?}");
    };
    assert!(limit.credits, "{limit:?}");
    assert!(
        limit.message.starts_with(CREDITS_REQUIRED),
        "{}",
        limit.message
    );
}

#[tokio::test]
async fn a_reported_window_is_renamed_to_zones_name() {
    let stand_in = StandIn::new(&format!(
        r#"echo '{{"type":"rate_limit_event","rate_limit_info":{{"status":"allowed_warning","rateLimitType":"five_hour","utilization":0.43,"resetsAt":{RESETS_AT}}}}}'
{CLAUDE_ANSWER}"#
    ));

    let (delivered, failure) = asked(&claude(&stand_in), AgentKind::Claude).await;

    assert!(failure.is_none(), "{failure:?}");
    let windows: Vec<&Window> = delivered
        .iter()
        .filter_map(|chunk| chunk.window.as_ref())
        .collect();
    let [window] = windows.as_slice() else {
        panic!("expected one window, got {windows:?}");
    };
    assert_eq!(window.name, FIVE_HOURS);
    assert_eq!(window.kind, Some(WindowKind::FiveHours));
    assert_eq!(window.used_percent, Some(43.0));
}

#[tokio::test]
async fn a_limit_on_a_window_is_renamed_on_both_paths() {
    let streaming = refused_on("five_hour");
    let (delivered, failure) = asked(&claude(&streaming), AgentKind::Claude).await;
    assert!(reasons(&delivered).is_empty(), "a refused turn finished");
    assert_eq!(refused_window(failure).name, FIVE_HOURS);

    let buffered = refused_on("five_hour");
    let failure = claude(&buffered)
        .chat_with_options(
            "sonnet",
            &[Message::user("What does a.rs do?")],
            None,
            RequestOptions::new(512),
        )
        .await
        .err();
    assert_eq!(refused_window(failure).name, FIVE_HOURS);
}

#[tokio::test]
async fn a_rate_limited_turn_ends_limited_on_zones_window() {
    let stand_in = refused_on("seven_day_opus");

    let (delivered, failure) = asked(&claude(&stand_in), AgentKind::Claude).await;

    assert!(reasons(&delivered).is_empty(), "a refused turn finished");
    assert_eq!(
        spoken(&delivered),
        "",
        "the refusal was spoken as the answer"
    );
    let window = refused_window(failure);
    assert_eq!(window.name, SEVEN_DAYS_OPUS);
    assert_eq!(window.kind, Some(WindowKind::SevenDaysOpus));
    assert_eq!(
        window.resets_at,
        chrono::DateTime::from_timestamp(RESETS_AT, 0)
    );
}

#[tokio::test]
async fn an_overlong_line_fails_the_turn() {
    let prose = "x".repeat(DEFAULT_LINE_LIMIT + 1);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{prose}"}}]}},"parent_tool_use_id":null}}"#
    );
    let stand_in = StandIn::replaying(&format!("{line}\n"), CLAUDE_ANSWER);

    let (delivered, failure) = asked(&claude(&stand_in), AgentKind::Claude).await;

    let failure = failure.expect("an overlong line to fail the turn");
    assert!(matches!(failure, Error::Agent(_)), "{failure:?}");
    let rendered = failure.to_string();
    assert!(
        rendered.contains(&format!("one event exceeded {DEFAULT_LINE_LIMIT} bytes")),
        "{rendered}"
    );
    assert!(reasons(&delivered).is_empty(), "the turn still finished");
    assert!(!spoken(&delivered).contains(&prose[..PROSE_SAMPLE]));
}

#[tokio::test]
async fn tool_output_does_not_count_toward_the_output_limit() {
    let printed = "y".repeat(SMALL_OUTPUT_LIMIT * 4);
    let stand_in = StandIn::new(&format!(
        r#"echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"toolu_01","name":"Bash","input":{{"command":"cat big"}}}}]}},"parent_tool_use_id":null}}'
echo '{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_01","content":"{printed}"}}]}},"parent_tool_use_id":null}}'
{CLAUDE_ANSWER}"#
    ));
    let client = running(
        AgentKind::Claude,
        standing_in(&stand_in).with_output_limit(SMALL_OUTPUT_LIMIT),
    );

    let (delivered, failure) = asked(&client, AgentKind::Claude).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(spoken(&delivered), "Done.");
    assert_eq!(reasons(&delivered), [STOP]);
}

#[tokio::test]
async fn prose_past_the_limit_fails_with_prose_exceeded() {
    let prose = "z".repeat(SMALL_OUTPUT_LIMIT * 2);
    let stand_in = StandIn::new(&format!(
        r#"echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{prose}"}}]}},"parent_tool_use_id":null}}'
echo '{{"type":"result","subtype":"success","is_error":false}}'"#
    ));
    let client = running(
        AgentKind::Claude,
        standing_in(&stand_in).with_output_limit(SMALL_OUTPUT_LIMIT),
    );

    let (delivered, failure) = asked(&client, AgentKind::Claude).await;

    let failure = failure.expect("prose past the limit to fail the turn");
    assert!(failure.to_string().contains(PROSE_EXCEEDED), "{failure}");
    assert!(reasons(&delivered).is_empty(), "the turn still finished");
}

#[tokio::test]
async fn zones_own_tool_calls_are_not_listed_as_reasoning() {
    let zones = AgentKind::Claude.tool(Toolset::SERVER, "list_files");
    let claude = StandIn::new(&format!(
        r#"echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"toolu_01","name":"Read","input":{{"file_path":"/w/a.rs"}}}}]}},"parent_tool_use_id":null}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"toolu_02","name":"{zones}","input":{{"path":"."}}}}]}},"parent_tool_use_id":null}}'
{CLAUDE_ANSWER}"#
    ));
    let codex = StandIn::new(&format!(
        r#"echo '{{"type":"item.completed","item":{{"id":"item_0","type":"command_execution","command":"cat a.rs","aggregated_output":"","exit_code":0,"status":"completed"}}}}'
echo '{{"type":"item.completed","item":{{"id":"item_1","type":"mcp_tool_call","server":"{server}","tool":"list_files","arguments":{{"path":"."}},"status":"completed"}}}}'
{CODEX_ANSWER}"#,
        server = Toolset::SERVER
    ));

    for (agent, stand_in, own) in [
        (AgentKind::Claude, claude, "/w/a.rs"),
        (AgentKind::Codex, codex, "cat a.rs"),
    ] {
        let (delivered, failure) = asked(&running(agent, standing_in(&stand_in)), agent).await;

        assert!(failure.is_none(), "{agent}: {failure:?}");
        let reasoned = reasoned(&delivered);
        assert!(reasoned.contains(own), "{agent}: {reasoned:?}");
        assert!(!reasoned.contains("list_files"), "{agent}: {reasoned:?}");
    }
}

#[tokio::test]
async fn a_failed_turn_ends_in_the_runs_error_with_the_agents_stderr() {
    const WORDS: &str = "the model is overloaded";
    const STDERR: &str = "warning: the retry budget is spent";
    let stand_in = StandIn::new(&format!(
        r#"echo '{STDERR}' >&2
echo '{{"type":"result","subtype":"error_during_execution","is_error":true,"result":"{WORDS}"}}'
exit 1"#
    ));

    let (delivered, failure) = asked(&claude(&stand_in), AgentKind::Claude).await;

    assert!(reasons(&delivered).is_empty(), "a failed turn finished");
    let failure = failure.expect("the turn to fail");
    assert!(matches!(failure, Error::Agent(_)), "{failure:?}");
    let rendered = failure.to_string();
    let words = rendered.find(WORDS).expect("the agent's own words");
    let heading = rendered.find(STDERR_HEADING).expect("the stderr heading");
    let stderr = rendered
        .find(STDERR)
        .expect("the end of the agent's stderr");
    assert!(words < heading && heading < stderr, "{rendered}");
}

/// Claude's blocks are pinned apart by `a_host_agents_answer_keeps_its_blocks_apart`.
#[tokio::test]
async fn a_second_block_of_prose_starts_with_a_paragraph_break() {
    let stand_in = StandIn::new(
        r#"echo '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"I will look first."}}'
echo '{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"cat a.rs","aggregated_output":"","exit_code":0,"status":"completed"}}'
echo '{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"Found it."}}'
echo '{"type":"turn.completed","usage":{"input_tokens":4,"output_tokens":1}}'"#,
    );

    let (delivered, failure) = asked(
        &running(AgentKind::Codex, standing_in(&stand_in)),
        AgentKind::Codex,
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(spoken(&delivered), "I will look first.\n\nFound it.");
}
