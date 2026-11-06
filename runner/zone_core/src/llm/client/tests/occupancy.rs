//! The context window an agent reports, kept on the client for the turn
//! that ran it.

use super::*;

/// What Codex wrote of a turn on [`THREAD`], with its window in it.
const ROLLOUT: &str = include_str!("../fixtures/codex/rollout.jsonl");
/// The windows Codex publishes for its models.
const CATALOGUE: &str = include_str!("../fixtures/codex/models_cache.json");
const THREAD: &str = "019b2c41-0000-7000-8000-000000000001";

/// Where Codex keeps a rollout, under its home.
const SESSIONS: &str = "sessions/2026/11/05";

/// A Codex turn on [`THREAD`] that answers.
const ANSWERED: &str = r#"
echo '{"type":"thread.started","thread_id":"019b2c41-0000-7000-8000-000000000001"}'
echo '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"OK"}}'
echo '{"type":"turn.completed","usage":{"input_tokens":24633,"output_tokens":5}}'
"#;

/// What [`ROLLOUT`] says of the window and how much of it the turn filled.
fn rolled_out() -> Option<Occupancy> {
    Occupancy::reported(475_000, Some(24_638))
}

/// A Claude turn that reports its window in its result.
const CLAUDE_REPORTED: &str = r#"
echo '{"type":"system","subtype":"init","session_id":"6f1"}'
echo '{"type":"assistant","message":{"model":"claude-sonnet-4-6","role":"assistant","content":[{"type":"text","text":"Done."}],"usage":{"input_tokens":3,"cache_creation_input_tokens":120,"cache_read_input_tokens":16500,"output_tokens":9}},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Done.","modelUsage":{"claude-sonnet-4-6":{"contextWindow":200000}}}'
"#;

/// What [`CLAUDE_REPORTED`] says of the window and how much of it the turn
/// filled.
fn claude_reported() -> Option<Occupancy> {
    Occupancy::reported(200_000, Some(3 + 120 + 16_500 + 9))
}

/// A Codex stand-in that writes `rollout` as [`THREAD`]'s, and `catalogue`
/// as its models cache when given, under its `CODEX_HOME`, as Codex does
/// while it runs, then replies with `reply`.
fn codex(rollout: &str, catalogue: Option<&str>, reply: &str) -> StandIn {
    let catalogue = catalogue
        .map(|catalogue| {
            format!("cat > \"$CODEX_HOME/models_cache.json\" <<'CATALOGUE'\n{catalogue}\nCATALOGUE")
        })
        .unwrap_or_default();
    let rollout = rollout.trim_end();
    StandIn::new(&format!(
        "mkdir -p \"$CODEX_HOME/{SESSIONS}\"\n\
         cat > \"$CODEX_HOME/{SESSIONS}/rollout-2026-11-05T13-36-38-{THREAD}.jsonl\" <<'ROLLOUT'\n\
         {rollout}\nROLLOUT\n\
         {catalogue}\n\
         {reply}"
    ))
}

/// A client running `stand_in` as Codex, homed in `home`.
fn homed(stand_in: &StandIn, home: &TempDir) -> LlmClient {
    running(
        AgentKind::Codex,
        standing_in(stand_in).with_variable("CODEX_HOME", home.path().display().to_string()),
    )
}

/// Read `client`'s turn up to and including its finish, saying what the
/// client held as the agent's context once the finish arrived, and then
/// the rest of the stream.
async fn at_the_finish(client: &LlmClient, agent: AgentKind) -> (Option<Occupancy>, Option<Error>) {
    let mut stream = client
        .chat_stream_with_options(
            model(agent),
            &[Message::user("What does a.rs do?")],
            None,
            RequestOptions::new(512),
        )
        .await
        .expect("a running agent");
    let mut finished = None;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("a turn that finishes");
        if !reasons(std::slice::from_ref(&chunk)).is_empty() {
            finished = Some(client.occupancy());
            break;
        }
    }
    let (_, failure) = collected(stream).await;
    (finished.expect("the turn finished"), failure)
}

#[tokio::test]
async fn claude_occupancy_is_recorded_for_the_turn() {
    let stand_in = StandIn::new(CLAUDE_REPORTED);
    let client = running(AgentKind::Claude, standing_in(&stand_in));
    assert_eq!(client.occupancy(), None);

    let (finished, failure) = at_the_finish(&client, AgentKind::Claude).await;

    assert!(failure.is_none(), "{failure:?}");
    let reported = claude_reported();
    assert_eq!(
        finished, reported,
        "claude reports its window before it finishes"
    );
    assert_eq!(client.occupancy(), reported);
}

#[tokio::test]
async fn codex_occupancy_after_its_finish_is_recorded_once_the_stream_is_drained() {
    let home = TempDir::new().expect("a codex home");
    let stand_in = codex(ROLLOUT, None, ANSWERED);
    let client = homed(&stand_in, &home);

    let (finished, failure) = at_the_finish(&client, AgentKind::Codex).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(
        finished, None,
        "codex reports its window only after it exits"
    );
    assert_eq!(client.occupancy(), rolled_out());
}

#[tokio::test]
async fn a_timed_out_codex_turn_reports_no_occupancy() {
    let home = TempDir::new().expect("a codex home");
    let stand_in = codex(
        ROLLOUT,
        Some(CATALOGUE),
        r#"echo '{"type":"thread.started","thread_id":"019b2c41-0000-7000-8000-000000000001"}'
exec sleep 300"#,
    );
    let client = running(
        AgentKind::Codex,
        standing_in(&stand_in)
            .with_variable("CODEX_HOME", home.path().display().to_string())
            .with_timeout(Duration::from_secs(1)),
    );

    let (_, failure) = asked(&client, AgentKind::Codex).await;

    let failure = failure.expect("the turn to time out");
    assert!(failure.to_string().contains("timed out"), "{failure}");
    assert_eq!(client.occupancy(), None);
}

#[tokio::test]
async fn codex_falls_back_to_the_published_window_from_models_cache() {
    let unreported: String = ROLLOUT
        .lines()
        .filter(|line| !line.contains("token_count"))
        .map(|line| format!("{line}\n"))
        .collect();
    let home = TempDir::new().expect("a codex home");
    let stand_in = codex(&unreported, Some(CATALOGUE), ANSWERED);
    let client = homed(&stand_in, &home);

    let (_, failure) = asked(&client, AgentKind::Codex).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(
        client.occupancy(),
        Occupancy::reported(500_000 * 95 / 100, None),
        "the published window, cut to its effective share, with its fill unknown"
    );
}

#[tokio::test]
async fn a_buffered_turn_records_the_occupancy_each_agent_reports() {
    let home = TempDir::new().expect("a codex home");
    let claude = StandIn::new(CLAUDE_REPORTED);
    let codex = codex(ROLLOUT, None, ANSWERED);

    for (agent, client, reported) in [
        (
            AgentKind::Claude,
            running(AgentKind::Claude, standing_in(&claude)),
            claude_reported(),
        ),
        (AgentKind::Codex, homed(&codex, &home), rolled_out()),
    ] {
        client
            .chat_with_options(
                model(agent),
                &[Message::user("What does a.rs do?")],
                None,
                RequestOptions::new(512),
            )
            .await
            .unwrap_or_else(|error| panic!("{agent}: {error:?}"));

        assert_eq!(client.occupancy(), reported, "{agent}");
    }
}
