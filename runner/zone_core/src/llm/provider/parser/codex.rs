//! Reading `codex exec --json`.

use std::collections::BTreeMap;

use abnegate_secret::sanitize;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::llm::provider::event::{AgentEvent, subject};
use crate::llm::provider::output::Output;
use crate::llm::provider::settings::Toolset;
use crate::llm::provider::subagent::{Subagent, SubagentStatus};
use abnegate_llm::{Limit, ToolCall, Usage};

const UNWORDED_FAILURE: &str = "the agent reported a failed turn";

/// The status codex names a sign-in it could not use by, which stays a
/// failure whatever else its words say.
const UNAUTHORIZED: &str = "401";

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "thread.started")]
    Started {
        #[serde(default)]
        thread_id: Option<String>,
    },
    #[serde(rename = "item.started")]
    Begun { item: Item },
    #[serde(rename = "item.updated")]
    Updated { item: Item },
    #[serde(rename = "item.completed")]
    Completed { item: Item },
    #[serde(rename = "turn.completed")]
    Turn {
        #[serde(default)]
        usage: Option<TokenCounts>,
    },
    #[serde(rename = "turn.failed")]
    Failed {
        #[serde(default)]
        error: Option<Reason>,
        #[serde(default)]
        message: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        message: Option<String>,
    },
    #[serde(other)]
    Ignored,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Item {
    #[serde(rename = "agent_message")]
    Message { text: String },
    #[serde(rename = "command_execution")]
    Command {
        #[serde(default)]
        id: String,
        #[serde(default)]
        command: String,
        #[serde(default)]
        aggregated_output: String,
        #[serde(default)]
        exit_code: Option<i64>,
    },
    #[serde(rename = "mcp_tool_call")]
    Call {
        #[serde(default)]
        id: String,
        server: String,
        tool: String,
        #[serde(default)]
        arguments: Value,
        #[serde(default)]
        error: Option<Reason>,
    },
    #[serde(rename = "collab_tool_call")]
    Collab(Collab),
    #[serde(other)]
    Ignored,
}

