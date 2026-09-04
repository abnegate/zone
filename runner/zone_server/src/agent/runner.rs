//! The chat agent's reason/act loop, expressed as a stream of events.
//!
//! `zone_core::agent::Agent` runs the same pattern for tasks, but it blocks on
//! whole completions and reports progress through a callback. A chat has to
//! render tokens as they arrive and stay cancellable mid-tool, so this loop
//! streams every completion and yields events instead. The websocket handler
//! consumes the result exactly like the plain completion stream it replaces.

use futures::{Stream, StreamExt};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;
use zone_core::llm::{
    LlmClient, Message as LlmMessage, Role as LlmRole, StreamToolCall, ToolCall as LlmToolCall,
};

use super::Citation;
use super::citations;
use super::tools::ChatTools;

/// Maximum reason/act rounds before we stop and answer with what we have.
pub const MAX_ITERATIONS: usize = 6;

/// Maximum tool executions in a single turn, across all rounds. A model that
/// loops on one tool would otherwise sit inside the iteration budget forever.
pub const MAX_TOOL_CALLS: usize = 12;

/// What the loop reports as it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// A fragment of the assistant's visible answer.
    Chunk(String),
    /// The model asked for a tool and we are about to run it.
    ToolCallStarted {
        id: String,
        name: String,
        arguments: String,
    },
    /// A tool finished. `detail` is a short human-readable outcome, not the
    /// full output, which can run to thousands of characters.
    ToolCallCompleted {
        id: String,
        name: String,
        success: bool,
        detail: String,
        duration_ms: u64,
        citations: Vec<Citation>,
    },
    /// The model produced an image. Carries the raw URL from the provider;
    /// the consumer decides how to store it and whether it is a duplicate.
    Image(String),
    /// The turn could not continue. Anything already streamed still stands.
    Failed(String),
}

/// Everything one agent turn needs.
pub struct AgentRun {
    pub llm: LlmClient,
    pub model: String,
    pub tools: ChatTools,
    /// Conversation so far, including the system prompt and the new user turn.
    pub messages: Vec<LlmMessage>,
}

