//! The chat agent's reason/act loop, expressed as a stream of events.
//!
//! `zone_core::agent::Agent` runs the same pattern for tasks, but it blocks on
//! whole completions and reports progress through a callback. A chat has to
//! render tokens as they arrive and stay cancellable mid-tool, so this loop
//! streams every completion and yields events instead. The websocket handler
//! consumes the result exactly like the plain completion stream it replaces.

use futures::{Stream, StreamExt};
use std::collections::BTreeMap;
use std::time::Instant;
use zone_core::llm::{
    LlmClient, Message as LlmMessage, Role as LlmRole, StreamToolCall, ToolCall as LlmToolCall,
};

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

        for iteration in 0..MAX_ITERATIONS {
            let stream = match llm
                .chat_stream_with_model(&model, messages.clone(), Some(definitions.clone()))
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::error!("Agent completion failed on iteration {}: {}", iteration, e);
                    // A first-round failure is usually the provider rejecting
                    // the tool definitions, which is the one cause the reader
                    // can actually act on.
                    yield AgentEvent::Failed(if iteration == 0 {
                        "Failed to generate response. This model may not support tool calling — \
                         turn off agent mode to continue."
                            .to_string()
                    } else {
                        "Failed to generate response".to_string()
                    });
                    return;
                }
            };
            let mut stream = Box::pin(stream);

            let mut text = String::new();
            let mut pending = ToolCallAccumulator::default();

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
                    text.push_str(content);
                    yield AgentEvent::Chunk(content.clone());
                }

                for image in &choice.delta.generated_images {
                    yield AgentEvent::Image(image.image_url.url.clone());
                }

                if let Some(deltas) = &choice.delta.tool_calls {
                    pending.merge(deltas);
                }

                if choice.finish_reason.is_some() {
                    break;
                }
            }

            let requested = pending.finish();
            if requested.is_empty() {
                // No tools wanted: this was the final answer.
                return;
            }

            // Replay the model's own turn so the tool results it gets back are
            // attached to the calls it made.
            messages.push(LlmMessage {
                role: LlmRole::Assistant,
                content: (!text.is_empty()).then(|| text.clone()),
                name: None,
                tool_calls: Some(requested.clone()),
                tool_call_id: None,
                images: Vec::new(),
                generated_images: Vec::new(),
            });

            for call in requested {
                if tool_calls_used >= MAX_TOOL_CALLS {
                    yield AgentEvent::Failed(format!(
                        "Stopped after {} tool calls without reaching an answer",
                        MAX_TOOL_CALLS
                    ));
                    return;
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
                };

                messages.push(LlmMessage::tool_result(&call.id, output));
            }
        }

        yield AgentEvent::Failed(format!(
            "Stopped after {} rounds of tool use without reaching an answer",
            MAX_ITERATIONS
        ));
    }
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
