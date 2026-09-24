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

/// The wording a Fable turn is reported with when the account has no usage
/// credits to spend on it. The task worker never retries a run it reads this
/// in, so this is load-bearing like [`THROTTLED`].
pub const UNFUNDED: &str = "The signed-in Claude account needs usage credits for Fable; turn them on at claude.ai/settings/usage or pick another model";

/// Claude's API code for that refusal, and the CLI's own name for it.
const UNFUNDED_CODES: [&str; 2] = ["credits_required", "model_requires_usage_credits"];

/// Claude's words for that refusal, lowercased, on a result with no code.
const UNFUNDED_WORDINGS: [&str; 2] = [
    "requires usage credits. switch to another model",
    "reached your fable limit",
];

/// The plan's weekly Fable allowance.
const FABLE_WINDOW: &str = "seven_day_overage_included";

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        message: Option<AssistantMessage>,
        #[serde(default)]
        api_error: Option<String>,
        #[serde(default)]
        api_error_code: Option<String>,
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
        #[serde(default)]
        api_error_code: Option<String>,
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
    #[serde(default, rename = "overageStatus")]
    overage: Option<String>,
    #[serde(default, rename = "errorCode")]
    code: Option<String>,
}

impl Limit {
    /// Whether the request went through: inside the plan's window, or past it
    /// on usage credits the account allows.
    fn headroom(&self) -> bool {
        [&self.status, &self.overage]
            .into_iter()
            .flatten()
            .any(|status| ALLOWED.contains(&status.as_str()))
    }

    /// What refused the request, in the task worker's words, or nothing when
    /// it went through.
    fn refusal(self) -> Option<String> {
        if unfunded_code(self.code.as_deref()) {
            return Some(UNFUNDED.to_string());
        }
        if self.headroom() {
            return None;
        }
        if self.kind.as_deref() == Some(FABLE_WINDOW) {
            return Some(UNFUNDED.to_string());
        }
        let status = self.status.unwrap_or_default();
        let kind = self.kind.unwrap_or_else(|| "request".to_string());
        Some(format!("{THROTTLED} ({kind}, {status})"))
    }
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
            api_error,
            api_error_code,
            ..
        } if unfunded_code(api_error.as_deref()) || unfunded_code(api_error_code.as_deref()) => {
            events.push(AgentEvent::Failed(UNFUNDED.to_string()));
        }
        Event::Assistant {
            message: Some(message),
            ..
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
            api_error_code,
        } => {
            if let Some(usage) = usage {
                events.push(AgentEvent::Usage(usage.into()));
            }
            let failed = is_error
                || subtype
                    .as_deref()
                    .is_some_and(|subtype| subtype.starts_with("error"));
            if !failed {
                events.push(AgentEvent::Finished {
                    finish_reason: subtype,
                });
            } else if unfunded_code(api_error_code.as_deref())
                || result.as_deref().is_some_and(unfunded_wording)
            {
                events.push(AgentEvent::Failed(UNFUNDED.to_string()));
            } else {
                let message = result
                    .filter(|result| !result.trim().is_empty())
                    .or(subtype)
                    .unwrap_or_else(|| "the agent reported a failed run".to_string());
                events.push(AgentEvent::Failed(message));
            }
        }
        Event::RateLimit {
            rate_limit_info: Some(limit),
        } => {
            if let Some(refusal) = limit.refusal() {
                events.push(AgentEvent::Failed(refusal));
            }
        }
        Event::Assistant { message: None, .. }
        | Event::RateLimit {
            rate_limit_info: None,
        }
        | Event::Ignored => {}
    }
}

fn unfunded_code(code: Option<&str>) -> bool {
    code.is_some_and(|code| UNFUNDED_CODES.contains(&code))
}

fn unfunded_wording(words: &str) -> bool {
    let words = words.to_ascii_lowercase();
    UNFUNDED_WORDINGS
        .iter()
        .any(|wording| words.contains(wording))
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
        for line in [
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"seven_day"}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"seven_day","overageStatus":"rejected","overageDisabledReason":"out_of_credits","isUsingOverage":false}}"#,
        ] {
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
    }

    /// claude reports a plan's window as rejected once the account's usage
    /// credits carry a request past it, and the request goes through.
    #[test]
    fn a_request_usage_credits_carry_past_the_plans_window_is_not_a_failure() {
        for kind in ["five_hour", "seven_day", "seven_day_overage_included"] {
            for overage in ["allowed", "allowed_warning"] {
                let line = format!(
                    r#"{{"type":"rate_limit_event","rate_limit_info":{{"status":"rejected","resetsAt":1790208000,"rateLimitType":"{kind}","overageStatus":"{overage}","isUsingOverage":true}},"uuid":"3f0e","session_id":"6f1"}}"#
                );
                let mut events = Vec::new();
                interpret(&line, &mut events);
                assert!(
                    events.is_empty(),
                    "{kind} on usage credits ({overage}) was treated as a failure: {events:?}"
                );
            }
        }
    }

    /// What claude 2.1.278 streams for a Fable turn the account has no usage
    /// credits for, put together from its own code: no signed-in run has
    /// recorded one.
    const UNFUNDED_STREAM: &str = include_str!("fixtures/claude/fable-credits-required.jsonl");

    /// The same, once the plan's weekly Fable allowance is spent.
    const FABLE_LIMIT_STREAM: &str = include_str!("fixtures/claude/fable-limit-reached.jsonl");

    fn failures(events: &[AgentEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Failed(message) => Some(message.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_fable_turn_without_usage_credits_fails_as_that_and_not_as_a_rate_limit() {
        for stream in [UNFUNDED_STREAM, FABLE_LIMIT_STREAM] {
            let events = interpret_all(stream);

            let failures = failures(&events);
            assert!(!failures.is_empty(), "no failure in {events:?}");
            assert!(
                failures.iter().all(|message| *message == UNFUNDED),
                "{failures:?}"
            );
            assert_eq!(text(&events), "", "claude's refusal read as an answer");
        }
    }

    /// The first failure a turn reports ends it, so each line claude reports
    /// the refusal on has to name it by itself.
    #[test]
    fn every_line_reporting_a_fable_turn_without_usage_credits_names_it_alone() {
        for stream in [UNFUNDED_STREAM, FABLE_LIMIT_STREAM] {
            for line in stream
                .lines()
                .filter(|line| !line.contains(r#""subtype":"init""#))
            {
                let mut events = Vec::new();
                interpret(line, &mut events);
                assert_eq!(failures(&events), [UNFUNDED], "{line}");
            }
        }
    }

    #[test]
    fn a_failed_result_in_claudes_words_for_a_fable_turn_without_usage_credits_names_it() {
        for wording in [
            "Fable 5 requires usage credits. Switch to another model to continue.",
            "You've reached your Fable limit. Switch to another model to continue.",
        ] {
            let line = serde_json::json!({
                "type": "result",
                "subtype": "success",
                "is_error": true,
                "result": wording,
            })
            .to_string();
            let mut events = Vec::new();
            interpret(&line, &mut events);
            assert_eq!(failures(&events), [UNFUNDED], "{wording}");
        }
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
