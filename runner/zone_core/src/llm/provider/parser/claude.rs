//! Reading `claude --output-format stream-json`.

use serde::Deserialize;

use crate::llm::provider::event::AgentEvent;
use crate::llm::{FunctionCall, ToolCall, Usage};

/// Statuses that report headroom rather than refuse the request.
const ALLOWED: [&str; 2] = ["allowed", "allowed_warning"];

/// The wording a throttled run is reported with.
///
/// The task worker recognises a throttled run by the words in the failure, so
/// this phrase is load-bearing and not decoration.
const THROTTLED: &str = "rate limit reached";

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        message: Option<AssistantMessage>,
    },
    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        usage: Option<TokenCounts>,
    },
    #[serde(rename = "rate_limit_event")]
    RateLimit {
        #[serde(default)]
        rate_limit_info: Option<Limit>,
    },
    #[serde(other)]
    Ignored,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Vec<Block>,
    #[serde(default)]
    usage: Option<TokenCounts>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Block {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Ignored,
}

#[derive(Debug, Deserialize)]
struct Limit {
    #[serde(default)]
    status: Option<String>,
    #[serde(default, rename = "rateLimitType")]
    kind: Option<String>,
}

/// Anthropic reports cache reads and cache writes separately from fresh input.
/// All three are prompt tokens, and dropping the cached ones understates a
/// long conversation's real prompt size by most of it.
#[derive(Debug, Deserialize)]
struct TokenCounts {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
}

impl From<TokenCounts> for Usage {
    fn from(counts: TokenCounts) -> Self {
        let prompt_tokens = counts
            .input_tokens
            .saturating_add(counts.cache_read_input_tokens)
            .saturating_add(counts.cache_creation_input_tokens);
        Self {
            prompt_tokens,
            completion_tokens: counts.output_tokens,
            total_tokens: prompt_tokens.saturating_add(counts.output_tokens),
        }
    }
}

