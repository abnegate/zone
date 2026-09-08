//! Reading `codex exec --json`.

use serde::Deserialize;

use crate::llm::provider::event::AgentEvent;
use crate::llm::{FunctionCall, ToolCall, Usage};

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
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "command_execution".to_string(),
                arguments: serde_json::json!({ "command": command }).to_string(),
            },
        })),
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
        Event::Error { message } => events.push(AgentEvent::Failed(
            message.unwrap_or_else(|| "the agent reported an error".to_string()),
        )),
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

    fn interpret_all(sample: &str) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for line in sample.lines() {
            interpret(line, &mut events);
        }
        events
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

        let failures: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Failed(message) => Some(message.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(failures.len(), 2, "both the error and the failed turn");
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
