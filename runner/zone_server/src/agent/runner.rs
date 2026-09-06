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
use super::approval::{ApprovalPolicy, requires_approval};
use super::citations;
use super::receipts::ActionReceipt;
use super::tools::ChatTools;
use crate::services::chat::{
    history::{NewEntry, ReplayMessage},
    session::RunContext,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zone_core::context::{self, ContextStatus, ContextUsage, Entry, Summary};
use zone_core::llm::{RequestOptions, Usage};

/// Maximum reason/act rounds for a chat turn. Raised now that old tool
/// traces are compacted instead of replayed raw.
pub const MAX_ITERATIONS: usize = 64;

/// Maximum tool executions in a single chat turn, across all rounds.
pub const MAX_TOOL_CALLS: usize = 256;

/// How many reason/act rounds and tool calls a surface is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopBudget {
    pub max_iterations: usize,
    pub max_tool_calls: usize,
}

impl LoopBudget {
    pub const fn chat() -> Self {
        Self {
            max_iterations: MAX_ITERATIONS,
            max_tool_calls: MAX_TOOL_CALLS,
        }
    }

    pub const fn task() -> Self {
        Self {
            max_iterations: 50,
            max_tool_calls: 100,
        }
    }
}

/// What the loop reports as it runs.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum AgentEvent {
    /// A fragment of the assistant's visible answer.
    Chunk(String),
    /// Persistence acknowledgement barriers: commit successfully before polling again.
    Canonical(NewEntry),
    Consumed(Vec<String>),
    Checkpoint {
        previous: Option<Summary>,
        summary: Summary,
    },
    Context(ContextUsage),
    Usage(Usage),
    Finalizing(String),
    /// The model asked for a tool and we are about to run it.
    ToolCallStarted {
        id: String,
        name: String,
        arguments: String,
    },
    /// A tool finished. `detail` is a short human-readable outcome, not the
    /// full output, which can run to thousands of characters. Workspace writes
    /// also carry a receipt for the console.
    ToolCallCompleted {
        id: String,
        name: String,
        success: bool,
        detail: String,
        duration_ms: u64,
        citations: Vec<Citation>,
        receipt: Option<ActionReceipt>,
    },
    /// The model produced an image. Carries the raw URL from the provider;
    /// the consumer decides how to store it and whether it is a duplicate.
    Image(String),
    /// A mutating file or shell tool is waiting for the user to confirm.
    ToolApprovalRequired {
        id: String,
        name: String,
        arguments: String,
    },
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
    pub budget: LoopBudget,
    pub approval: ApprovalPolicy,
}

/// Run one agent turn, yielding events until the model produces a final answer.
pub fn run(run: AgentRun) -> impl Stream<Item = AgentEvent> {
    let context = RunContext::from_messages(run.messages.clone());
    run_with_context(run, context, true)
}

