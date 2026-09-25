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

const UNNAMED_WINDOW: &str = "request";

const UNWORDED_FAILURE: &str = "the agent reported a failed run";

const FAILED_SUBTYPE_PREFIX: &str = "error";

const CALL_TYPE: &str = "function";

/// Begins the failure of a turn on a model the signed-in account cannot
/// spend usage credits on, before claude's own words. The task worker never
/// retries a run it reads this in, so this is load-bearing like [`THROTTLED`].
pub const UNFUNDED: &str = "The signed-in Claude account cannot spend usage credits on this model";

/// [`UNFUNDED`] for a context past the length the plan covers.
pub const UNFUNDED_CONTEXT: &str =
    "The signed-in Claude account cannot spend usage credits on a context this long";

/// Begins such a refusal instead when claude could not look the account's
/// usage credits up, which a later attempt may get past.
pub const UNCONFIRMED: &str = "The signed-in Claude account's usage credits could not be confirmed";

const INDETERMINATE_REASONS: [&str; 2] = ["fetch_error", "unknown"];

const OVERAGE_INCLUDED_WINDOW: &str = "seven_day_overage_included";

const FABLE: &str = "Fable";
const FABLE_LIMIT_REACHED: &str = "You've reached your Fable limit.";
const REQUIRES_USAGE_CREDITS: &str = " requires usage credits.";
const FABLE_VERSION_CHARACTERS: usize = 40;
const MIDDLE_DOT: char = '\u{b7}';
const LINE_BREAK: char = '\n';

/// claude's own kind for an API error, the `api_error` of the assistant line
/// it reports one on, of those Zone tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiError {
    ModelRequiresUsageCredits,
    LongContextCreditsRequired,
}

impl ApiError {
    const ALL: [Self; 2] = [
        Self::ModelRequiresUsageCredits,
        Self::LongContextCreditsRequired,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::ModelRequiresUsageCredits => "model_requires_usage_credits",
            Self::LongContextCreditsRequired => "long_context_credits_required",
        }
    }

    fn named(kind: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| known.as_str() == kind)
    }

    fn marker(self) -> &'static str {
        match self {
            Self::ModelRequiresUsageCredits => UNFUNDED,
            Self::LongContextCreditsRequired => UNFUNDED_CONTEXT,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        message: Option<AssistantMessage>,
        #[serde(default)]
        is_api_error_message: bool,
        #[serde(default)]
        api_error: Option<String>,
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

impl AssistantMessage {
    fn words(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                Block::Text { text } => Some(text.as_str()),
                Block::ToolUse { .. } | Block::Ignored => None,
            })
            .collect()
    }
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
    window: Option<String>,
    #[serde(default, rename = "overageDisabledReason")]
    reason: Option<String>,
    #[serde(default, rename = "isUsingOverage")]
    on_credits: bool,
    #[serde(default, rename = "errorCode")]
    code: Option<String>,
}

impl Limit {
    fn headroom(&self) -> bool {
        self.on_credits
            || self
                .status
                .as_deref()
                .is_some_and(|status| ALLOWED.contains(&status))
    }

