//! Reading `codex exec --json`.

use serde::Deserialize;
use serde_json::Value;

use crate::llm::provider::event::AgentEvent;
use crate::llm::provider::settings::Toolset;
use crate::llm::{FunctionCall, ToolCall, Usage};

const CALL_TYPE: &str = "function";

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
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
    #[serde(other)]
    Ignored,
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
        Self {
            prompt_tokens: counts.input_tokens,
            completion_tokens: counts.output_tokens,
            total_tokens: counts.input_tokens.saturating_add(counts.output_tokens),
        }
    }
}

pub fn interpret(line: &str, events: &mut Vec<AgentEvent>) {
    let Ok(event) = serde_json::from_str::<Event>(line.trim()) else {
        return;
    };

    match event {
        Event::Completed {
            item: Item::Message { text },
        } => events.push(AgentEvent::Text(text)),
        Event::Completed {
            item: Item::Command { id, command },
        } => events.push(AgentEvent::Tool(ToolCall {
            id,
            call_type: CALL_TYPE.to_string(),
            function: FunctionCall {
                name: "command_execution".to_string(),
                arguments: serde_json::json!({ "command": command }).to_string(),
            },
        })),
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
            events.push(AgentEvent::Tool(ToolCall {
                id,
                call_type: CALL_TYPE.to_string(),
                function: FunctionCall {
                    name: Toolset::qualified(&server, &tool),
                    arguments: arguments.to_string(),
                },
            }));
        }
        Event::Turn { usage } => {
            if let Some(usage) = usage {
                events.push(AgentEvent::Usage(usage.into()));
            }
            events.push(AgentEvent::Finished {
                finish_reason: Some("completed".to_string()),
            });
        }
        Event::Failed { error, message } => events.push(AgentEvent::Failed(
            error
                .and_then(|error| error.message)
                .or(message)
                .unwrap_or_else(|| "the agent reported a failed turn".to_string()),
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
        | Event::Ignored => {}
    }
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
    fn an_exhausted_quota_reads_as_a_rate_limit_to_the_worker() {
        let events = interpret_all(QUOTA);

        let failures = failures(&events);

        assert_eq!(
            failures.len(),
            1,
            "the failed turn, and only the failed turn"
        );
        for message in failures {
            assert!(
                message.to_ascii_lowercase().contains("usage limit"),
                "the worker cannot classify {message:?}"
            );
        }
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
            r#"{"type":"thread.started","thread_id":"x"}"#,
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
}