/// Shared loop for ordinary text and tool-assisted chats. Every durable event is a
/// suspension point: the consumer must commit before resuming model or tool work.
pub fn run_with_context(
    run: AgentRun,
    mut context: RunContext,
    agentic: bool,
) -> impl Stream<Item = AgentEvent> {
    async_stream::stream! {
        let AgentRun {
            llm,
            model,
            tools,
            budget,
            approval,
            ..
        } = run;
        let mut used = 0usize;
        let mut identifiers: BTreeSet<String> = context
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .message
                    .tool_calls
                    .iter()
                    .flatten()
                    .map(|call| call.id.clone())
            })
            .collect();
        let mut observations = BTreeMap::<(String, String), String>::new();
        let mut failures = BTreeSet::new();
        let mut finalizing = false;
        let mut reason = None::<String>;
        for iteration in 0..=budget.max_iterations {
            if agentic && iteration == budget.max_iterations {
                finalizing = true;
                reason = Some("The configured model-round budget was reached.".into());
            }
            if finalizing {
                let reason = reason
                    .take()
                    .unwrap_or_else(|| "Tool execution has ended for this turn.".into());
                yield AgentEvent::Finalizing(reason.clone());
                context.entries.push(Entry {id:Uuid::new_v4().to_string(),message:LlmMessage::system(format!("{reason} Answer the user in ordinary text using the evidence already available. Do not call more tools or emit function JSON. Explain any remaining uncertainty.")),preserve:true,consumed:true});
            }
            let definitions = (agentic && !finalizing).then_some(tools.definitions());
            let mut usage = context.usage(&model, definitions);
            if usage
                .threshold
                .is_some_and(|threshold| usage.used > threshold)
            {
                usage.status = ContextStatus::Compacting;
            }
            yield AgentEvent::Context(usage);
            let mut prepared = match context::prepare(
                &llm,
                &model,
                &context.entries,
                definitions,
                &context.policy,
                context.summary.as_ref(),
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    let mut usage = context.usage(&model, definitions);
                    usage.status = ContextStatus::Blocked;
                    usage.reason = Some(error.to_string());
                    yield AgentEvent::Context(usage);
                    yield AgentEvent::Failed(error.to_string());
                    return;
                }
            };
            if prepared.summary != context.summary {
                if let Some(summary) = &prepared.summary {
                    yield AgentEvent::Checkpoint {
                        previous: context.summary.clone(),
                        summary: summary.clone(),
                    };
                }
                context.summary = prepared.summary.clone();
            }
            context.decorate(&mut prepared.usage);
            yield AgentEvent::Context(prepared.usage);
            if let Err(error) = context.transport(&mut prepared.messages).await {
                yield AgentEvent::Failed(error);
                return;
            }
            let stream = match llm
                .chat_stream_with_options(
                    &model,
                    &prepared.messages,
                    definitions,
                    RequestOptions {
                        reserved: context.policy.reserved,
                    },
                )
                .await
            {
                Ok(stream) => stream,
                Err(error) => {
                    if agentic && !finalizing && error.unsupported_tools() {
                        finalizing = true;
                        reason=Some("The model does not support callable tools. Use supplied search evidence where sufficient.".into());
                        continue;
                    }
                    yield AgentEvent::Failed(format!("Failed to generate response: {error}"));
                    return;
                }
            };
            let consumed = context.consume();
            if !consumed.is_empty() {
                yield AgentEvent::Consumed(consumed);
            }
            futures::pin_mut!(stream);
            let mut text = String::new();
            let mut streamed = 0usize;
            let mut pending = ToolCallAccumulator::default();
            let mut images = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield AgentEvent::Failed(format!("Stream error: {error}"));
                        return;
                    }
                };
                if let Some(usage) = chunk.usage {
                    yield AgentEvent::Usage(usage);
                }
                let Some(choice) = chunk.choices.first() else {
                    continue;
                };
                if let Some(content) = &choice.delta.content {
                    text.push_str(content);
                    if (!agentic || pending.is_empty() && !might_be_tool_text(&text))
                        && streamed < text.len()
                    {
                        yield AgentEvent::Chunk(text[streamed..].to_string());
                        streamed = text.len();
                    }
                }
                for image in &choice.delta.generated_images {
                    images.push(image.image_url.url.clone());
                    yield AgentEvent::Image(image.image_url.url.clone());
                }
                if let Some(deltas) = &choice.delta.tool_calls {
                    pending.merge(deltas);
                }
                // A usage-only SSE frame may follow finish_reason; drain through [DONE].
            }
            let mut requested = pending.finish();
            let parsed = if agentic && !text.is_empty() {
                parse_text_tool_calls(&text, tools.names())
            } else {
                TextToolCalls::Prose
            };
            let replay = match parsed {
                TextToolCalls::Calls(calls) => {
                    if requested.is_empty() {
                        requested = calls;
                    }
                    None
                }
                TextToolCalls::Malformed if !requested.is_empty() => None,
                TextToolCalls::Malformed if !finalizing => {
                    context.entries.push(Entry {id:Uuid::new_v4().to_string(),message:LlmMessage::system("The last reply contained a malformed tool call; no tools were executed. Emit valid callable-tool arguments or answer in ordinary prose."),preserve:true,consumed:true});
                    continue;
                }
                TextToolCalls::Malformed => {
                    yield AgentEvent::Failed(
                        "The model could not finish without malformed tool calls.".into(),
                    );
                    return;
                }
                TextToolCalls::Prose => (!text.is_empty()).then(|| text.clone()),
            };
            if !agentic || requested.is_empty() {
                if text.trim().is_empty() && images.is_empty() {
                    yield AgentEvent::Failed(
                        "The model returned an empty response. Try again or choose another model."
                            .into(),
                    );
                    return;
                }
                if streamed < text.len() {
                    yield AgentEvent::Chunk(text[streamed..].into());
                }
                let mut message = LlmMessage::assistant(text);
                message.images = images;
                let entry = canonical(message, Vec::new());
                context.append(&entry);
                let id = entry.id.clone();
                yield AgentEvent::Canonical(entry);
                yield AgentEvent::Consumed(vec![id]);
                return;
            }
            if finalizing {
                yield AgentEvent::Failed(
                    "The model could not finish without requesting more tools.".into(),
                );
                return;
            }
            unique_identifiers(&mut requested, &mut identifiers);
            let signatures = requested.iter().map(signature).collect::<Vec<_>>();
            let denied_repeat = signatures
                .iter()
                .all(|signature| failures.contains(signature));
            let mutations = requested
                .iter()
                .filter(|call| tools.mutating(&call.function.name))
                .map(|call| call.id.clone())
                .collect();
            let envelope = canonical(
                LlmMessage {
                    role: LlmRole::Assistant,
                    content: replay,
                    name: None,
                    tool_calls: Some(requested.clone()),
                    tool_call_id: None,
                    images,
                    generated_images: Vec::new(),
                },
                mutations,
            );
            context.append(&envelope);
            yield AgentEvent::Canonical(envelope);
            let mut progress = false;
            let mut requested = std::collections::VecDeque::from(requested);
            while let Some(call) = requested.pop_front() {
                if denied_repeat || used >= budget.max_tool_calls {
                    finalizing = true;
                    reason = Some(
                        if denied_repeat {
                            "Repeated failed calls made no progress."
                        } else {
                            "The configured tool-call budget was reached."
                        }
                        .into(),
                    );
                    let entry = canonical(
                        LlmMessage::tool_result(
                            &call.id,
                            "Not executed: repeated failures or the configured tool budget require a final answer using existing evidence.",
                        ),
                        Vec::new(),
                    );
                    context.append(&entry);
                    yield AgentEvent::Canonical(entry);
                    continue;
                }
                let mutation = tools.mutating(&call.function.name);
                let mut batch = vec![call];
                if !mutation {
                    while used + batch.len() < budget.max_tool_calls
                        && requested
                            .front()
                            .is_some_and(|call| !tools.mutating(&call.function.name))
                    {
                        batch.push(requested.pop_front().expect("Read batch front exists"));
                    }
                }
                used += batch.len();
                for call in &batch {
                    yield AgentEvent::ToolCallStarted {
                        id: call.id.clone(),
                        name: call.function.name.clone(),
                        arguments: call.function.arguments.clone(),
                    };
                }
                let call = &batch[0];
                let denied =
                    if mutation && !approval.is_auto() && requires_approval(&call.function.name) {
                        yield AgentEvent::ToolApprovalRequired {
                            id: call.id.clone(),
                            name: call.function.name.clone(),
                            arguments: call.function.arguments.clone(),
                        };
                        match &approval {
                            ApprovalPolicy::Required(gate) => !gate.await_decision(&call.id).await,
                            ApprovalPolicy::Auto => false,
                        }
                    } else {
                        false
                    };
                // A fresh acknowledgement boundary after potentially long approval waits.
                yield AgentEvent::Context(context.usage(&model, definitions));
                let completed = futures::future::join_all(batch.into_iter().map(|call| {
                    let tools = &tools;
                    async move {
                        let signature = signature(&call);
                        let started = Instant::now();
                        let result = if denied {
                            zone_core::tools::ToolResult::error("The user denied this tool call.")
                        } else {
                            tools
                                .execute(&call.function.name, &call.function.arguments)
                                .await
                        };
                        (
                            signature,
                            finish_tool(tools, call, result, started.elapsed().as_millis() as u64)
                                .await,
                        )
                    }
                }))
                .await;
                for (signature, finished) in completed {
                    let digest = hex::encode(Sha256::digest(finished.output.as_bytes()));
                    if mutation && finished.success {
                        observations.clear();
                        failures.clear();
                        progress = true;
                    } else if finished.success {
                        if observations
                            .insert(signature.clone(), digest.clone())
                            .as_ref()
                            != Some(&digest)
                        {
                            failures.clear();
                            progress = true;
                        }
                    } else {
                        let novel = failures.insert(signature);
                        progress |= !mutation && novel;
                    }
                    let mut message = LlmMessage::tool_result(&finished.id, &finished.output);
                    message.images = finished.images.clone();
                    let entry = canonical(message, Vec::new());
                    context.append(&entry);
                    yield AgentEvent::Canonical(entry);
                    for url in &finished.images {
                        yield AgentEvent::Image(url.clone());
                    }
                    yield AgentEvent::ToolCallCompleted {
                        id: finished.id,
                        name: finished.name,
                        success: finished.success,
                        detail: finished.detail,
                        duration_ms: finished.duration_ms,
                        citations: finished.citations,
                        receipt: finished.receipt,
                    };
                }
            }
            if !progress && !finalizing {
                finalizing = true;
                reason = Some(
                    "Repeated tool reads returned unchanged evidence without progress.".into(),
                );
            }
            if used >= budget.max_tool_calls {
                finalizing = true;
                reason = Some("The configured tool-call budget was reached.".into());
            }
        }
    }
}