/// Run one agent turn, yielding events until the model produces a final answer.
pub fn run(run: AgentRun) -> impl Stream<Item = AgentEvent> {
    async_stream::stream! {
        let AgentRun { llm, model, tools, mut messages } = run;
        let definitions = tools.definitions();
        let mut tool_calls_used = 0usize;
        let names = tools.names();
        let mut identifiers = BTreeSet::new();
        let mut previous = Vec::new();
        let mut finalizing = false;

        for iteration in 0..=MAX_ITERATIONS {
            if iteration == MAX_ITERATIONS {
                finalizing = true;
            }
            if finalizing {
                messages.push(LlmMessage::system(
                    "Callable tool use has ended for this turn. Answer the user in ordinary text using the results already available. \
                     Do not request more tools or return function JSON. If the results are insufficient, explain what remains unknown. \
                     For a greeting or general conversation, reply directly without claiming workspace facts."
                ));
            }
            let stream = match llm
                .chat_stream_with_model(&model, messages.clone(), (!finalizing).then(|| definitions.clone()))
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::error!("Agent completion failed on iteration {}: {}", iteration, e);
                    if !finalizing && e.unsupported_tools() {
                        messages.push(LlmMessage::system(
                            "This completion request could not use callable tools because the model does not support them. \
                             Previously supplied context, including any server-provided web search results, remains available. \
                             Use that evidence to answer when sufficient; server-side web search does not require model tool support. \
                             If the request still requires a callable tool, explain that the user needs to choose a model with tool support. \
                             Do not invent tool results or deny web search results already supplied."
                        ));
                        finalizing = true;
                        continue;
                    }
                    yield AgentEvent::Failed("Failed to generate response. Check the model connection and try again.".to_string());
                    return;
                }
            };
            let mut stream = Box::pin(stream);

            let mut text = String::new();
            let mut pending = ToolCallAccumulator::default();
            let mut has_image = false;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        // Keep what already streamed: one unparseable envelope
                        // should not discard a reply that is mostly complete.
                        tracing::error!("Agent stream error: {}", e);
                        yield AgentEvent::Failed("Stream error".to_string());
                        return;
                    }
                };

                let Some(choice) = chunk.choices.first() else {
                    continue;
                };

                if let Some(content) = &choice.delta.content
                    && !content.is_empty()
                {
                    // Held until the stream ends: LiteLLM+Ollama often writes a
                    // tool call as JSON in `content` instead of `delta.tool_calls`,
                    // and that JSON must not become the visible reply.
                    text.push_str(content);
                }

                for image in &choice.delta.generated_images {
                    has_image = true;
                    yield AgentEvent::Image(image.image_url.url.clone());
                }

                if let Some(deltas) = &choice.delta.tool_calls {
                    pending.merge(deltas);
                }

                if choice.finish_reason.is_some() {
                    break;
                }
            }

            let mut requested = pending.finish();
            let parsed = parse_text_tool_calls(&text, &names);
            if finalizing {
                if !requested.is_empty() || !matches!(parsed, TextToolCalls::Prose) {
                    yield AgentEvent::Failed(
                        "The model could not finish without requesting more tools. Try a model with reliable tool support.".to_string()
                    );
                } else if !text.trim().is_empty() {
                    yield AgentEvent::Chunk(text);
                } else if !has_image {
                    yield AgentEvent::Failed("The model returned an empty response. Try again or choose another model.".to_string());
                }
                return;
            }
            let replay_content = match parsed {
                TextToolCalls::Calls(calls) => {
                    if requested.is_empty() {
                        requested = calls;
                    }
                    None
                }
                TextToolCalls::Malformed if !requested.is_empty() => None,
                TextToolCalls::Malformed => {
                    messages.push(LlmMessage::user(
                        "Your previous response contained a malformed tool call and no tools from it were executed. \
                         Return a complete valid function tool call with a JSON object for arguments, \
                         or answer the user in ordinary text. Do not repeat the malformed response."
                    ));
                    continue;
                }
                TextToolCalls::Prose => (!text.is_empty()).then(|| text.clone()),
            };
            if requested.is_empty() {
                if !text.trim().is_empty() {
                    yield AgentEvent::Chunk(text);
                } else if !has_image {
                    yield AgentEvent::Failed("The model returned an empty response. Try again or choose another model.".to_string());
                }
                return;
            }
            unique_identifiers(&mut requested, &mut identifiers);

            // Replay the model's own turn so the tool results it gets back are
            // attached to the calls it made.
            messages.push(LlmMessage {
                role: LlmRole::Assistant,
                content: replay_content,
                name: None,
                tool_calls: Some(requested.clone()),
                tool_call_id: None,
                images: Vec::new(),
                generated_images: Vec::new(),
            });

            // Only an identical ordered batch is a repetition. A subset may
            // reread state changed by a later write in the previous batch.
            let signatures: Vec<_> = requested.iter().map(signature).collect();
            let repeated = signatures == previous;
            previous = signatures;
            finalizing = repeated;

            for call in requested {
                if repeated || tool_calls_used >= MAX_TOOL_CALLS {
                    finalizing = true;
                    messages.push(LlmMessage::tool_result(
                        &call.id,
                        "Not executed: tool use stopped because the requests repeated or the turn's tool budget was exhausted. Use the existing results to answer."
                    ));
                    continue;
                }
                tool_calls_used += 1;

                yield AgentEvent::ToolCallStarted {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                };

                let started = Instant::now();
                let result = tools
                    .execute(&call.function.name, &call.function.arguments)
                    .await;
                let duration_ms = started.elapsed().as_millis() as u64;

                let output = result.to_message();

                yield AgentEvent::ToolCallCompleted {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    success: result.success,
                    detail: summarize(&result, &output),
                    duration_ms,
                    citations: if result.success {
                        citations::from_tool(&call.function.name, &output)
                    } else {
                        Vec::new()
                    },
                };

                messages.push(LlmMessage::tool_result(&call.id, output));
            }
            finalizing |= tool_calls_used >= MAX_TOOL_CALLS;
        }
    }
}

/// Match semantic arguments even when a provider changes JSON key ordering.
fn signature(call: &LlmToolCall) -> (String, String) {
    let arguments = serde_json::from_str::<serde_json::Value>(&call.function.arguments)
        .map(|mut value| {
            value.sort_all_objects();
            value.to_string()
        })
        .unwrap_or_else(|_| call.function.arguments.clone());
    (call.function.name.clone(), arguments)
}