/// A call codex made to one of its collaboration tools, which start, talk to,
/// wait on and close the agents it runs beside itself. Every field is read
/// leniently: news of a collaborator is never worth losing the line over.
#[derive(Debug, Deserialize)]
struct Collab {
    #[serde(default, deserialize_with = "lenient")]
    tool: Option<CollabTool>,
    #[serde(default, deserialize_with = "lenient")]
    receiver_thread_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient")]
    prompt: Option<String>,
    /// Each agent's last known state by its thread id, kept raw so that one
    /// agent in an unknown shape does not hide the others.
    #[serde(default, deserialize_with = "lenient")]
    agents_states: Option<BTreeMap<String, Value>>,
    #[serde(default, deserialize_with = "lenient")]
    status: Option<CollabStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CollabTool {
    SpawnAgent,
    SendInput,
    Wait,
    CloseAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CollabStatus {
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Deserialize)]
struct CollabAgent {
    #[serde(default, deserialize_with = "lenient")]
    status: Option<CollabAgentStatus>,
    #[serde(default, deserialize_with = "lenient")]
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CollabAgentStatus {
    PendingInit,
    Running,
    Interrupted,
    Completed,
    Errored,
    Shutdown,
    NotFound,
}

impl Collab {
    /// What this call said about each agent it named, one update per agent.
    fn subagents(self) -> Vec<Subagent> {
        let Some(tool) = self.tool else {
            return Vec::new();
        };
        let spawned = tool == CollabTool::SpawnAgent;
        let closed = tool == CollabTool::CloseAgent && self.status == Some(CollabStatus::Completed);
        let mut subagents: Vec<Subagent> = Vec::new();
        if spawned {
            let description = self.prompt.as_deref().and_then(described);
            let receivers = self.receiver_thread_ids.unwrap_or_default();
            for id in receivers.into_iter().filter(|id| !id.is_empty()) {
                subagents.push(Subagent {
                    description: description.clone(),
                    status: Some(SubagentStatus::Running),
                    ..Subagent::new(id)
                });
            }
        }
        for (id, state) in self.agents_states.unwrap_or_default() {
            let Ok(CollabAgent {
                status: Some(status),
                message,
            }) = serde_json::from_value(state)
            else {
                continue;
            };
            let Some(status) = status.subagent(closed) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let index = match subagents.iter().position(|subagent| subagent.id == id) {
                Some(index) => index,
                None => {
                    subagents.push(Subagent::new(id));
                    subagents.len() - 1
                }
            };
            let subagent = &mut subagents[index];
            subagent.status = Some(status);
            match status {
                SubagentStatus::Completed => {
                    subagent.summary = message.as_deref().and_then(worded);
                }
                SubagentStatus::Failed => subagent.error = message.as_deref().and_then(worded),
                SubagentStatus::Running | SubagentStatus::Stopped => {}
            }
        }
        if spawned && self.status == Some(CollabStatus::Failed) {
            for subagent in &mut subagents {
                subagent.status = Some(SubagentStatus::Failed);
            }
        }
        subagents
    }
}

impl CollabAgentStatus {
    /// What a reader is shown for this status. codex reports a closed agent as
    /// it stood just before the close, so one still running then was stopped
    /// by it. An agent codex could not find says nothing about it.
    fn subagent(self, closed: bool) -> Option<SubagentStatus> {
        match self {
            Self::PendingInit | Self::Running if closed => Some(SubagentStatus::Stopped),
            Self::PendingInit | Self::Running => Some(SubagentStatus::Running),
            Self::Completed => Some(SubagentStatus::Completed),
            Self::Errored => Some(SubagentStatus::Failed),
            Self::Interrupted | Self::Shutdown => Some(SubagentStatus::Stopped),
            Self::NotFound => None,
        }
    }
}

/// What an agent was asked to do, on one line: the prompt's first line, cut
/// and cleaned as a tool call's subject is.
fn described(prompt: &str) -> Option<String> {
    subject(&serde_json::json!({ "prompt": prompt }).to_string())
}

/// An agent's own report, cleaned of anything a reader must not see.
fn worded(message: &str) -> Option<String> {
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    Some(sanitize(message).into_owned())
}

/// A field codex may one day send in another shape, read as absent rather
/// than losing the whole line to a parse error.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

#[derive(Debug, Deserialize)]
struct Reason {
    #[serde(default)]
    message: Option<String>,
}

/// Codex reports the cached share of the prompt separately, and unlike
/// Anthropic it reports it as part of `input_tokens` rather than beside it, so
/// the cached count is not added again.
#[derive(Debug, Deserialize)]
struct TokenCounts {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

impl From<TokenCounts> for Usage {
    fn from(counts: TokenCounts) -> Self {
        Self::new(counts.input_tokens, counts.output_tokens)
    }
}

pub fn interpret(line: &str, events: &mut Vec<AgentEvent>) {
    let Ok(event) = serde_json::from_str::<Event>(line.trim()) else {
        return;
    };

    match event {
        Event::Started {
            thread_id: Some(id),
        } if !id.is_empty() => events.push(AgentEvent::Session(id)),
        Event::Completed {
            item: Item::Message { text },
        } => events.push(AgentEvent::Text(text)),
        Event::Completed {
            item:
                Item::Command {
                    id,
                    command,
                    aggregated_output,
                    exit_code,
                },
        } => {
            let call = ToolCall::function(
                id,
                "command_execution",
                serde_json::json!({ "command": command }).to_string(),
            );
            events.push(AgentEvent::Tool(call.clone()));
            events.push(AgentEvent::Output(Output::new(
                call,
                aggregated_output,
                exit_code == Some(0),
            )));
        }
        Event::Completed {
            item:
                Item::Call {
                    id,
                    server,
                    tool,
                    arguments,
                    error,
                },
        } => {
            if let Some(reason) = error.and_then(|error| error.message) {
                tracing::warn!(%server, %tool, reason, "codex could not complete an MCP tool call");
            }
            events.push(AgentEvent::Tool(ToolCall::function(
                id,
                Toolset::qualified(&server, &tool),
                arguments.to_string(),
            )));
        }
        Event::Begun {
            item: Item::Collab(collab),
        }
        | Event::Updated {
            item: Item::Collab(collab),
        }
        | Event::Completed {
            item: Item::Collab(collab),
        } => events.extend(collab.subagents().into_iter().map(AgentEvent::Subagent)),
        Event::Turn { usage } => {
            if let Some(usage) = usage {
                events.push(AgentEvent::Usage(usage.into()));
            }
            events.push(AgentEvent::Finished {
                finish_reason: Some("completed".to_string()),
            });
        }
        Event::Failed { error, message } => events.push(failed(
            error
                .and_then(|error| error.message)
                .or(message)
                .unwrap_or_else(|| UNWORDED_FAILURE.to_string()),
        )),
        Event::Error { message } => {
            tracing::warn!(
                reason = message.as_deref().unwrap_or_default(),
                "codex reported an error mid-turn"
            );
        }
        Event::Completed {
            item: Item::Ignored,
        }
        | Event::Begun { .. }
        | Event::Updated { .. }
        | Event::Started { .. }
        | Event::Ignored => {}
    }
}

/// A failed turn, which is a limit when codex's words say one refused it.
/// codex reports no reset time with one.
fn failed(message: String) -> AgentEvent {
    if Limit::worded(&message) && !message.contains(UNAUTHORIZED) {
        return AgentEvent::Limited(Limit::new(message));
    }
    AgentEvent::Failed(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `codex exec --json --skip-git-repo-check -`.
    const SESSION: &str = r#"{"type":"thread.started","thread_id":"019b2c41-0000-7000-8000-000000000001"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"cargo test","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"cargo test","aggregated_output":"test result: ok. 12 passed\n","exit_code":0,"status":"completed"}}
{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"The suite passes."}}
{"type":"turn.completed","usage":{"input_tokens":4310,"cached_input_tokens":3900,"output_tokens":128}}"#;

    /// Recorded from a run that ran out of quota.
    const QUOTA: &str = r#"{"type":"thread.started","thread_id":"019b2c41-0000-7000-8000-000000000002"}
{"type":"turn.started"}
{"type":"error","message":"You have hit your usage limit. Try again later."}
{"type":"turn.failed","error":{"message":"You have hit your usage limit. Try again later."}}"#;

    /// Rung 3: codex 0.156.1 driven by a scripted Responses provider, with the
    /// flags zone passes, calling a scratch copy of zone's MCP endpoint.
    const RECOMMENDED: &str = include_str!("fixtures/codex/rung3-recommended-flags.jsonl");

    /// Rung 3: an echo call, then a call to a tool that takes no arguments.
    const TWO_CALLS: &str = include_str!("fixtures/codex/rung3-mock-approve-two-calls.jsonl");

    /// Rung 3: the provider dropped its first stream and codex reconnected.
    const RECONNECTED: &str =
        include_str!("fixtures/codex/rung3-mock-reconnect-then-complete.jsonl");

    /// Rung 3, run without the approval key: codex refused the call itself.
    const REFUSED_BY_CODEX: &str = include_str!("fixtures/codex/rung3-mock-no-approval-key.jsonl");

    /// Rung 3: zone refused the call, as it does when the reader denies one.
    const REFUSED_BY_ZONE: &str = include_str!("fixtures/codex/rung3-mock-denied-by-zone.jsonl");

    /// Rung 1: no sign-in, against the real api.openai.com.
    const SIGNED_OUT: &str = include_str!("fixtures/codex/rung1-unauthenticated.jsonl");

    /// Rung 2b: a real model, qwen2.5:7b-instruct on Ollama.
    const REAL_MODEL: &str = include_str!("fixtures/codex/rung2b-ollama-translated-approve.jsonl");

    /// Rung 3, gpt-6-sol: the call is made from inside code mode's `exec`.
    const CODE_MODE: &str = include_str!("fixtures/codex/rung3-code-mode-echo.jsonl");

    /// Two agents spawned, waited on and one of them closed. Composed from
    /// codex 0.163's `exec_events.rs` and its JSONL event processor rather than
    /// recorded: a spawn starts with no receiver yet and completes with one,
    /// and only `item.started` and `item.completed` carry a collab call.
    const COLLAB: &str = include_str!("fixtures/codex/collab-spawn-wait.jsonl");

    const REVIEWER: &str = "019b2c41-0000-7000-8000-0000000000a1";
    const TESTER: &str = "019b2c41-0000-7000-8000-0000000000a2";

    fn subagents(events: &[AgentEvent]) -> Vec<&Subagent> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Subagent(subagent) => Some(subagent),
                _ => None,
            })
            .collect()
    }