    /// claude writes the typed line for a refused request right after its
    /// event, so an event that line can name better waits for it.
    fn refusal(&self) -> Option<String> {
        if self.headroom()
            || self.code.is_some()
            || self.window.as_deref() == Some(OVERAGE_INCLUDED_WINDOW)
        {
            return None;
        }
        let window = self.window.as_deref().unwrap_or(UNNAMED_WINDOW);
        let status = self.status.as_deref().unwrap_or_default();
        Some(format!("{THROTTLED} ({window}, {status})"))
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

/// One turn of claude's stream, read a line at a time.
#[derive(Debug, Default)]
pub struct Reader {
    reason: Option<String>,
}

impl Reader {
    pub fn interpret(&mut self, line: &str, events: &mut Vec<AgentEvent>) {
        let Ok(event) = serde_json::from_str::<Event>(line.trim()) else {
            return;
        };

        match event {
            Event::Assistant {
                message,
                is_api_error_message,
                api_error,
            } => self.assistant(message, is_api_error_message, api_error.as_deref(), events),
            Event::Result {
                subtype,
                is_error,
                result,
                usage,
            } => self.result(subtype, is_error, result, usage, events),
            Event::RateLimit {
                rate_limit_info: Some(limit),
            } => self.limit(&limit, events),
            Event::RateLimit {
                rate_limit_info: None,
            }
            | Event::Ignored => {}
        }
    }

    fn assistant(
        &self,
        message: Option<AssistantMessage>,
        api_error_message: bool,
        api_error: Option<&str>,
        events: &mut Vec<AgentEvent>,
    ) {
        if let Some(kind) = api_error.and_then(ApiError::named) {
            let words = message.as_ref().map(AssistantMessage::words);
            events.push(AgentEvent::Failed(
                self.refused(kind.marker(), words.as_deref().unwrap_or_default()),
            ));
            return;
        }
        let Some(message) = message.filter(|_| !api_error_message) else {
            return;
        };
        for block in message.content {
            match block {
                Block::Text { text } => events.push(AgentEvent::Text(text)),
                Block::ToolUse { id, name, input } => {
                    events.push(AgentEvent::Tool(ToolCall {
                        id,
                        call_type: CALL_TYPE.to_string(),
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

    fn result(
        &self,
        subtype: Option<String>,
        is_error: bool,
        result: Option<String>,
        usage: Option<TokenCounts>,
        events: &mut Vec<AgentEvent>,
    ) {
        if let Some(usage) = usage {
            events.push(AgentEvent::Usage(usage.into()));
        }
        let failed = is_error
            || subtype
                .as_deref()
                .is_some_and(|subtype| subtype.starts_with(FAILED_SUBTYPE_PREFIX));
        if !failed {
            events.push(AgentEvent::Finished {
                finish_reason: subtype,
            });
            return;
        }
        let message = match result.filter(|result| !result.trim().is_empty()) {
            Some(words) if fable_refusal(&words) => self.refused(UNFUNDED, &words),
            Some(words) => words,
            None => subtype.unwrap_or_else(|| UNWORDED_FAILURE.to_string()),
        };
        events.push(AgentEvent::Failed(message));
    }

    fn limit(&mut self, limit: &Limit, events: &mut Vec<AgentEvent>) {
        self.reason.clone_from(&limit.reason);
        if let Some(refusal) = limit.refusal() {
            events.push(AgentEvent::Failed(refusal));
        }
    }

    /// claude's `words` for a turn usage credits could not fund, begun with
    /// `unfunded`, or with [`UNCONFIRMED`] when claude could not look the
    /// account's credits up for the request it refused.
    fn refused(&self, unfunded: &str, words: &str) -> String {
        let marker = match self.reason.as_deref() {
            Some(reason) if INDETERMINATE_REASONS.contains(&reason) => {
                format!("{UNCONFIRMED} ({reason})")
            }
            _ => unfunded.to_string(),
        };
        let words = words.trim();
        if words.is_empty() {
            marker
        } else {
            format!("{marker}: {words}")
        }
    }
}

fn fable_refusal(words: &str) -> bool {
    words.starts_with(FABLE_LIMIT_REACHED) || fable_requires_credits(words)
}

fn fable_requires_credits(words: &str) -> bool {
    let Some((name, _)) = words
        .strip_prefix(FABLE)
        .and_then(|rest| rest.split_once(REQUIRES_USAGE_CREDITS))
    else {
        return false;
    };
    match name.strip_prefix(' ') {
        None => name.is_empty(),
        Some(version) => {
            (1..=FABLE_VERSION_CHARACTERS).contains(&version.chars().count())
                && !version.contains([MIDDLE_DOT, LINE_BREAK])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// Recorded from `claude --verbose --output-format stream-json --print`.
    const SESSION: &str = r#"{"type":"system","subtype":"init","cwd":"/w","session_id":"6f1","tools":["Read","Edit"],"model":"claude-opus-4","permissionMode":"default","apiKeySource":"none"}
{"type":"assistant","message":{"id":"msg_014","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"Reading the file first."}],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":4,"cache_creation_input_tokens":1200,"cache_read_input_tokens":8400,"output_tokens":7}},"session_id":"6f1"}
{"type":"assistant","message":{"id":"msg_015","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"tool_use","id":"toolu_01A","name":"Read","input":{"file_path":"/w/main.rs"}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":9600,"output_tokens":58}},"session_id":"6f1"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01A","content":"fn main() {}"}]},"session_id":"6f1"}
{"type":"assistant","message":{"id":"msg_016","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"The entry point is empty."}],"stop_reason":"end_turn","usage":{"input_tokens":3,"cache_read_input_tokens":9700,"output_tokens":12}},"session_id":"6f1"}
{"type":"result","subtype":"success","is_error":false,"duration_ms":8421,"duration_api_ms":7980,"num_turns":3,"result":"The entry point is empty.","session_id":"6f1","total_cost_usd":0.0412,"usage":{"input_tokens":9,"cache_creation_input_tokens":1200,"cache_read_input_tokens":27700,"output_tokens":77}}"#;

    /// What claude 2.1.278 streams for Zone's `--model fable` when the account
    /// cannot fund the turn, put together from its own code: no signed-in run
    /// has recorded one.
    const UNFUNDED_STREAM: &str = include_str!("fixtures/claude/fable-credits-required.jsonl");

    /// The same, once the plan's weekly Fable allowance is spent.
    const FABLE_LIMIT_STREAM: &str = include_str!("fixtures/claude/fable-limit-reached.jsonl");

    /// A turn on a long-context model past the context the plan covers, with
    /// usage credits off.
    const LONG_CONTEXT_STREAM: &str =
        include_str!("fixtures/claude/long-context-credits-required.jsonl");

    const REQUIRES_CREDITS: &str =
        "Fable 5.1 requires usage credits. Switch to another model to continue.";
    const LIMIT_REACHED: &str =
        "You've reached your Fable limit. Switch to another model to continue.";
    const LONG_CONTEXT: &str = "API Error: Usage credits required for 1M context · turn on usage credits at claude.ai/settings/usage?from=cc_cli_limit_message (they take effect in a new session)";
    const SESSION_LIMIT: &str = "You've hit your session limit · resets 5pm";

    const MODEL_REQUIRES_USAGE_CREDITS: &str = "model_requires_usage_credits";
    const LONG_CONTEXT_CREDITS_REQUIRED: &str = "long_context_credits_required";

    fn interpret_all(sample: &str) -> Vec<AgentEvent> {
        let mut reader = Reader::default();
        let mut events = Vec::new();
        for line in sample.lines() {
            reader.interpret(line, &mut events);
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

    fn failures(events: &[AgentEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Failed(message) => Some(message.as_str()),
                _ => None,
            })
            .collect()
    }

    fn limit(info: Value) -> String {
        json!({
            "type": "rate_limit_event",
            "rate_limit_info": info,
            "uuid": "3f0e",
            "session_id": "6f1",
        })
        .to_string()
    }

    /// claude's words for a failed request, as the assistant line it writes
    /// for one, typed `kind` when claude gives it a kind.
    fn api_error(words: &str, kind: Option<&str>) -> Value {
        let mut line = json!({
            "type": "assistant",
            "message": {
                "diagnostics": null,
                "model": "<synthetic>",
                "role": "assistant",
                "stop_details": null,
                "stop_reason": "stop_sequence",
                "type": "message",
                "content": [{"type": "text", "text": words}],
            },
            "parent_tool_use_id": null,
            "session_id": "6f1",
            "timestamp": "2026-09-25T03:12:09.412Z",
            "error": "rate_limit",
            "request_id": "req_011CZf",
            "is_api_error_message": true,
        });
        if let Some(kind) = kind {
            line["api_error"] = kind.into();
        }
        line
    }

    fn failed_result(words: &str) -> Value {
        json!({
            "type": "result",
            "subtype": "success",
            "is_error": true,
            "api_error_status": 429,
            "result": words,
            "session_id": "6f1",
        })
    }

    fn stream(lines: &[String]) -> String {
        lines.join("\n")
    }

    fn unfunded(words: &str) -> String {
        format!("{UNFUNDED}: {words}")
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
        assert_eq!(tools[0].call_type, "function");
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
            let line = limit(json!({
                "status": status,
                "rateLimitType": "five_hour",
                "utilization": 72.5,
                "isUsingOverage": false,
            }));
            let events = interpret_all(&line);
            assert!(events.is_empty(), "{status} was treated as a failure");
        }
    }

    #[test]
    fn a_refused_request_reads_as_a_rate_limit_to_the_worker_naming_its_window() {
        for window in [
            "five_hour",
            "seven_day",
            "seven_day_opus",
            "seven_day_sonnet",
            "overage",
        ] {
            for info in [
                json!({"status": "rejected", "rateLimitType": window}),
                json!({
                    "status": "rejected",
                    "rateLimitType": window,
                    "overageStatus": "rejected",
                    "overageDisabledReason": "out_of_credits",
                    "isUsingOverage": false,
                }),
            ] {
                let events = interpret_all(&limit(info));

                let [AgentEvent::Failed(message)] = events.as_slice() else {
                    panic!("expected one failure, got {events:?}");
                };
                assert_eq!(message, &format!("rate limit reached ({window}, rejected)"));
            }
        }
    }

    #[test]
    fn a_refused_request_that_names_no_window_is_a_rate_limit_on_the_request() {
        let events = interpret_all(&limit(json!({"status": "rejected"})));

        assert_eq!(
            failures(&events),
            ["rate limit reached (request, rejected)"]
        );
    }

    /// claude reports a plan's window as rejected once the account's usage
    /// credits carry a request past it, and the request goes through.
    #[test]
    fn a_request_usage_credits_carry_past_the_plans_window_is_not_a_failure() {
        for window in [
            "five_hour",
            "seven_day",
            "seven_day_opus",
            "seven_day_sonnet",
            "seven_day_overage_included",
        ] {
            for overage in ["allowed", "allowed_warning"] {
                let line = limit(json!({
                    "status": "rejected",
                    "resetsAt": 1_790_208_000,
                    "rateLimitType": window,
                    "overageStatus": overage,
                    "isUsingOverage": true,
                }));
                let events = interpret_all(&line);
                assert!(
                    events.is_empty(),
                    "{window} on usage credits ({overage}) was treated as a failure: {events:?}"
                );
            }
        }
    }

    /// claude runs a request on usage credits only when it says so. Credits
    /// the account allows beside any other status, or beside none, carried
    /// nothing past a window.
    #[test]
    fn allowed_usage_credits_are_headroom_only_when_claude_says_they_are_in_use() {
        for info in [
            json!({
                "status": "rejected",
                "rateLimitType": "five_hour",
                "overageStatus": "allowed",
                "isUsingOverage": false,
            }),
            json!({"status": "rejected", "rateLimitType": "five_hour", "overageStatus": "allowed"}),
            json!({"status": "exhausted", "rateLimitType": "five_hour", "overageStatus": "allowed"}),
            json!({"rateLimitType": "five_hour", "overageStatus": "allowed_warning"}),
        ] {
            let events = interpret_all(&limit(info.clone()));

            assert!(
                matches!(events.as_slice(), [AgentEvent::Failed(message)] if message.starts_with(THROTTLED)),
                "{info}: {events:?}"
            );
        }
    }

    /// A request past the plan's window that failed anyway still ends the
    /// turn a failure, and claude's words for it are no answer.
    #[test]
    fn a_failed_request_beside_allowed_usage_credits_still_fails_the_turn() {
        let events = interpret_all(&stream(&[
            limit(json!({
                "status": "rejected",
                "resetsAt": 1_790_208_000,
                "rateLimitType": "five_hour",
                "overageStatus": "allowed",
                "isUsingOverage": false,
            })),
            api_error(SESSION_LIMIT, None).to_string(),
            failed_result(SESSION_LIMIT).to_string(),
        ]));

        let failures = failures(&events);
        assert_eq!(
            failures.first(),
            Some(&"rate limit reached (five_hour, rejected)"),
            "{events:?}"
        );
        assert_eq!(text(&events), "", "claude's failure read as an answer");
    }

    /// Once usage credits carry a turn, a request that fails anyway ends in
    /// claude's API error, which the failed result then reports.
    #[test]
    fn an_api_error_never_streams_into_the_answer() {
        for (line, words) in [
            (api_error(SESSION_LIMIT, None), SESSION_LIMIT),
            (
                api_error(
                    "API Error: Claude's response exceeded the 32000 output token maximum.",
                    Some("max_output_tokens"),
                ),
                "API Error: Claude's response exceeded the 32000 output token maximum.",
            ),
        ] {
            let events = interpret_all(&stream(&[
                limit(json!({
                    "status": "rejected",
                    "rateLimitType": "five_hour",
                    "overageStatus": "allowed",
                    "isUsingOverage": true,
                })),
                line.to_string(),
                failed_result(words).to_string(),
            ]));

            assert_eq!(text(&events), "", "claude's API error read as an answer");
            assert_eq!(failures(&events), [words]);
        }
    }

    #[test]
    fn a_fable_turn_the_account_cannot_fund_fails_in_claudes_words_and_not_as_a_rate_limit() {
        for (stream, words) in [
            (UNFUNDED_STREAM, REQUIRES_CREDITS),
            (FABLE_LIMIT_STREAM, LIMIT_REACHED),
        ] {
            let events = interpret_all(stream);

            let failures = failures(&events);
            assert!(!failures.is_empty(), "no failure in {events:?}");
            assert!(
                failures.iter().all(|message| *message == unfunded(words)),
                "{failures:?}"
            );
            assert_eq!(text(&events), "", "claude's refusal read as an answer");
        }
    }

    /// claude writes the refused request's rate_limit_event, then the line
    /// that types the refusal. The event waits for that line, and the
    /// failed result that follows reports the same refusal.
    #[test]
    fn the_event_before_a_fable_refusal_waits_for_the_line_that_names_it() {
        for (stream, words) in [
            (UNFUNDED_STREAM, REQUIRES_CREDITS),
            (FABLE_LIMIT_STREAM, LIMIT_REACHED),
        ] {
            let mut reader = Reader::default();
            let verdicts: Vec<Vec<String>> = stream
                .lines()
                .map(|line| {
                    let mut events = Vec::new();
                    reader.interpret(line, &mut events);
                    failures(&events).into_iter().map(str::to_string).collect()
                })
                .collect();

            assert_eq!(
                verdicts,
                [
                    Vec::new(),
                    Vec::new(),
                    vec![unfunded(words)],
                    vec![unfunded(words)]
                ],
                "{stream}"
            );
        }
    }

    /// Each reason claude refuses a Fable turn for has its own words, which
    /// name what the account's owner can do.
    #[test]
    fn every_fable_refusal_reaches_the_worker_in_claudes_own_words() {
        for (reason, words) in [
            ("overage_not_provisioned", REQUIRES_CREDITS),
            (
                "out_of_credits",
                "You're out of usage credits. Switch to another model to continue.",
            ),
            (
                "org_level_disabled_until",
                "You've hit your monthly spend limit. Switch to another model to continue.",
            ),
            (
                "member_level_disabled",
                "Fable 5.1 requires usage credits. Switch to another model, or manage usage credits at claude.ai/admin-settings/usage, to continue.",
            ),
        ] {
            let events = interpret_all(&stream(&[
                limit(json!({
                    "status": "rejected",
                    "overageStatus": "rejected",
                    "overageDisabledReason": reason,
                    "isUsingOverage": false,
                    "errorCode": "credits_required",
                })),
                api_error(words, Some(MODEL_REQUIRES_USAGE_CREDITS)).to_string(),
                failed_result(words).to_string(),
            ]));

            assert_eq!(
                failures(&events).first().copied(),
                Some(unfunded(words).as_str()),
                "{reason}"
            );
            assert_eq!(text(&events), "", "{reason}");
        }
    }

    /// claude holds nothing against the account when it could not look its
    /// usage credits up, so neither does the worker.
    #[test]
    fn a_refusal_claude_could_not_check_the_usage_credits_of_is_not_unfunded() {
        for reason in ["fetch_error", "unknown"] {
            for (kind, words) in [
                (MODEL_REQUIRES_USAGE_CREDITS, REQUIRES_CREDITS),
                (MODEL_REQUIRES_USAGE_CREDITS, LIMIT_REACHED),
                (LONG_CONTEXT_CREDITS_REQUIRED, LONG_CONTEXT),
            ] {
                let events = interpret_all(&stream(&[
                    limit(json!({
                        "status": "rejected",
                        "overageStatus": "rejected",
                        "overageDisabledReason": reason,
                        "isUsingOverage": false,
                        "errorCode": "credits_required",
                    })),
                    api_error(words, Some(kind)).to_string(),
                    failed_result(words).to_string(),
                ]));

                let failures = failures(&events);
                assert_eq!(
                    failures.first().copied(),
                    Some(format!("{UNCONFIRMED} ({reason}): {words}").as_str()),
                    "{reason}, {kind}"
                );
                assert!(
                    failures.iter().all(|message| !message.contains(UNFUNDED)
                        && !message.contains(UNFUNDED_CONTEXT)),
                    "{failures:?}"
                );
            }
        }
    }

    /// The reason that counts is the one claude gave for the request it
    /// refused, the latest before the refusal.
    #[test]
    fn the_latest_event_decides_whether_claude_could_check_the_usage_credits() {
        let unchecked = limit(json!({
            "status": "allowed",
            "overageStatus": "rejected",
            "overageDisabledReason": "fetch_error",
            "isUsingOverage": false,
        }));
        let refused = limit(json!({
            "status": "rejected",
            "overageStatus": "rejected",
            "overageDisabledReason": "overage_not_provisioned",
            "isUsingOverage": false,
            "errorCode": "credits_required",
        }));
        let refusal = api_error(REQUIRES_CREDITS, Some(MODEL_REQUIRES_USAGE_CREDITS)).to_string();

        let checked = interpret_all(&stream(&[
            unchecked.clone(),
            refused.clone(),
            refusal.clone(),
        ]));
        assert_eq!(failures(&checked), [unfunded(REQUIRES_CREDITS)]);

        let unchecked_last = interpret_all(&stream(&[refused, unchecked, refusal]));
        assert_eq!(
            failures(&unchecked_last),
            [format!("{UNCONFIRMED} (fetch_error): {REQUIRES_CREDITS}")]
        );
    }

    #[test]
    fn a_failed_result_in_claudes_words_for_a_fable_refusal_names_it_whatever_the_version() {
        for words in [
            REQUIRES_CREDITS,
            "Fable requires usage credits. Switch to another model to continue.",
            "Fable 6 Preview requires usage credits. Switch to another model, or manage usage credits at claude.ai/settings/usage?from=cc_cli_limit_message, to continue.",
            LIMIT_REACHED,
        ] {
            let events = interpret_all(&failed_result(words).to_string());

            assert_eq!(failures(&events), [unfunded(words)], "{words}");
        }
    }

    #[test]
    fn a_failed_result_that_only_resembles_a_fable_refusal_keeps_its_own_words() {
        for words in [
            "Opus 5 requires usage credits. Switch to another model to continue.",
            "Fables requires usage credits. Switch to another model to continue.",
            "Fable 5.1 · preview requires usage credits. Switch to another model to continue.",
            "Fable 5.1 of a much longer name than any model has had so far requires usage credits.",
            "Usage credits are required for Fable 5.1.",
            "You have hit your weekly limit",
        ] {
            let events = interpret_all(&failed_result(words).to_string());

            assert_eq!(failures(&events), [words], "{words}");
        }
    }

    /// claude's typed kind names a Fable refusal. The server's code does not:
    /// it is the same for any request usage credits could have carried.
    #[test]
    fn the_servers_credits_code_alone_never_names_fable() {
        let words = "Usage credits are required for this request.";
        let mut refusal = api_error(words, None);
        refusal["api_error_code"] = "credits_required".into();
        let mut result = failed_result(words);
        result["api_error_code"] = "credits_required".into();

        let events = interpret_all(&stream(&[
            limit(json!({
                "status": "rejected",
                "overageStatus": "rejected",
                "overageDisabledReason": "overage_not_provisioned",
                "isUsingOverage": false,
                "errorCode": "credits_required",
            })),
            refusal.to_string(),
            result.to_string(),
        ]));

        assert_eq!(failures(&events), [words]);
    }

    #[test]
    fn a_long_context_refusal_never_reads_as_fable() {
        let events = interpret_all(LONG_CONTEXT_STREAM);

        let failures = failures(&events);
        assert_eq!(
            failures.first().copied(),
            Some(format!("{UNFUNDED_CONTEXT}: {LONG_CONTEXT}").as_str()),
            "{events:?}"
        );
        assert!(
            failures.iter().all(|message| !message.contains(UNFUNDED)),
            "{failures:?}"
        );
        assert_eq!(text(&events), "", "claude's refusal read as an answer");
    }

    #[test]
    fn a_failed_result_carries_the_agents_own_wording() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Invalid API key provided","session_id":"6f1"}"#;
        let events = interpret_all(line);

        let [AgentEvent::Failed(message)] = events.as_slice() else {
            panic!("expected one failure, got {events:?}");
        };
        assert_eq!(message, "Invalid API key provided");
    }

    #[test]
    fn a_failed_result_without_wording_falls_back_to_its_subtype() {
        let line =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":"   "}"#;
        let events = interpret_all(line);

        let [AgentEvent::Failed(message)] = events.as_slice() else {
            panic!("expected one failure, got {events:?}");
        };
        assert_eq!(message, "error_max_turns");
    }

    #[test]
    fn a_failed_result_without_wording_or_subtype_says_the_run_failed() {
        let events = interpret_all(r#"{"type":"result","is_error":true}"#);

        assert_eq!(failures(&events), ["the agent reported a failed run"]);
    }

    #[test]
    fn a_success_subtype_carrying_an_error_flag_is_still_a_failure() {
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"You have hit your weekly limit"}"#;
        let events = interpret_all(line);

        assert!(matches!(events.as_slice(), [AgentEvent::Failed(_)]));
    }

    #[test]
    fn unknown_events_and_noise_are_skipped() {
        let events = interpret_all(
            [
                r#"{"type":"system","subtype":"init"}"#,
                r#"{"type":"user","message":{"role":"user","content":[]}}"#,
                r#"{"type":"invented_next_release","payload":{}}"#,
                "[not json",
                "",
                "   ",
            ]
            .join("\n")
            .as_str(),
        );
        assert!(events.is_empty(), "noise produced {events:?}");
    }
}