/// Longest tool outcome we show in the UI trace.
const DETAIL_CHARS: usize = 240;

/// One line describing how a tool call went, for the UI rather than the model.
fn summarize(result: &zone_core::tools::ToolResult, output: &str) -> String {
    if !result.success {
        return output.lines().next().unwrap_or("Failed").to_string();
    }
    let line_count = output.lines().filter(|l| !l.trim().is_empty()).count();
    let first = output.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let summary = match line_count {
        0 => "No output".to_string(),
        1 => first.to_string(),
        n => format!("{} ({} lines)", first, n),
    };
    match summary.char_indices().nth(DETAIL_CHARS) {
        Some((byte_idx, _)) => format!("{}…", &summary[..byte_idx]),
        None => summary,
    }
}

/// Recover tool calls that a streaming proxy wrote as assistant text.
///
/// LiteLLM in front of Ollama answers a non-streaming tools request with
/// `message.tool_calls`, but the same request streamed arrives as JSON in
/// `delta.content` (`{"name":"...","arguments":{...}}`). Without this, the
/// loop treats that JSON as the final answer and never runs a tool.
#[derive(Debug)]
enum TextToolCalls {
    Prose,
    Calls(Vec<LlmToolCall>),
    Malformed,
}

fn parse_text_tool_calls(text: &str, names: &[String]) -> TextToolCalls {
    let trimmed = strip_code_fence(text.trim());
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => {
            return if resembles_tool_call(trimmed, names) {
                TextToolCalls::Malformed
            } else {
                TextToolCalls::Prose
            };
        }
    };
    let items = match value {
        serde_json::Value::Array(items) => items,
        object if object.is_object() => vec![object],
        _ => return TextToolCalls::Prose,
    };
    if !items.iter().any(|item| resembles_tool_value(item, names)) {
        return TextToolCalls::Prose;
    }
    match items
        .iter()
        .enumerate()
        .map(|(index, item)| text_tool_call(item, index, names))
        .collect()
    {
        Some(calls) => TextToolCalls::Calls(calls),
        None => TextToolCalls::Malformed,
    }
}

fn strip_code_fence(text: &str) -> &str {
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(str::trim_start)
        .unwrap_or(text);
    text.strip_suffix("```").map(str::trim_end).unwrap_or(text)
}

fn resembles_tool_value(value: &serde_json::Value, names: &[String]) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "id" | "type" | "function" | "name" | "arguments"
        )
    }) {
        return false;
    }
    let function = value.get("function").unwrap_or(value);
    function
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|name| {
            names.iter().any(|registered| registered == name)
                || (value.get("type").and_then(serde_json::Value::as_str) == Some("function")
                    && function.get("arguments").is_some())
        })
}

/// Inspect only envelope keys at the start of JSON-shaped output. String values
/// are decoded as tokens so quoted examples and nested argument data cannot be
/// mistaken for a top-level tool request. This recognizes intent, never repairs
/// arguments or returns anything executable.
fn resembles_tool_call(text: &str, names: &[String]) -> bool {
    let mut remaining = text.trim_start();
    if let Some(array) = remaining.strip_prefix('[') {
        remaining = array.trim_start();
    }
    let Some(object) = remaining.strip_prefix('{') else {
        return false;
    };
    remaining = object.trim_start();
    let mut wrapped = false;
    let mut recognized = false;
    let mut explicit = false;
    let mut named = false;
    loop {
        let mut tokens = serde_json::Deserializer::from_str(remaining).into_iter::<String>();
        let Some(Ok(key)) = tokens.next() else {
            return recognized;
        };
        if !matches!(
            key.as_str(),
            "id" | "type" | "function" | "name" | "arguments"
        ) {
            return false;
        }
        remaining = remaining[tokens.byte_offset()..].trim_start();
        let Some(value) = remaining.strip_prefix(':') else {
            return recognized;
        };
        remaining = value.trim_start();
        if key == "function"
            && !wrapped
            && let Some(function) = remaining.strip_prefix('{')
        {
            wrapped = true;
            remaining = function.trim_start();
            continue;
        }
        if key == "arguments" && explicit && named {
            recognized = true;
        }
        let mut values =
            serde_json::Deserializer::from_str(remaining).into_iter::<serde_json::Value>();
        let Some(Ok(value)) = values.next() else {
            return recognized;
        };
        if key == "type" {
            explicit = value.as_str() == Some("function");
        }
        if key == "name" {
            named = value.as_str().is_some();
            recognized = value
                .as_str()
                .is_some_and(|name| names.iter().any(|registered| registered == name));
        }
        remaining = remaining[values.byte_offset()..].trim_start();
        let Some(next) = remaining.strip_prefix(',') else {
            return recognized;
        };
        remaining = next.trim_start();
    }
}