    fn collab(item: Value) -> Vec<AgentEvent> {
        let mut item = item;
        item["type"] = Value::from("collab_tool_call");
        let mut events = Vec::new();
        interpret(
            &serde_json::json!({ "type": "item.completed", "item": item }).to_string(),
            &mut events,
        );
        events
    }

    fn interpret_all(sample: &str) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for line in sample.lines() {
            interpret(line, &mut events);
        }
        events
    }

    fn calls(events: &[AgentEvent]) -> Vec<(&str, &str, &str)> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Tool(call) => Some((
                    call.id.as_str(),
                    call.function.name.as_str(),
                    call.function.arguments.as_str(),
                )),
                _ => None,
            })
            .collect()
    }

    fn answer(events: &[AgentEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn failures(events: &[AgentEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Failed(message) => Some(message.as_str()),
                _ => None,
            })
            .collect()
    }

    fn finished(events: &[AgentEvent]) -> bool {
        matches!(events.last(), Some(AgentEvent::Finished { .. }))
    }

    #[test]
    fn a_finished_command_carries_what_it_printed_and_whether_it_worked() {
        let events = interpret_all(SESSION);

        let outputs: Vec<&Output> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Output(output) => Some(output),
                _ => None,
            })
            .collect();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].call.id, "item_1");
        assert_eq!(
            outputs[0].call.function.arguments,
            r#"{"command":"cargo test"}"#
        );
        assert_eq!(outputs[0].text, "test result: ok. 12 passed\n");
        assert!(outputs[0].succeeded);

        let mut failed = Vec::new();
        interpret(
            r#"{"type":"item.completed","item":{"id":"item_9","type":"command_execution","command":"gh pr create","aggregated_output":"already exists","exit_code":1,"status":"failed"}}"#,
            &mut failed,
        );
        assert!(
            matches!(&failed[..], [AgentEvent::Tool(_), AgentEvent::Output(output)] if !output.succeeded)
        );
    }

    #[test]
    fn a_call_to_zones_tool_is_reported_under_the_name_the_model_saw() {
        let events = interpret_all(RECOMMENDED);

        assert_eq!(
            calls(&events),
            [(
                "item_0",
                "mcp__zone__echo",
                r#"{"text":"r6-rung3-nonce-9b2d"}"#
            )]
        );
        assert_eq!(
            answer(&events),
            "The echo tool returned: Wall time: 0.0035 seconds\nOutput: r6-rung3-nonce-9b2d"
        );
        assert!(failures(&events).is_empty(), "{:?}", failures(&events));
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn every_call_is_reported_including_one_that_takes_no_arguments() {
        let events = interpret_all(TWO_CALLS);

        assert_eq!(
            calls(&events),
            [
                (
                    "item_0",
                    "mcp__zone__echo",
                    r#"{"text":"r6-rung3-nonce-9b2d"}"#
                ),
                ("item_1", "mcp__zone__memory_list", "{}"),
            ]
        );
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn a_call_codex_refused_itself_is_reported_and_the_turn_goes_on() {
        let events = interpret_all(REFUSED_BY_CODEX);

        assert_eq!(
            calls(&events),
            [(
                "item_0",
                "mcp__zone__echo",
                r#"{"text":"r6-rung3-nonce-9b2d"}"#
            )]
        );
        assert!(
            failures(&events).is_empty(),
            "a refused call is the model's to answer, not the end of the turn: {:?}",
            failures(&events)
        );
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn a_call_zone_refused_is_reported_and_the_turn_goes_on() {
        let events = interpret_all(REFUSED_BY_ZONE);

        assert_eq!(
            calls(&events),
            [(
                "item_0",
                "mcp__zone__echo",
                r#"{"text":"deny: r6-rung3-nonce-9b2d"}"#
            )]
        );
        assert_eq!(
            answer(&events),
            "Zone refused the call: Wall time: 0.0014 seconds\nOutput: The user denied this tool call."
        );
        assert!(failures(&events).is_empty(), "{:?}", failures(&events));
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn an_error_codex_recovers_from_still_ends_in_its_answer() {
        let events = interpret_all(RECONNECTED);

        assert!(
            failures(&events).is_empty(),
            "a reconnect codex went on to recover from failed the turn: {:?}",
            failures(&events)
        );
        assert_eq!(
            answer(&events),
            "The echo tool returned: Wall time: 0.0012 seconds\nOutput: r6-rung3-nonce-9b2d"
        );
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn an_error_line_on_its_own_ends_nothing() {
        let mut events = Vec::new();
        interpret(
            r#"{"type":"error","message":"Reconnecting... 1/5 (stream disconnected before completion: stream closed before response.completed)"}"#,
            &mut events,
        );

        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_turn_that_fails_after_every_retry_fails_once_in_codexs_own_words() {
        let events = interpret_all(SIGNED_OUT);

        let failures = failures(&events);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("401 Unauthorized: Missing bearer or basic authentication"),
            "{failures:?}"
        );
        assert!(!finished(&events), "{events:?}");
    }

    #[test]
    fn a_real_models_call_reads_like_a_scripted_one() {
        let events = interpret_all(REAL_MODEL);

        assert_eq!(
            calls(&events),
            [(
                "item_1",
                "mcp__zone__echo",
                r#"{"text":"r6-rung2-nonce-4c1e"}"#
            )]
        );
        assert_eq!(
            answer(&events),
            "The output of the echo tool is `r6-rung2-nonce-4c1e`."
        );
        assert!(failures(&events).is_empty(), "{:?}", failures(&events));
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn a_call_made_from_code_mode_reads_like_a_direct_one() {
        let events = interpret_all(CODE_MODE);

        assert_eq!(
            calls(&events),
            [(
                "item_0",
                "mcp__zone__echo",
                r#"{"text":"t6-a22-nonce-5e1f"}"#
            )]
        );
        assert!(failures(&events).is_empty(), "{:?}", failures(&events));
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn a_recorded_session_yields_its_text_command_and_completion() {
        let events = interpret_all(SESSION);

        let text: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, ["The suite passes."]);

        let tools: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Tool(call) => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 1, "one completed command, not its start too");
        assert_eq!(tools[0].id, "item_1");
        assert_eq!(tools[0].function.arguments, r#"{"command":"cargo test"}"#);

        assert!(matches!(events.last(), Some(AgentEvent::Finished { .. })));
    }

    #[test]
    fn the_completed_turn_carries_the_token_counts() {
        let events = interpret_all(SESSION);

        let usage = events
            .iter()
            .find_map(|event| match event {
                AgentEvent::Usage(usage) => Some(usage),
                _ => None,
            })
            .expect("a usage event");

        assert_eq!(usage.prompt_tokens, 4310);
        assert_eq!(usage.completion_tokens, 128);
        assert_eq!(usage.total_tokens, 4438);
    }

    #[test]
    fn an_exhausted_quota_is_a_limit_without_a_reset_time() {
        let events = interpret_all(QUOTA);

        let [AgentEvent::Session(_), AgentEvent::Limited(limit)] = events.as_slice() else {
            panic!(
                "expected the session, then the failed turn, and only it, as a limit: {events:?}"
            );
        };
        assert_eq!(
            limit.message,
            "You have hit your usage limit. Try again later."
        );
        assert!(
            limit.message.to_ascii_lowercase().contains("usage limit"),
            "the worker cannot classify {limit:?}"
        );
        assert_eq!(limit.resets_at, None);
        assert_eq!(limit.window, None);
        assert!(!limit.credits);
    }

    #[test]
    fn an_unauthorized_turn_in_limit_words_stays_a_failure() {
        let mut events = Vec::new();
        interpret(
            r#"{"type":"turn.failed","error":{"message":"unexpected status 401 Unauthorized: you have hit your usage limit for this key"}}"#,
            &mut events,
        );

        assert!(
            matches!(events.as_slice(), [AgentEvent::Failed(message)] if message.contains("401")),
            "{events:?}"
        );
    }

    #[test]
    fn a_failed_turn_without_a_nested_reason_uses_its_own_message() {
        let mut events = Vec::new();
        interpret(
            r#"{"type":"turn.failed","message":"stream disconnected"}"#,
            &mut events,
        );

        let [AgentEvent::Failed(message)] = events.as_slice() else {
            panic!("expected one failure, got {events:?}");
        };
        assert_eq!(message, "stream disconnected");
    }

    #[test]
    fn a_failed_turn_with_no_wording_at_all_still_reports_a_failure() {
        let mut events = Vec::new();
        interpret(r#"{"type":"turn.failed"}"#, &mut events);

        assert!(matches!(events.as_slice(), [AgentEvent::Failed(_)]));
    }

    #[test]
    fn a_started_item_is_not_mistaken_for_a_finished_one() {
        let mut events = Vec::new();
        interpret(
            r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls"}}"#,
            &mut events,
        );
        assert!(events.is_empty(), "a start produced {events:?}");
    }

    #[test]
    fn unknown_events_and_noise_are_skipped() {
        let mut events = Vec::new();
        for line in [
            r#"{"type":"thread.started"}"#,
            r#"{"type":"thread.started","thread_id":""}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"type":"invented_next_release"}}"#,
            r#"{"type":"invented_next_release"}"#,
            "[not json",
            "",
        ] {
            interpret(line, &mut events);
        }
        assert!(events.is_empty(), "noise produced {events:?}");
    }

    #[test]
    fn spawned_agents_are_followed_until_they_report_back() {
        let events = interpret_all(COLLAB);

        let review = "Review the parser changes\u{2026}";
        assert_eq!(
            subagents(&events),
            [
                &Subagent {
                    description: Some(review.to_string()),
                    status: Some(SubagentStatus::Running),
                    ..Subagent::new(REVIEWER)
                },
                &Subagent {
                    description: Some("Run the test suite and report failures".to_string()),
                    status: Some(SubagentStatus::Running),
                    ..Subagent::new(TESTER)
                },
                &Subagent {
                    status: Some(SubagentStatus::Completed),
                    summary: Some("The parser changes read every field leniently.".to_string()),
                    ..Subagent::new(REVIEWER)
                },
                &Subagent {
                    status: Some(SubagentStatus::Failed),
                    error: Some("cargo test failed: 3 tests failed".to_string()),
                    ..Subagent::new(TESTER)
                },
                &Subagent {
                    status: Some(SubagentStatus::Completed),
                    summary: Some("The parser changes read every field leniently.".to_string()),
                    ..Subagent::new(REVIEWER)
                },
            ]
        );
        assert!(calls(&events).is_empty(), "{:?}", calls(&events));
        assert_eq!(
            answer(&events),
            "The review passed and the test run failed."
        );
        assert!(finished(&events), "{events:?}");
    }

    #[test]
    fn a_spawn_that_failed_fails_its_agent_with_its_words() {
        let events = collab(serde_json::json!({
            "id": "item_0",
            "tool": "spawn_agent",
            "receiver_thread_ids": [REVIEWER],
            "prompt": "Review it",
            "agents_states": { REVIEWER: { "status": "errored", "message": "no model available" } },
            "status": "failed",
        }));

        assert_eq!(
            subagents(&events),
            [&Subagent {
                description: Some("Review it".to_string()),
                status: Some(SubagentStatus::Failed),
                error: Some("no model available".to_string()),
                ..Subagent::new(REVIEWER)
            }]
        );

        let events = collab(serde_json::json!({
            "tool": "spawn_agent",
            "receiver_thread_ids": [REVIEWER],
            "agents_states": { REVIEWER: { "status": "pending_init" } },
            "status": "failed",
        }));
        assert_eq!(
            subagents(&events),
            [&Subagent {
                status: Some(SubagentStatus::Failed),
                ..Subagent::new(REVIEWER)
            }]
        );
    }

    #[test]
    fn an_interrupted_or_shut_down_agent_is_stopped() {
        let events = collab(serde_json::json!({
            "tool": "wait",
            "receiver_thread_ids": [REVIEWER, TESTER],
            "agents_states": {
                REVIEWER: { "status": "interrupted", "message": null },
                TESTER: { "status": "shutdown" },
            },
            "status": "completed",
        }));

        let stopped: Vec<_> = subagents(&events)
            .into_iter()
            .map(|subagent| (subagent.id.as_str(), subagent.status))
            .collect();
        assert_eq!(
            stopped,
            [
                (REVIEWER, Some(SubagentStatus::Stopped)),
                (TESTER, Some(SubagentStatus::Stopped)),
            ]
        );
    }

    #[test]
    fn closing_an_agent_that_was_still_running_stops_it() {
        let closed = collab(serde_json::json!({
            "tool": "close_agent",
            "receiver_thread_ids": [TESTER],
            "agents_states": { TESTER: { "status": "running" } },
            "status": "completed",
        }));
        assert_eq!(
            subagents(&closed),
            [&Subagent {
                status: Some(SubagentStatus::Stopped),
                ..Subagent::new(TESTER)
            }]
        );

        let waited = collab(serde_json::json!({
            "tool": "wait",
            "receiver_thread_ids": [TESTER],
            "agents_states": { TESTER: { "status": "running" } },
            "status": "completed",
        }));
        assert_eq!(
            subagents(&waited),
            [&Subagent {
                status: Some(SubagentStatus::Running),
                ..Subagent::new(TESTER)
            }],
            "only a close stops an agent it found running"
        );
    }

    #[test]
    fn an_update_to_a_collab_call_is_read_like_its_start() {
        let mut events = Vec::new();
        interpret(
            &serde_json::json!({
                "type": "item.updated",
                "item": {
                    "id": "item_2",
                    "type": "collab_tool_call",
                    "tool": "wait",
                    "agents_states": { TESTER: { "status": "completed", "message": "done" } },
                    "status": "in_progress",
                },
            })
            .to_string(),
            &mut events,
        );

        assert_eq!(
            subagents(&events),
            [&Subagent {
                status: Some(SubagentStatus::Completed),
                summary: Some("done".to_string()),
                ..Subagent::new(TESTER)
            }]
        );
    }

    #[test]
    fn a_started_or_updated_command_is_still_not_counted() {
        let mut events = Vec::new();
        for kind in ["item.started", "item.updated"] {
            interpret(
                &serde_json::json!({
                    "type": kind,
                    "item": {
                        "id": "item_1",
                        "type": "command_execution",
                        "command": "cargo test",
                        "aggregated_output": "",
                        "exit_code": null,
                        "status": "in_progress",
                    },
                })
                .to_string(),
                &mut events,
            );
            interpret(
                &serde_json::json!({
                    "type": kind,
                    "item": { "id": "item_2", "type": "agent_message", "text": "half a thought" },
                })
                .to_string(),
                &mut events,
            );
        }

        assert!(
            events.is_empty(),
            "a start or an update produced {events:?}"
        );
    }

    #[test]
    fn an_unknown_tool_status_or_missing_agent_is_ignored() {
        for item in [
            serde_json::json!({
                "tool": "invented_next_release",
                "receiver_thread_ids": [REVIEWER],
                "agents_states": { REVIEWER: { "status": "running" } },
            }),
            serde_json::json!({
                "receiver_thread_ids": [REVIEWER],
                "agents_states": { REVIEWER: { "status": "running" } },
            }),
            serde_json::json!({
                "tool": "wait",
                "agents_states": {
                    REVIEWER: { "status": "not_found" },
                    TESTER: { "status": "invented_next_release", "message": "?" },
                    "": { "status": "running" },
                },
            }),
            serde_json::json!({ "tool": "send_input", "receiver_thread_ids": [REVIEWER] }),
            serde_json::json!({ "tool": "spawn_agent", "receiver_thread_ids": [], "prompt": "Go" }),
        ] {
            let events = collab(item.clone());
            assert!(events.is_empty(), "{item} produced {events:?}");
        }
    }

    #[test]
    fn a_collab_field_in_an_unknown_shape_costs_only_that_field() {
        let events = collab(serde_json::json!({
            "tool": "wait",
            "receiver_thread_ids": "not a list",
            "prompt": 7,
            "status": "invented_next_release",
            "agents_states": {
                REVIEWER: "not an object",
                TESTER: { "status": "completed", "message": ["not", "text"] },
            },
        }));

        assert_eq!(
            subagents(&events),
            [&Subagent {
                status: Some(SubagentStatus::Completed),
                ..Subagent::new(TESTER)
            }]
        );
    }

    #[test]
    fn an_agents_words_are_cleaned_before_a_reader_sees_them() {
        let events = collab(serde_json::json!({
            "tool": "spawn_agent",
            "receiver_thread_ids": [REVIEWER],
            "prompt": "\u{1b}[31mReview\u{1b}[0m the diff",
            "agents_states": { REVIEWER: { "status": "errored", "message": "  \u{1b}[1mboom\u{1b}[0m  " } },
            "status": "completed",
        }));

        let [subagent] = subagents(&events)[..] else {
            panic!("expected one subagent, got {events:?}");
        };
        assert_eq!(subagent.description.as_deref(), Some("Review the diff"));
        assert_eq!(subagent.error.as_deref(), Some("boom"));

        let blank = collab(serde_json::json!({
            "tool": "wait",
            "agents_states": { REVIEWER: { "status": "completed", "message": "   " } },
        }));
        assert_eq!(
            subagents(&blank),
            [&Subagent {
                status: Some(SubagentStatus::Completed),
                ..Subagent::new(REVIEWER)
            }]
        );
    }
}