fn canonical(message: LlmMessage, mutations: Vec<String>) -> NewEntry {
    NewEntry {
        id: Uuid::new_v4().to_string(),
        message: ReplayMessage::from(&message),
        mutations,
    }
}

struct FinishedTool {
    id: String,
    name: String,
    success: bool,
    detail: String,
    duration_ms: u64,
    citations: Vec<Citation>,
    receipt: Option<ActionReceipt>,
    images: Vec<String>,
    output: String,
}

async fn finish_tool(
    tools: &ChatTools,
    call: LlmToolCall,
    result: zone_core::tools::ToolResult,
    duration_ms: u64,
) -> FinishedTool {
    let receipt = tools
        .write_receipt(
            &call.id,
            &call.function.name,
            &call.function.arguments,
            &result,
        )
        .await;
    let output = result.to_message();
    let citations = if result.success {
        citations::from_tool(&call.function.name, &output)
    } else {
        Vec::new()
    };
    FinishedTool {
        id: call.id,
        name: call.function.name,
        success: result.success,
        detail: summarize(&result, &output),
        duration_ms,
        citations,
        receipt,
        images: result.images,
        output,
    }
}

/// True while the buffered assistant text could still become a tool envelope
/// rather than the visible answer. Conservative: once it cannot, stream it.
fn might_be_tool_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.is_empty()
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('`')
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
            registered(names, name)
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
            recognized = value.as_str().is_some_and(|name| registered(names, name));
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
    if !registered(names, name)
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

fn registered(names: &[String], name: &str) -> bool {
    names.iter().any(|n| n == name)
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
    fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

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
    fn tool_shaped_prefixes_are_held_until_classified() {
        assert!(might_be_tool_text(""));
        assert!(might_be_tool_text("   "));
        assert!(might_be_tool_text("{\"name\""));
        assert!(might_be_tool_text(" [{\"name\""));
        assert!(might_be_tool_text("```json"));
        assert!(!might_be_tool_text("Hello"));
        assert!(!might_be_tool_text(
            "Here is an example: {\"name\":\"read_file\"}"
        ));
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