fn text_tool_call(
    value: &serde_json::Value,
    index: usize,
    names: &[String],
) -> Option<LlmToolCall> {
    let object = value.as_object()?;
    let function = value.get("function").unwrap_or(value);
    let name = function.get("name")?.as_str()?;
    if !names.iter().any(|registered| registered == name)
        || value
            .get("type")
            .is_some_and(|kind| kind.as_str() != Some("function"))
        || object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "id" | "type" | "function" | "name" | "arguments"
            )
        })
        || function.as_object()?.keys().any(|key| {
            !matches!(key.as_str(), "name" | "arguments") && value.get("function").is_some()
        })
    {
        return None;
    }
    let arguments = match function.get("arguments")? {
        serde_json::Value::String(raw) => {
            let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
            if !parsed.is_object() {
                return None;
            }
            raw.clone()
        }
        object if object.is_object() => object.to_string(),
        _ => return None,
    };
    Some(LlmToolCall {
        id: value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("call_{}", index)),
        call_type: "function".to_string(),
        function: zone_core::llm::FunctionCall {
            name: name.to_string(),
            arguments,
        },
    })
}

fn unique_identifiers(calls: &mut [LlmToolCall], identifiers: &mut BTreeSet<String>) {
    // Reserve incoming ids before allocating replacements, including ids that
    // collide with the generated namespace later in this same response.
    let reserved: BTreeSet<String> = calls.iter().map(|call| call.id.clone()).collect();
    for call in calls {
        if !call.id.is_empty() && identifiers.insert(call.id.clone()) {
            continue;
        }
        let mut index = identifiers.len();
        loop {
            let candidate = format!("zone_call_{}", index);
            if !reserved.contains(&candidate) && identifiers.insert(candidate.clone()) {
                call.id = candidate;
                break;
            }
            index += 1;
        }
    }
}

/// Reassembles tool calls that arrive split across streaming deltas.
///
/// Providers disagree about how much of a call each delta carries: OpenAI sends
/// the id and name once then streams argument fragments, while some
/// OpenAI-compatible proxies repeat the whole name every delta. Both shapes,
/// and calls that arrive with no id at all, have to survive this.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    calls: BTreeMap<u32, PartialToolCall>,
}

#[derive(Debug, Default)]
struct PartialToolCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    fn merge(&mut self, deltas: &[StreamToolCall]) {
        for delta in deltas {
            let entry = self.calls.entry(delta.index).or_default();

            if let Some(id) = &delta.id
                && !id.is_empty()
            {
                entry.id = Some(id.clone());
            }

            let Some(function) = &delta.function else {
                continue;
            };
            if let Some(name) = &function.name {
                merge_name(&mut entry.name, name);
            }
            if let Some(arguments) = &function.arguments {
                entry.arguments.push_str(arguments);
            }
        }
    }

    /// Complete calls in the order the model emitted them. Entries with no name
    /// are dropped: there is nothing to dispatch on.
    fn finish(self) -> Vec<LlmToolCall> {
        self.calls
            .into_iter()
            .filter(|(_, call)| !call.name.is_empty())
            .map(|(index, call)| LlmToolCall {
                id: call.id.unwrap_or_else(|| format!("call_{}", index)),
                call_type: "function".to_string(),
                function: zone_core::llm::FunctionCall {
                    name: call.name,
                    arguments: call.arguments,
                },
            })
            .collect()
    }
}