pub fn interpret(line: &str, events: &mut Vec<AgentEvent>) {
    let Ok(event) = serde_json::from_str::<Event>(line.trim()) else {
        return;
    };

    match event {
        Event::Assistant {
            message: Some(message),
        } => {
            for block in message.content {
                match block {
                    Block::Text { text } => events.push(AgentEvent::Text(text)),
                    Block::ToolUse { id, name, input } => {
                        events.push(AgentEvent::Tool(ToolCall {
                            id,
                            call_type: "function".to_string(),
                            function: FunctionCall {
                                name,
                                arguments: input.to_string(),
                            },
                        }));
                    }
                    Block::Ignored => {}
                }
            }
            if let Some(usage) = message.usage {
                events.push(AgentEvent::Usage(usage.into()));
            }
        }
        Event::Result {
            subtype,
            is_error,
            result,
            usage,
        } => {
            if let Some(usage) = usage {
                events.push(AgentEvent::Usage(usage.into()));
            }
            let failed = is_error
                || subtype
                    .as_deref()
                    .is_some_and(|subtype| subtype.starts_with("error"));
            if failed {
                let message = result
                    .filter(|result| !result.trim().is_empty())
                    .or(subtype)
                    .unwrap_or_else(|| "the agent reported a failed run".to_string());
                events.push(AgentEvent::Failed(message));
            } else {
                events.push(AgentEvent::Finished {
                    finish_reason: subtype,
                });
            }
        }
        Event::RateLimit {
            rate_limit_info: Some(limit),
        } => {
            let status = limit.status.unwrap_or_default();
            if ALLOWED.contains(&status.as_str()) {
                return;
            }
            let kind = limit.kind.unwrap_or_else(|| "request".to_string());
            events.push(AgentEvent::Failed(format!(
                "{THROTTLED} ({kind}, {status})"
            )));
        }
        Event::Assistant { message: None } | Event::RateLimit { .. } | Event::Ignored => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `claude --verbose --output-format stream-json --print`.
    const SESSION: &str = r#"{"type":"system","subtype":"init","cwd":"/w","session_id":"6f1","tools":["Read","Edit"],"model":"claude-opus-4","permissionMode":"default","apiKeySource":"none"}
{"type":"assistant","message":{"id":"msg_014","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"Reading the file first."}],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":4,"cache_creation_input_tokens":1200,"cache_read_input_tokens":8400,"output_tokens":7}},"session_id":"6f1"}
{"type":"assistant","message":{"id":"msg_015","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"tool_use","id":"toolu_01A","name":"Read","input":{"file_path":"/w/main.rs"}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":9600,"output_tokens":58}},"session_id":"6f1"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01A","content":"fn main() {}"}]},"session_id":"6f1"}
{"type":"assistant","message":{"id":"msg_016","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"The entry point is empty."}],"stop_reason":"end_turn","usage":{"input_tokens":3,"cache_read_input_tokens":9700,"output_tokens":12}},"session_id":"6f1"}
{"type":"result","subtype":"success","is_error":false,"duration_ms":8421,"duration_api_ms":7980,"num_turns":3,"result":"The entry point is empty.","session_id":"6f1","total_cost_usd":0.0412,"usage":{"input_tokens":9,"cache_creation_input_tokens":1200,"cache_read_input_tokens":27700,"output_tokens":77}}"#;

    fn interpret_all(sample: &str) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for line in sample.lines() {
            interpret(line, &mut events);
        }
        events
    }

    fn text(events: &[AgentEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_recorded_session_yields_its_text_tools_and_result() {
        let events = interpret_all(SESSION);

        assert_eq!(
            text(&events),
            "Reading the file first.The entry point is empty."
        );

        let tools: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Tool(call) => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, "toolu_01A");
        assert_eq!(tools[0].function.name, "Read");
        assert_eq!(tools[0].function.arguments, r#"{"file_path":"/w/main.rs"}"#);

        let last = events.last().expect("a terminal event");
        assert!(
            matches!(last, AgentEvent::Finished { finish_reason } if finish_reason.as_deref() == Some("success")),
            "expected a successful finish, got {last:?}"
        );
    }

    #[test]
    fn cached_prompt_tokens_are_counted_as_prompt_tokens() {
        let events = interpret_all(SESSION);

        let usage = events
            .iter()
            .rev()
            .find_map(|event| match event {
                AgentEvent::Usage(usage) => Some(usage),
                _ => None,
            })
            .expect("a usage event");

        assert_eq!(usage.prompt_tokens, 9 + 1200 + 27700);
        assert_eq!(usage.completion_tokens, 77);
        assert_eq!(usage.total_tokens, 9 + 1200 + 27700 + 77);
    }

    #[test]
    fn a_headroom_report_is_not_a_failure() {
        for status in ["allowed", "allowed_warning"] {
            let line = format!(
                r#"{{"type":"rate_limit_event","rate_limit_info":{{"status":"{status}","rateLimitType":"five_hour","utilization":72.5}}}}"#
            );
            let mut events = Vec::new();
            interpret(&line, &mut events);
            assert!(events.is_empty(), "{status} was treated as a failure");
        }
    }

    #[test]
    fn a_refused_request_reads_as_a_rate_limit_to_the_worker() {
        let line = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"seven_day"}}"#;
        let mut events = Vec::new();
        interpret(line, &mut events);

        let [AgentEvent::Failed(message)] = events.as_slice() else {
            panic!("expected one failure, got {events:?}");
        };
        assert!(
            message.to_ascii_lowercase().contains("rate limit"),
            "the worker cannot classify {message:?}"
        );
        assert!(message.contains("seven_day"));
    }

    #[test]
    fn a_failed_result_carries_the_agents_own_wording() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Invalid API key provided","session_id":"6f1"}"#;
        let mut events = Vec::new();
        interpret(line, &mut events);

        let [AgentEvent::Failed(message)] = events.as_slice() else {
            panic!("expected one failure, got {events:?}");
        };
        assert_eq!(message, "Invalid API key provided");
    }

    #[test]
    fn a_failed_result_without_wording_falls_back_to_its_subtype() {
        let line =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":"   "}"#;
        let mut events = Vec::new();
        interpret(line, &mut events);

        let [AgentEvent::Failed(message)] = events.as_slice() else {
            panic!("expected one failure, got {events:?}");
        };
        assert_eq!(message, "error_max_turns");
    }

    #[test]
    fn a_success_subtype_carrying_an_error_flag_is_still_a_failure() {
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"You have hit your weekly limit"}"#;
        let mut events = Vec::new();
        interpret(line, &mut events);

        assert!(matches!(events.as_slice(), [AgentEvent::Failed(_)]));
    }

    #[test]
    fn unknown_events_and_noise_are_skipped() {
        let mut events = Vec::new();
        for line in [
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"user","message":{"role":"user","content":[]}}"#,
            r#"{"type":"invented_next_release","payload":{}}"#,
            "[not json",
            "",
            "   ",
        ] {
            interpret(line, &mut events);
        }
        assert!(events.is_empty(), "noise produced {events:?}");
    }
}
