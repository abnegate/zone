//! Which model and effort the agent is told to run.

use abnegate_llm::ReasoningEffort;

use super::*;

/// A turn of `agent` on `model` under `settings`, answered by a stand-in.
async fn on(
    agent: AgentKind,
    model: &str,
    configure: impl FnOnce(CliSettings) -> CliSettings,
) -> StandIn {
    let stand_in = answering(agent);
    let client = running(agent, configure(standing_in(&stand_in)));
    let (_, failure) = streamed(&client, model, &[Message::user("What does a.rs do?")]).await;
    assert!(failure.is_none(), "{agent} {model:?}: {failure:?}");
    stand_in
}

/// A model name the agent would send its API unchanged, and fail there, is
/// never named to it: zone's own `auto`, a local model's tagged name, or
/// none at all.
#[tokio::test]
async fn a_model_the_agent_does_not_know_is_left_to_the_agent() {
    for agent in AgentKind::ALL.iter().copied() {
        for unknown in ["auto", "qwen2.5:7b-instruct", "", "  "] {
            let stand_in = on(agent, unknown, |settings| settings).await;

            let arguments = stand_in.arguments();
            assert!(
                !arguments.iter().any(|argument| argument == "--model"),
                "{agent} was told to run {unknown:?}: {arguments:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_known_model_is_passed() {
    for (agent, known) in [
        (AgentKind::Claude, "sonnet"),
        (AgentKind::Claude, "claude-sonnet-4-6"),
        (AgentKind::Codex, "gpt-5.5"),
    ] {
        let stand_in = on(agent, known, |settings| settings).await;

        assert_eq!(
            stand_in.argument("--model").as_deref(),
            Some(known),
            "{agent}"
        );
    }
}

#[tokio::test]
async fn codex_effort_is_quoted_and_precedes_the_model() {
    let codex = on(AgentKind::Codex, "gpt-5.5", |settings| {
        settings.with_effort(ReasoningEffort::High)
    })
    .await;

    let arguments = codex.arguments();
    let effort = arguments
        .windows(2)
        .position(|pair| pair == ["-c", r#"model_reasoning_effort="high""#])
        .unwrap_or_else(|| panic!("no quoted effort in {arguments:?}"));
    let model = arguments
        .iter()
        .position(|argument| argument == "--model")
        .unwrap_or_else(|| panic!("no model in {arguments:?}"));
    assert!(effort < model, "{arguments:?}");
}

/// Every argument that sets the agent's reasoning effort.
fn efforts(arguments: &[String]) -> Vec<&str> {
    arguments
        .iter()
        .map(String::as_str)
        .filter(|argument| {
            *argument == "--effort" || argument.starts_with("model_reasoning_effort=")
        })
        .collect()
}

#[tokio::test]
async fn claude_effort_is_sent_and_precedes_the_model() {
    let claude = on(AgentKind::Claude, "sonnet", |settings| {
        settings.with_effort(ReasoningEffort::High)
    })
    .await;

    let arguments = claude.arguments();
    let effort = arguments
        .windows(2)
        .position(|pair| pair == ["--effort", "high"])
        .unwrap_or_else(|| panic!("no effort in {arguments:?}"));
    let model = arguments
        .windows(2)
        .position(|pair| pair == ["--model", "sonnet"])
        .unwrap_or_else(|| panic!("no model in {arguments:?}"));
    assert!(effort < model, "{arguments:?}");
    assert_eq!(efforts(&arguments), ["--effort"], "{arguments:?}");
}

#[tokio::test]
async fn an_effort_the_model_does_not_take_is_not_sent() {
    for (agent, model, effort) in [
        (AgentKind::Claude, "haiku", ReasoningEffort::High),
        (AgentKind::Claude, "sonnet", ReasoningEffort::Ultra),
        (AgentKind::Codex, "gpt-5.5", ReasoningEffort::Max),
        (AgentKind::Claude, "auto", ReasoningEffort::Ultra),
        (AgentKind::Codex, "auto", ReasoningEffort::Max),
    ] {
        let stand_in = on(agent, model, |settings| settings.with_effort(effort)).await;

        let arguments = stand_in.arguments();
        assert!(
            efforts(&arguments).is_empty(),
            "{agent} on {model:?} was sent {effort:?}: {arguments:?}"
        );
    }
}

/// A model the agent does not know is left to the agent, which then runs
/// its default model, so the effort is judged against that model's.
#[tokio::test]
async fn an_unknown_model_given_an_effort_runs_the_agents_default_at_that_effort() {
    for (agent, sent) in [
        (AgentKind::Claude, ["--effort", "high"]),
        (AgentKind::Codex, ["-c", r#"model_reasoning_effort="high""#]),
    ] {
        let stand_in = on(agent, "auto", |settings| {
            settings.with_effort(ReasoningEffort::High)
        })
        .await;

        let arguments = stand_in.arguments();
        assert!(
            arguments.windows(2).any(|pair| pair == sent),
            "{agent}: {arguments:?}"
        );
        assert!(
            !arguments.iter().any(|argument| argument == "--model"),
            "{agent}: {arguments:?}"
        );
    }
}