/// Fold a name fragment into the accumulated name, tolerating providers that
/// send it once, in pieces, or in full on every delta.
fn merge_name(current: &mut String, fragment: &str) {
    if fragment.is_empty() || current == fragment {
        return;
    }
    if current.is_empty() {
        current.push_str(fragment);
    } else if fragment.starts_with(current.as_str()) {
        *current = fragment.to_string();
    } else {
        current.push_str(fragment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_core::llm::StreamFunctionCall;
    use zone_core::tools::ToolResult;

    fn parse_text_tool_calls(text: &str) -> Vec<LlmToolCall> {
        match super::parse_text_tool_calls(text, &["run_shell".into(), "list_projects".into()]) {
            TextToolCalls::Calls(calls) => calls,
            _ => Vec::new(),
        }
    }

    fn delta(
        index: u32,
        id: Option<&str>,
        name: Option<&str>,
        arguments: Option<&str>,
    ) -> StreamToolCall {
        StreamToolCall {
            index,
            id: id.map(str::to_string),
            call_type: Some("function".to_string()),
            function: Some(StreamFunctionCall {
                name: name.map(str::to_string),
                arguments: arguments.map(str::to_string),
            }),
        }
    }

    #[test]
    fn accumulates_openai_shaped_deltas() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(
            0,
            Some("call_abc"),
            Some("search_knowledge"),
            Some(""),
        )]);
        acc.merge(&[delta(0, None, None, Some("{\"query\":"))]);
        acc.merge(&[delta(0, None, None, Some("\"deploys\"}"))]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "search_knowledge");
        assert_eq!(calls[0].function.arguments, r#"{"query":"deploys"}"#);
    }

    #[test]
    fn tolerates_providers_repeating_the_name() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(0, Some("c1"), Some("list_tasks"), Some("{"))]);
        acc.merge(&[delta(0, Some("c1"), Some("list_tasks"), Some("}"))]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "list_tasks");
        assert_eq!(calls[0].function.arguments, "{}");
    }

    #[test]
    fn tolerates_names_split_across_deltas() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(0, Some("c1"), Some("list_"), None)]);
        acc.merge(&[delta(0, None, Some("tasks"), None)]);

        assert_eq!(acc.finish()[0].function.name, "list_tasks");
    }

    #[test]
    fn tolerates_cumulative_names() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(0, Some("c1"), Some("list_"), None)]);
        acc.merge(&[delta(0, None, Some("list_tasks"), None)]);

        assert_eq!(acc.finish()[0].function.name, "list_tasks");
    }

    #[test]
    fn synthesizes_an_id_when_the_provider_omits_one() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(2, None, Some("list_sources"), Some("{}"))]);

        assert_eq!(acc.finish()[0].id, "call_2");
    }

    #[test]
    fn keeps_parallel_calls_in_index_order() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(1, Some("b"), Some("list_projects"), Some("{}"))]);
        acc.merge(&[delta(0, Some("a"), Some("list_tasks"), Some("{}"))]);

        let calls = acc.finish();
        assert_eq!(calls[0].id, "a");
        assert_eq!(calls[1].id, "b");
    }

    #[test]
    fn drops_entries_that_never_named_a_tool() {
        let mut acc = ToolCallAccumulator::default();
        acc.merge(&[delta(0, Some("c1"), None, Some("{}"))]);

        assert!(acc.finish().is_empty());
    }

    #[test]
    fn no_deltas_means_no_calls() {
        assert!(ToolCallAccumulator::default().finish().is_empty());
    }

    #[test]
    fn parses_ollama_shaped_text_tool_calls() {
        let calls =
            parse_text_tool_calls(r#"{"name": "run_shell", "arguments":{"command": "uname -s"}}"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "run_shell");
        assert!(calls[0].function.arguments.contains("uname -s"));
    }

    #[test]
    fn parses_fenced_and_openai_shaped_text_tool_calls() {
        let calls = parse_text_tool_calls(
            "```json\n{\"id\":\"abc\",\"function\":{\"name\":\"list_projects\",\"arguments\":\"{}\"}}\n```",
        );
        assert_eq!(calls[0].id, "abc");
        assert_eq!(calls[0].function.name, "list_projects");
        assert_eq!(calls[0].function.arguments, "{}");
    }

    #[test]
    fn ignores_ordinary_prose() {
        assert!(parse_text_tool_calls("There are no projects yet.").is_empty());
    }

    #[test]
    fn malformed_envelopes_are_not_answers_or_partial_calls() {
        let names = vec!["list_sources".into()];
        for text in [
            r#"{"id":"call_0","type":"function","function":{"name":"list_sources","arguments":{"limit":5}}"#,
            r#"{"name":"list_sources","arguments":{"limit":5}"#,
            r#"{"name":"list_sources","arguments":"{\"limit\":"}"#,
            r#"[{"name":"list_sources","arguments":{}},{"name":"unknown","arguments":{}}]"#,
            r#"[{"name":"list_sources","arguments":{}},42]"#,
            r#"{"name":"list_sources","arguments":null}"#,
            r#"{"function":{"name":"list_sources"}}"#,
        ] {
            assert!(
                matches!(
                    super::parse_text_tool_calls(text, &names),
                    TextToolCalls::Malformed
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn ordinary_json_and_embedded_examples_are_preserved() {
        let names = vec!["list_sources".into()];
        for text in [
            r#"{"name":"Jake","arguments":{}}"#,
            r#"{"answer":{"name":"list_sources","arguments":{}}}"#,
            r#"Example: {"name":"list_sources","arguments":{}}"#,
            r#"{"description":"a function with arguments", "answer":42}"#,
            "[]",
        ] {
            assert!(
                matches!(
                    super::parse_text_tool_calls(text, &names),
                    TextToolCalls::Prose
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn fenced_arrays_parse_every_call() {
        let names = vec!["list_sources".into()];
        let TextToolCalls::Calls(calls) = super::parse_text_tool_calls(
            "```json\n[{\"name\":\"list_sources\",\"arguments\":{}},{\"name\":\"list_sources\",\"arguments\":\"{}\"}]\n```",
            &names,
        ) else {
            panic!("expected calls")
        };
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn identifiers_are_unique_across_rounds_and_incoming_collisions() {
        let names = vec!["list_sources".into()];
        let mut calls: Vec<_> = ["call_0", "call_0", "zone_call_1"]
            .into_iter()
            .enumerate()
            .map(|(index, id)| {
                text_tool_call(
                    &serde_json::json!({"id":id,"name":"list_sources","arguments":{}}),
                    index,
                    &names,
                )
                .unwrap()
            })
            .collect();
        let mut identifiers = BTreeSet::new();
        unique_identifiers(&mut calls, &mut identifiers);
        assert_eq!(identifiers.len(), 3);
        assert_eq!(calls[0].id, "call_0");
        assert_eq!(calls[2].id, "zone_call_1");
        let previous: BTreeSet<_> = calls.iter().map(|call| call.id.clone()).collect();
        unique_identifiers(&mut calls, &mut identifiers);
        assert_eq!(identifiers.len(), 6);
        assert!(calls.iter().all(|call| !previous.contains(&call.id)));
    }

    #[test]
    fn registered_shorthand_missing_arguments_is_malformed() {
        assert!(matches!(
            super::parse_text_tool_calls(r#"{"name":"list_sources"}"#, &["list_sources".into()]),
            TextToolCalls::Malformed
        ));
    }

    #[test]
    fn generic_function_records_are_prose() {
        let names = vec!["list_sources".into()];
        for text in [
            r#"{"type":"function","description":"a mathematical mapping"}"#,
            r#"{"name":"list_sources","description":"a label for a mathematical mapping"}"#,
            r#"{"type":"function","description":"a mathematical mapping""#,
        ] {
            assert!(
                matches!(
                    super::parse_text_tool_calls(text, &names),
                    TextToolCalls::Prose
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn summarize_reports_the_failure_reason() {
        let result = ToolResult::error("The knowledge base search failed.");
        let output = result.to_message();
        assert_eq!(
            summarize(&result, &output),
            "Error: The knowledge base search failed."
        );
    }

    #[test]
    fn summarize_counts_multi_line_output() {
        let result = ToolResult::success("first hit\nsecond hit\nthird hit");
        let output = result.to_message();
        assert_eq!(summarize(&result, &output), "first hit (3 lines)");
    }

    #[test]
    fn summarize_passes_through_single_lines() {
        let result = ToolResult::success("This workspace has no tasks.");
        let output = result.to_message();
        assert_eq!(summarize(&result, &output), "This workspace has no tasks.");
    }

    #[test]
    fn summarize_caps_long_lines() {
        let result = ToolResult::success("x".repeat(DETAIL_CHARS * 2));
        let output = result.to_message();
        assert_eq!(
            summarize(&result, &output).chars().count(),
            DETAIL_CHARS + 1
        );
    }
}
