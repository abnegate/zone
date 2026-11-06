//! Reading `claude --output-format stream-json`.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use serde_json::value::RawValue;

use super::lenient;
use crate::llm::provider::event::{AgentEvent, subject};
use crate::llm::provider::occupancy::Occupancy;
use crate::llm::provider::output::Output;
use crate::llm::provider::subagent::{StepStatus, Subagent, SubagentStatus, SubagentStep};
use crate::llm::window;
use abnegate_llm::{Limit, ToolCall, Usage, Window};
use abnegate_secret::sanitize;

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

/// The system event claude opens a turn with, naming its session.
const INIT_SUBTYPE: &str = "init";

const TOO_MANY_REQUESTS: u16 = 429;

/// claude reports a window's use as a fraction; a [`Window`] holds a percent.
const PERCENT: f64 = 100.0;

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

/// claude's stream's name for each window of the plan, beside the name its usage endpoint gives
/// the same window.
const WINDOWS: [(&str, &str); 4] = [
    ("five_hour", window::FIVE_HOURS),
    ("seven_day", window::SEVEN_DAYS),
    ("seven_day_opus", window::SEVEN_DAYS_OPUS),
    ("seven_day_sonnet", window::SEVEN_DAYS_SONNET),
];

const FABLE: &str = "Fable";
const FABLE_LIMIT_REACHED: &str = "You've reached your Fable limit.";
const REQUIRES_USAGE_CREDITS: &str = " requires usage credits.";
const FABLE_VERSION_CHARACTERS: usize = 40;
const MIDDLE_DOT: char = '\u{b7}';
const LINE_BREAK: char = '\n';

/// The tools the main agent starts a subagent with. `Task` is the name older
/// releases gave `Agent`.
const AGENT_TOOLS: [&str; 2] = ["Agent", "Task"];

/// The kinds of task claude runs a subagent as. Every other task, such as a
/// command left running in the background, is no subagent.
const AGENT_TASKS: [&str; 2] = ["local_agent", "remote_agent"];

/// The status of a finished subagent's report, the `tool_use_result` of the
/// call that started it.
const COMPLETED: &str = "completed";

/// The statuses of a call that only launched its subagent, which then runs on
/// in the background, remotely or as a teammate and reports through task
/// events instead.
const LAUNCHED: [&str; 3] = ["async_launched", "remote_launched", "teammate_spawned"];

/// What a report's JSON opens with. serde reads a struct from an array too,
/// field by field in order, which would take a list's items for a report's.
const OBJECT: char = '{';

/// The `spawn_depth` of a subagent the main agent started itself.
const TOP_LEVEL_DEPTH: u64 = 1;

/// claude's words for where a task stands, as Zone's. A paused task is still
/// the reader's to wait on, and a killed one was stopped.
const TASK_STATUSES: [(&str, SubagentStatus); 7] = [
    ("pending", SubagentStatus::Running),
    ("running", SubagentStatus::Running),
    ("paused", SubagentStatus::Running),
    ("completed", SubagentStatus::Completed),
    ("failed", SubagentStatus::Failed),
    ("killed", SubagentStatus::Stopped),
    ("stopped", SubagentStatus::Stopped),
];

/// The system events claude tells of a task it runs beside the main agent
/// with, a subagent among them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskEvent {
    Started,
    Progress,
    Updated,
    Notification,
}

impl TaskEvent {
    const ALL: [Self; 4] = [
        Self::Started,
        Self::Progress,
        Self::Updated,
        Self::Notification,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Started => "task_started",
            Self::Progress => "task_progress",
            Self::Updated => "task_updated",
            Self::Notification => "task_notification",
        }
    }

    fn named(subtype: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|event| event.as_str() == subtype)
    }
}

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

/// The kinds of line claude streams that Zone reads, by the `type` each
/// names itself with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    System,
    Assistant,
    User,
    Result,
    RateLimit,
}

impl Kind {
    const ALL: [Self; 5] = [
        Self::System,
        Self::Assistant,
        Self::User,
        Self::Result,
        Self::RateLimit,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Assistant => "assistant",
            Self::User => "user",
            Self::Result => "result",
            Self::RateLimit => "rate_limit_event",
        }
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// A line's `type` alone, read before the rest so each kind is read straight
/// into its own shape. serde reads an enum tagged inside the line by copying
/// the whole line first, which would parse every tool's account of its call
/// for nothing.
#[derive(Debug, Deserialize)]
struct Tagged<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
}

#[derive(Debug, Deserialize)]
struct Assistant {
    #[serde(default)]
    message: Option<AssistantMessage>,
    #[serde(default)]
    parent_tool_use_id: Option<String>,
    #[serde(default)]
    is_api_error_message: bool,
    #[serde(default)]
    api_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct User {
    #[serde(default)]
    message: Option<UserMessage>,
    /// The tool's own account of the call the line returns, whose shape is
    /// the tool's. Only a subagent's report is ever read from it, so it is
    /// left unparsed until it is one.
    #[serde(default, alias = "toolUseResult")]
    tool_use_result: Option<Box<RawValue>>,
}

/// The line that ends a turn.
#[derive(Debug, Deserialize)]
struct Outcome {
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    usage: Option<TokenCounts>,
    #[serde(default, deserialize_with = "lenient")]
    api_error_status: Option<u16>,
    #[serde(default, rename = "modelUsage", deserialize_with = "lenient")]
    model_usage: Option<BTreeMap<String, ModelUsage>>,
}

#[derive(Debug, Deserialize)]
struct RateLimit {
    #[serde(default)]
    rate_limit_info: Option<Info>,
}

/// A system line: the one that opens the turn, or news of a task claude runs
/// beside the main agent. Every field is read leniently, so one in a new
/// shape costs only itself and never the session the line names.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct System {
    #[serde(deserialize_with = "lenient")]
    subtype: Option<String>,
    #[serde(deserialize_with = "lenient")]
    session_id: Option<String>,
    #[serde(deserialize_with = "lenient")]
    task_id: Option<String>,
    /// The call that started the task.
    #[serde(deserialize_with = "lenient")]
    tool_use_id: Option<String>,
    #[serde(deserialize_with = "lenient")]
    description: Option<String>,
    #[serde(deserialize_with = "lenient")]
    task_type: Option<String>,
    #[serde(deserialize_with = "lenient")]
    is_backgrounded: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    subagent_type: Option<String>,
    #[serde(deserialize_with = "lenient")]
    usage: Option<TaskUsage>,
    #[serde(deserialize_with = "lenient")]
    last_tool_name: Option<String>,
    #[serde(deserialize_with = "lenient")]
    summary: Option<String>,
    #[serde(deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(deserialize_with = "lenient")]
    patch: Option<Patch>,
    /// Whether a subagent, rather than the main agent, started the task.
    #[serde(deserialize_with = "lenient")]
    owned_by_subagent: Option<bool>,
    /// How deep a subagent's task is nested: [`TOP_LEVEL_DEPTH`] for one the
    /// main agent started, one more for each subagent between them.
    #[serde(deserialize_with = "lenient")]
    spawn_depth: Option<u64>,
}

impl System {
    /// Whether the task belongs to an agent a subagent started, which the
    /// main agent never did.
    fn nested(&self) -> bool {
        self.owned_by_subagent == Some(true)
            || self
                .spawn_depth
                .is_some_and(|depth| depth > TOP_LEVEL_DEPTH)
    }
}

/// What a task has spent so far.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TaskUsage {
    #[serde(deserialize_with = "lenient")]
    total_tokens: Option<u64>,
    #[serde(deserialize_with = "lenient")]
    tool_uses: Option<u64>,
    #[serde(deserialize_with = "lenient")]
    duration_ms: Option<u64>,
}

/// The fields of a task that changed, of those Zone shows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Patch {
    #[serde(deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(deserialize_with = "lenient")]
    description: Option<String>,
    #[serde(deserialize_with = "lenient")]
    error: Option<String>,
    #[serde(deserialize_with = "lenient")]
    is_backgrounded: Option<bool>,
}

/// The input of a call that starts a subagent.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Delegation {
    #[serde(deserialize_with = "lenient")]
    description: Option<String>,
    #[serde(deserialize_with = "lenient")]
    subagent_type: Option<String>,
    #[serde(deserialize_with = "lenient")]
    model: Option<String>,
    #[serde(deserialize_with = "lenient")]
    run_in_background: Option<bool>,
}

/// The `tool_use_result` of a call that started a subagent.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Report {
    #[serde(deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(rename = "resolvedModel", deserialize_with = "lenient")]
    model: Option<String>,
    #[serde(rename = "totalToolUseCount", deserialize_with = "lenient")]
    tool_uses: Option<u64>,
    #[serde(rename = "totalDurationMs", deserialize_with = "lenient")]
    duration_ms: Option<u64>,
    #[serde(rename = "totalTokens", deserialize_with = "lenient")]
    tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    content: Vec<Block>,
    #[serde(default)]
    usage: Option<TokenCounts>,
}

/// What a result reports for one model the turn ran on.
#[derive(Debug, Deserialize)]
struct ModelUsage {
    #[serde(default, rename = "contextWindow")]
    context_window: Option<u64>,
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
struct UserMessage {
    #[serde(default, deserialize_with = "lenient")]
    content: Option<Vec<Returned>>,
}

/// What claude hands the model back for one of its calls.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Returned {
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Ignored,
}

/// A tool result's text, which claude writes either as one string or as a
/// list of blocks of which only the text ones say anything.
fn printed(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
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

impl Block {
    fn call(self) -> Option<ToolCall> {
        match self {
            Self::ToolUse { id, name, input } => {
                Some(ToolCall::function(id, name, input.to_string()))
            }
            Self::Text { .. } | Self::Ignored => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Info {
    #[serde(default)]
    status: Option<String>,
    #[serde(default, rename = "rateLimitType")]
    name: Option<String>,
    #[serde(default, rename = "overageDisabledReason")]
    reason: Option<String>,
    #[serde(default, rename = "isUsingOverage")]
    on_credits: bool,
    #[serde(default, rename = "errorCode")]
    code: Option<String>,
    #[serde(default, rename = "resetsAt", deserialize_with = "reset")]
    resets_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "lenient")]
    utilization: Option<f64>,
}

impl Info {
    fn allowed(&self) -> bool {
        self.status
            .as_deref()
            .is_some_and(|status| ALLOWED.contains(&status))
    }

    fn headroom(&self) -> bool {
        self.on_credits || self.allowed()
    }

    /// A refused request as the worker reads a rate limit, naming its window.
    /// claude writes the typed line for a refused request right after its
    /// event, so an event that line can name better has none.
    fn refusal(&self) -> Option<String> {
        if self.headroom()
            || self.code.is_some()
            || self.name.as_deref() == Some(OVERAGE_INCLUDED_WINDOW)
        {
            return None;
        }
        let window = self.name.as_deref().unwrap_or(UNNAMED_WINDOW);
        let status = self.status.as_deref().unwrap_or_default();
        Some(format!("{THROTTLED} ({window}, {status})"))
    }

    /// The named window of the plan this event reports on, under the name
    /// claude's usage endpoint gives it, counted as spent in full when claude
    /// refused a request on it without saying how much was used. The window
    /// usage credits carry a turn past is no part of the plan's usage.
    fn window(&self, refused: bool) -> Option<Window> {
        let name = self
            .name
            .as_deref()
            .filter(|name| *name != OVERAGE_INCLUDED_WINDOW)?;
        let used_percent = self
            .utilization
            .map(|fraction| fraction * PERCENT)
            .or(refused.then_some(PERCENT));
        let mut window = Window::new(read_as(name));
        window.used_percent = used_percent;
        window.resets_at = self.resets_at;
        Some(window)
    }
}

/// `streamed`, a window as claude's stream names it, as its usage endpoint names it, or as it is
/// when the endpoint has no such window.
fn read_as(streamed: &str) -> &str {
    WINDOWS
        .iter()
        .find(|(name, _)| *name == streamed)
        .map_or(streamed, |(_, read)| read)
}

fn reset<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.as_ref().and_then(reset_time))
}

/// When a window resets, which claude has written both in epoch seconds and
/// as an RFC 3339 timestamp.
fn reset_time(value: &Value) -> Option<DateTime<Utc>> {
    let seconds = match value {
        Value::Number(seconds) => seconds.as_i64(),
        Value::String(text) => match DateTime::parse_from_rfc3339(text) {
            Ok(time) => return Some(time.with_timezone(&Utc)),
            Err(_) => text.trim().parse().ok(),
        },
        _ => None,
    };
    seconds.and_then(|seconds| DateTime::from_timestamp(seconds, 0))
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

impl TokenCounts {
    /// Every token the request sent and its answer took: the share of the
    /// window the request filled.
    fn filled(&self) -> u64 {
        [
            self.input_tokens,
            self.cache_read_input_tokens,
            self.cache_creation_input_tokens,
            self.output_tokens,
        ]
        .into_iter()
        .map(u64::from)
        .sum()
    }
}

impl From<TokenCounts> for Usage {
    fn from(counts: TokenCounts) -> Self {
        let prompt_tokens = counts
            .input_tokens
            .saturating_add(counts.cache_read_input_tokens)
            .saturating_add(counts.cache_creation_input_tokens);
        Self::new(prompt_tokens, counts.output_tokens)
    }
}

#[derive(Debug, Default)]
enum Credits {
    #[default]
    Unused,
    Unreported(String),
    Reported,
}

/// One turn of claude's stream, read a line at a time.
#[derive(Debug, Default)]
pub struct Reader {
    reason: Option<String>,
    /// The latest refused window. claude reports a subagent's refused request
    /// with the same event as the main agent's, naming neither, so the window
    /// fails the turn only on the main agent's API error or a failed result.
    refusal: Option<String>,
    /// When the latest refused request's window resets, and the window, which
    /// a limit that ends the turn reports.
    resets_at: Option<DateTime<Utc>>,
    window: Option<Window>,
    credits: Credits,
    /// Calls still waiting on their result, by the id claude gave each.
    calls: HashMap<String, ToolCall>,
    /// The model the main agent's requests ran on, which names its window
    /// among every model the result reports.
    model: Option<String>,
    /// The tokens the main agent's latest request and its answer took.
    used: Option<u64>,
    /// Whether each subagent runs in the background, by the id of the call
    /// that started it. A background subagent's call returns once it is
    /// launched, so its result is no report of the subagent's work.
    subagents: HashMap<String, bool>,
    /// The subagent each of a subagent's own calls belongs to, by call id.
    steps: HashMap<String, String>,
    /// The call that started each subagent's task, by task id: claude names
    /// only the task on some of its news of one.
    tasks: HashMap<String, String>,
    /// The subagent the main agent started that each agent a subagent
    /// started descends from, by the id of the call that started it. A
    /// nested agent's calls are steps of that subagent's run.
    nested: HashMap<String, String>,
    /// The subagents whose call returned their full report, which the
    /// shorter summary of a later task event must not replace.
    concluded: HashSet<String>,
}

impl Reader {
    pub fn interpret(&mut self, line: &str, events: &mut Vec<AgentEvent>) {
        let line = line.trim();
        let Some(kind) = serde_json::from_str::<Tagged>(line)
            .ok()
            .and_then(|tagged| Kind::named(&tagged.kind))
        else {
            return;
        };

        match kind {
            Kind::System => {
                if let Ok(system) = serde_json::from_str::<System>(line) {
                    self.system(system, events);
                }
            }
            Kind::Assistant => {
                let Ok(assistant) = serde_json::from_str::<Assistant>(line) else {
                    return;
                };
                match assistant.parent_tool_use_id {
                    Some(parent) => self.subagent(&parent, assistant.message, events),
                    None => self.assistant(
                        assistant.message,
                        assistant.is_api_error_message,
                        assistant.api_error.as_deref(),
                        events,
                    ),
                }
            }
            Kind::User => {
                if let Ok(user) = serde_json::from_str::<User>(line) {
                    self.returned(user.message, user.tool_use_result.as_deref(), events);
                }
            }
            Kind::Result => {
                let Ok(outcome) = serde_json::from_str::<Outcome>(line) else {
                    return;
                };
                events.extend(
                    self.occupancy(outcome.model_usage)
                        .map(AgentEvent::Occupancy),
                );
                self.result(
                    outcome.subtype,
                    outcome.is_error,
                    outcome.result,
                    outcome.usage,
                    outcome.api_error_status,
                    events,
                );
            }
            Kind::RateLimit => {
                if let Ok(RateLimit {
                    rate_limit_info: Some(info),
                }) = serde_json::from_str::<RateLimit>(line)
                {
                    self.observe(&info, events);
                }
            }
        }
    }

    fn system(&mut self, system: System, events: &mut Vec<AgentEvent>) {
        let Some(subtype) = system.subtype.as_deref() else {
            return;
        };
        if subtype == INIT_SUBTYPE {
            events.extend(
                system
                    .session_id
                    .filter(|id| !id.is_empty())
                    .map(AgentEvent::Session),
            );
            return;
        }
        let update = match TaskEvent::named(subtype) {
            Some(TaskEvent::Started) => self.begun(system),
            Some(TaskEvent::Progress) => self.progressed(system),
            Some(TaskEvent::Updated) => self.patched(system),
            Some(TaskEvent::Notification) => self.notified(system),
            None => None,
        };
        events.extend(update.and_then(changed).map(AgentEvent::Subagent));
    }

    /// A task claude started, a subagent when it is of an agent's kind, or
    /// of no kind and started by a call already known to start one. An agent
    /// a subagent started is none: its calls are its root subagent's steps.
    fn begun(&mut self, task: System) -> Option<Subagent> {
        if task.nested() {
            return None;
        }
        let id = task.tool_use_id.filter(|id| !id.is_empty())?;
        if self.nested.contains_key(&id) {
            return None;
        }
        let agent = match task.task_type.as_deref() {
            Some(kind) => AGENT_TASKS.contains(&kind),
            None => self.subagents.contains_key(&id),
        };
        if !agent {
            return None;
        }
        if let Some(task_id) = task.task_id {
            self.tasks.insert(task_id, id.clone());
        }
        let background = self.subagents.entry(id.clone()).or_default();
        if let Some(backgrounded) = task.is_backgrounded {
            *background = backgrounded;
        }
        Some(Subagent {
            description: shown(task.description),
            kind: shown(task.subagent_type),
            background: task.is_backgrounded,
            status: Some(SubagentStatus::Running),
            ..Subagent::new(id)
        })
    }

    fn progressed(&self, task: System) -> Option<Subagent> {
        let id = self.tracked(&task)?;
        let usage = task.usage.unwrap_or_default();
        Some(Subagent {
            kind: shown(task.subagent_type),
            tool_uses: usage.tool_uses,
            duration_ms: usage.duration_ms,
            tokens: usage.total_tokens,
            last_tool: shown(task.last_tool_name),
            summary: self.summary(&id, task.summary),
            ..Subagent::new(id)
        })
    }

    fn patched(&mut self, task: System) -> Option<Subagent> {
        let id = self.tracked(&task)?;
        let patch = task.patch?;
        if let Some(backgrounded) = patch.is_backgrounded {
            self.subagents.insert(id.clone(), backgrounded);
        }
        Some(Subagent {
            description: shown(patch.description),
            background: patch.is_backgrounded,
            status: patch.status.as_deref().and_then(task_status),
            error: shown(patch.error),
            ..Subagent::new(id)
        })
    }

    fn notified(&self, task: System) -> Option<Subagent> {
        let id = self.tracked(&task)?;
        let usage = task.usage.unwrap_or_default();
        Some(Subagent {
            status: task.status.as_deref().and_then(task_status),
            tool_uses: usage.tool_uses,
            duration_ms: usage.duration_ms,
            tokens: usage.total_tokens,
            summary: self.summary(&id, task.summary),
            ..Subagent::new(id)
        })
    }

    /// A task event's `summary` of subagent `id`, unless its call already
    /// returned the full report: claude falls back to the task's description
    /// for a summary it has none of.
    fn summary(&self, id: &str, summary: Option<String>) -> Option<String> {
        if self.concluded.contains(id) {
            return None;
        }
        shown(summary)
    }

    /// The subagent news of a task is about: the call that started it, named
    /// outright or through its task. A task that is no known subagent, such
    /// as a command run in the background, is none.
    fn tracked(&self, task: &System) -> Option<String> {
        let id = task
            .tool_use_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .or_else(|| self.tasks.get(task.task_id.as_deref()?))?;
        self.subagents.contains_key(id).then(|| id.clone())
    }

    /// The plan's window usage credits carried this turn past, the first time
    /// it is asked after claude said so.
    pub fn credits(&mut self) -> Option<String> {
        let Credits::Unreported(window) = &self.credits else {
            return None;
        };
        let window = window.clone();
        self.credits = Credits::Reported;
        Some(window)
    }

    fn assistant(
        &mut self,
        message: Option<AssistantMessage>,
        api_error_message: bool,
        api_error: Option<&str>,
        events: &mut Vec<AgentEvent>,
    ) {
        let kind = api_error.and_then(ApiError::named);
        if kind.is_some() || api_error_message {
            let words = message
                .as_ref()
                .map(AssistantMessage::words)
                .unwrap_or_default();
            let ended = match kind {
                Some(kind) => Some(self.unfunded(kind.marker(), &words)),
                None => self
                    .refusal
                    .as_deref()
                    .map(|window| self.limited(worded(window, &words), false)),
            };
            events.extend(ended);
            return;
        }
        // The main agent's request went through, so any window refused
        // before it was a subagent's.
        self.refusal = None;
        self.resets_at = None;
        self.window = None;
        let Some(message) = message else {
            return;
        };
        if let Some(model) = message.model.as_ref().filter(|model| !model.is_empty()) {
            self.model = Some(model.clone());
        }
        if let Some(counts) = &message.usage {
            self.used = Some(counts.filled());
        }
        for block in message.content {
            match block {
                Block::Text { text } => events.push(AgentEvent::Text(text)),
                block => {
                    let Some(call) = block.call() else {
                        continue;
                    };
                    let started = self.started(&call);
                    events.push(self.called(call));
                    events.extend(started.map(AgentEvent::Subagent));
                }
            }
        }
        if let Some(usage) = message.usage {
            events.push(AgentEvent::Usage(usage.into()));
        }
    }

    fn called(&mut self, call: ToolCall) -> AgentEvent {
        self.calls.insert(call.id.clone(), call.clone());
        AgentEvent::Tool(call)
    }

    /// The news that the main agent started a subagent with `call`, when it
    /// did.
    fn started(&mut self, call: &ToolCall) -> Option<Subagent> {
        if !AGENT_TOOLS.contains(&call.function.name.as_str()) {
            return None;
        }
        let delegation: Delegation =
            serde_json::from_str(&call.function.arguments).unwrap_or_default();
        self.subagents.insert(
            call.id.clone(),
            delegation.run_in_background.unwrap_or_default(),
        );
        Some(Subagent {
            description: shown(delegation.description),
            kind: shown(delegation.subagent_type),
            model: shown(delegation.model),
            background: delegation.run_in_background,
            status: Some(SubagentStatus::Running),
            ..Subagent::new(call.id.clone())
        })
    }

    /// A line of a subagent's own conversation. Its words, token counts and
    /// API errors stay there: claude hands a failure to the main agent as the
    /// result of the call that started the subagent. What it reaches for is
    /// a step of its own run, and still the turn's work for whoever observes
    /// the turn's tools. An agent nested inside it is a step too, and its
    /// own calls steps of the same run. A call by an agent no known subagent
    /// descends from is no step of any.
    fn subagent(
        &mut self,
        parent: &str,
        message: Option<AssistantMessage>,
        events: &mut Vec<AgentEvent>,
    ) {
        let root = self.root(parent);
        let calls = message
            .into_iter()
            .flat_map(|message| message.content)
            .filter_map(Block::call);
        for call in calls {
            if let Some(root) = &root {
                if AGENT_TOOLS.contains(&call.function.name.as_str()) {
                    self.nested.insert(call.id.clone(), root.clone());
                }
                self.steps.insert(call.id.clone(), root.clone());
                events.push(AgentEvent::Subagent(stepped(
                    root,
                    &call,
                    StepStatus::Running,
                )));
            }
            self.calls.insert(call.id.clone(), call);
        }
    }

    /// The subagent the main agent started that the agent `parent` names is,
    /// or descends from.
    fn root(&self, parent: &str) -> Option<String> {
        if self.subagents.contains_key(parent) {
            return Some(parent.to_string());
        }
        self.nested.get(parent).cloned()
    }

    fn returned(
        &mut self,
        message: Option<UserMessage>,
        report: Option<&RawValue>,
        events: &mut Vec<AgentEvent>,
    ) {
        let results = message
            .and_then(|message| message.content)
            .unwrap_or_default();
        for result in results {
            let Returned::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = result
            else {
                continue;
            };
            let Some(call) = self.calls.remove(&tool_use_id) else {
                continue;
            };
            let text = printed(&content);
            if let Some(parent) = self.steps.remove(&tool_use_id) {
                let status = if is_error {
                    StepStatus::Failed
                } else {
                    StepStatus::Succeeded
                };
                events.push(AgentEvent::Subagent(stepped(&parent, &call, status)));
            }
            events.extend(
                self.reported(&tool_use_id, &text, is_error, report)
                    .and_then(changed)
                    .map(AgentEvent::Subagent),
            );
            events.push(AgentEvent::Output(Output::new(call, text, !is_error)));
        }
    }

    /// What the result of the call that started subagent `id` says of it. A
    /// foreground subagent's result is its report; a launched one's says only
    /// that it runs on, and its report comes later as task news.
    fn reported(
        &mut self,
        id: &str,
        text: &str,
        is_error: bool,
        report: Option<&RawValue>,
    ) -> Option<Subagent> {
        let background = *self.subagents.get(id)?;
        let report: Report = report
            .map(|report| report.get().trim_start())
            .filter(|report| report.starts_with(OBJECT))
            .and_then(|report| serde_json::from_str(report).ok())
            .unwrap_or_default();
        let mut update = Subagent {
            model: shown(report.model),
            ..Subagent::new(id)
        };
        if is_error {
            update.status = Some(SubagentStatus::Failed);
            update.error = shown(Some(text));
            return Some(update);
        }
        match report.status.as_deref() {
            Some(status) if LAUNCHED.contains(&status) => {
                self.subagents.insert(id.to_string(), true);
                update.background = Some(true);
            }
            Some(COMPLETED) => {
                self.concluded.insert(id.to_string());
                update.status = Some(SubagentStatus::Completed);
                update.tool_uses = report.tool_uses;
                update.duration_ms = report.duration_ms;
                update.tokens = report.tokens;
                update.summary = shown(Some(text));
            }
            None if !background => {
                self.concluded.insert(id.to_string());
                update.status = Some(SubagentStatus::Completed);
                update.summary = shown(Some(text));
            }
            Some(_) | None => {}
        }
        Some(update)
    }

    /// The window of the model the main agent ran on, else the widest the
    /// result reports, with what the main agent's latest request filled.
    fn occupancy(&self, usage: Option<BTreeMap<String, ModelUsage>>) -> Option<Occupancy> {
        let usage = usage?;
        let window = self
            .model
            .as_deref()
            .and_then(|model| usage.get(model))
            .and_then(|entry| entry.context_window)
            .or_else(|| {
                usage
                    .values()
                    .filter_map(|entry| entry.context_window)
                    .max()
            });
        Occupancy::reported(window, self.used)
    }

    fn result(
        &self,
        subtype: Option<String>,
        is_error: bool,
        result: Option<String>,
        usage: Option<TokenCounts>,
        status: Option<u16>,
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
        let words = result.filter(|result| !result.trim().is_empty());
        let ended = match (words, &self.refusal) {
            (Some(words), _) if fable_refusal(&words) => self.unfunded(UNFUNDED, &words),
            (Some(words), Some(window)) => self.limited(worded(window, &words), false),
            (Some(words), None) => self.judged(words, status),
            (None, Some(window)) => self.limited(window.clone(), false),
            (None, None) => self.judged(
                subtype.unwrap_or_else(|| UNWORDED_FAILURE.to_string()),
                status,
            ),
        };
        events.push(ended);
    }

    fn observe(&mut self, info: &Info, events: &mut Vec<AgentEvent>) {
        self.reason.clone_from(&info.reason);
        if info.on_credits && matches!(self.credits, Credits::Unused) {
            let window = info.name.as_deref().unwrap_or(UNNAMED_WINDOW);
            self.credits = Credits::Unreported(window.to_string());
        }
        if info.headroom() {
            if info.allowed() && info.utilization.is_some() {
                events.extend(info.window(false).map(AgentEvent::Window));
            }
            return;
        }
        self.resets_at = info.resets_at;
        self.window = info.window(true);
        if let Some(refusal) = info.refusal() {
            self.refusal = Some(refusal);
        }
    }

    /// The turn refused past a limit, in `message`, with whatever claude said
    /// of the window it refused on.
    fn limited(&self, message: String, credits: bool) -> AgentEvent {
        let mut limit = Limit::new(message).with_credits(credits);
        limit.resets_at = self.resets_at;
        limit.window = self.window.clone();
        AgentEvent::Limited(limit)
    }

    /// A failed result no refused window accounts for, which is a limit when
    /// claude's status or its words say one refused the turn.
    fn judged(&self, message: String, status: Option<u16>) -> AgentEvent {
        if status == Some(TOO_MANY_REQUESTS) || Limit::worded(&message) {
            return self.limited(message, false);
        }
        AgentEvent::Failed(message)
    }

    /// claude's `words` for a turn usage credits could not fund, begun with
    /// `unfunded`, or a failure begun with [`UNCONFIRMED`] when claude could
    /// not look the account's credits up for the request it refused.
    fn unfunded(&self, unfunded: &str, words: &str) -> AgentEvent {
        match self.reason.as_deref() {
            Some(reason) if INDETERMINATE_REASONS.contains(&reason) => {
                AgentEvent::Failed(worded(&format!("{UNCONFIRMED} ({reason})"), words))
            }
            _ => self.limited(worded(unfunded, words), true),
        }
    }
}

/// `marker`, followed by claude's own `words` when it gave any.
fn worded(marker: &str, words: &str) -> String {
    let words = words.trim();
    if words.is_empty() {
        marker.to_string()
    } else {
        format!("{marker}: {words}")
    }
}

/// An agent's own words made safe to show, or nothing when they say nothing.
fn shown(text: Option<impl AsRef<str>>) -> Option<String> {
    let text = text?;
    let text = sanitize(text.as_ref());
    (!text.trim().is_empty()).then(|| text.into_owned())
}

/// `update`, unless it says nothing beyond which subagent it is about.
fn changed(update: Subagent) -> Option<Subagent> {
    (update != Subagent::new(update.id.as_str())).then_some(update)
}

/// The news of one of `parent`'s own calls standing at `status`. Only a call
/// just made is the subagent's latest tool.
fn stepped(parent: &str, call: &ToolCall, status: StepStatus) -> Subagent {
    let name = &call.function.name;
    Subagent {
        last_tool: (status == StepStatus::Running).then(|| name.clone()),
        step: Some(SubagentStep {
            id: call.id.clone(),
            name: name.clone(),
            subject: subject(&call.function.arguments),
            status,
        }),
        ..Subagent::new(parent)
    }
}

fn task_status(name: &str) -> Option<SubagentStatus> {
    TASK_STATUSES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, status)| *status)
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
    const LIMIT_HIT: &str = "You've hit your limit · resets 5pm";
    const OPUS_LIMIT: &str = "You've hit your Opus limit · resets Mon 9am";
    const OVERLOADED: &str = "API Error: Repeated 529 Overloaded errors";

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

    /// The words of every event that ended the turn unanswered, a limit's
    /// as much as a failure's: the worker reads both the same way.
    fn failures(events: &[AgentEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Failed(message) | AgentEvent::Limited(Limit { message, .. }) => {
                    Some(message.as_str())
                }
                _ => None,
            })
            .collect()
    }

    fn limits(events: &[AgentEvent]) -> Vec<&Limit> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Limited(limit) => Some(limit),
                _ => None,
            })
            .collect()
    }

    fn windows(events: &[AgentEvent]) -> Vec<&Window> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Window(window) => Some(window),
                _ => None,
            })
            .collect()
    }

    fn at(seconds: i64) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp(seconds, 0)
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
        failed_with(words, 429)
    }

    fn failed_with(words: &str, status: u16) -> Value {
        json!({
            "type": "result",
            "subtype": "success",
            "is_error": true,
            "api_error_status": status,
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

    fn throttled(window: &str, words: &str) -> String {
        format!("{THROTTLED} ({window}, rejected): {words}")
    }

    /// The event claude writes when it refuses a request past the plan's
    /// weekly Opus limit, which names no agent.
    fn refused_past_the_opus_limit() -> String {
        limit(json!({
            "status": "rejected",
            "resetsAt": 1_790_208_000,
            "rateLimitType": "seven_day_opus",
            "isUsingOverage": false,
        }))
    }

    const AGENT_CALL: &str = "toolu_01Agent";
    const ANSWER: &str = "Fable could not run on this account, so the review is mine: it is sound.";
    const OPUS_ANSWER: &str = "Opus is past its weekly limit, so the review is mine: it is sound.";

    fn said(words: &str) -> Value {
        json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": words}],
                "usage": {"input_tokens": 12, "cache_read_input_tokens": 3400, "output_tokens": 20},
            },
            "parent_tool_use_id": null,
            "session_id": "6f1",
        })
    }

    fn called(id: &str, name: &str, input: Value) -> Value {
        json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{"type": "tool_use", "id": id, "name": name, "input": input}],
            },
            "parent_tool_use_id": null,
            "session_id": "6f1",
        })
    }

    /// The main agent's call that starts a subagent on `model`.
    fn delegated(model: &str) -> Value {
        called(
            AGENT_CALL,
            "Agent",
            json!({
                "description": "Ask for a review",
                "prompt": "Review the change.",
                "subagent_type": "general-purpose",
                "model": model,
            }),
        )
    }

    /// `line` as claude writes it for a subagent: claude runs an Agent in the
    /// background by default and writes each of its lines into the stream,
    /// naming the call that started it, with its API errors and their kinds.
    fn subagents(mut line: Value) -> Value {
        line["parent_tool_use_id"] = AGENT_CALL.into();
        line["subagent_type"] = "general-purpose".into();
        line["task_description"] = "Ask for a review".into();
        line
    }

    fn succeeded(words: &str) -> Value {
        json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": words,
            "session_id": "6f1",
        })
    }

    fn refused_for_credits() -> String {
        limit(json!({
            "status": "rejected",
            "overageStatus": "rejected",
            "overageDisabledReason": "overage_not_provisioned",
            "isUsingOverage": false,
            "errorCode": "credits_required",
        }))
    }

    fn tools(events: &[AgentEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Tool(call) => Some(call.function.name.as_str()),
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
        assert_eq!(tools[0].call_type, "function");
        assert_eq!(tools[0].function.name, "Read");
        assert_eq!(tools[0].function.arguments, r#"{"file_path":"/w/main.rs"}"#);

        let last = events.last().expect("a terminal event");
        assert!(
            matches!(last, AgentEvent::Finished { finish_reason } if finish_reason.as_deref() == Some("success")),
            "expected a successful finish, got {last:?}"
        );
    }

    fn outputs(events: &[AgentEvent]) -> Vec<&Output> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Output(output) => Some(output),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tool_result_is_paired_with_the_call_it_answers() {
        let events = interpret_all(SESSION);

        let outputs = outputs(&events);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].call.id, "toolu_01A");
        assert_eq!(outputs[0].call.function.name, "Read");
        assert_eq!(outputs[0].text, "fn main() {}");
        assert!(outputs[0].succeeded);
    }

    #[test]
    fn a_result_in_blocks_or_marked_an_error_keeps_its_text_and_failure() {
        let lines = [
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "toolu_02", "name": "Bash", "input": {"command": "gh pr create --fill"}}]}, "parent_tool_use_id": null}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_02", "is_error": true, "content": [{"type": "text", "text": "a pull request already exists"}, {"type": "image"}]}]}}),
        ]
        .map(|line| line.to_string())
        .join("\n");

        let events = interpret_all(&lines);

        let outputs = outputs(&events);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].text, "a pull request already exists");
        assert!(!outputs[0].succeeded);
    }

    #[test]
    fn a_subagents_call_is_paired_too_and_an_unknown_result_is_ignored() {
        let lines = [
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "toolu_03", "name": "Bash", "input": {"command": "ls"}}]}, "parent_tool_use_id": "toolu_task"}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_03", "content": "a\nb"}]}, "parent_tool_use_id": "toolu_task"}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_unknown", "content": "x"}]}}),
            json!({"type": "user", "message": {"content": "a plain prompt"}}),
        ]
        .map(|line| line.to_string())
        .join("\n");

        let events = interpret_all(&lines);

        let outputs = outputs(&events);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].call.id, "toolu_03");
        assert_eq!(outputs[0].text, "a\nb");
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
    fn a_headroom_report_becomes_a_window_and_not_a_failure() {
        for status in ["allowed", "allowed_warning"] {
            let line = limit(json!({
                "status": status,
                "resetsAt": 1_790_208_000,
                "rateLimitType": "five_hour",
                "utilization": 0.43,
                "isUsingOverage": false,
            }));
            let events = interpret_all(&line);

            assert!(failures(&events).is_empty(), "{status}: {events:?}");
            assert_eq!(
                windows(&events),
                [&Window::new(window::FIVE_HOURS)
                    .with_used_percent(43.0)
                    .with_resets_at(at(1_790_208_000).expect("a timestamp"))],
                "{status}"
            );
        }
    }

    #[test]
    fn a_headroom_report_without_its_use_or_its_window_reports_no_window() {
        for info in [
            json!({"status": "allowed", "rateLimitType": "five_hour", "isUsingOverage": false}),
            json!({"status": "allowed", "utilization": 0.43, "isUsingOverage": false}),
        ] {
            let events = interpret_all(&limit(info.clone()));
            assert!(events.is_empty(), "{info}: {events:?}");
        }
    }

    #[test]
    fn a_refused_request_past_the_credits_window_is_a_limit_with_its_reset_time_and_no_window() {
        let events = interpret_all(FABLE_LIMIT_STREAM);

        let limits = limits(&events);
        assert!(!limits.is_empty(), "no limit in {events:?}");
        for limit in limits {
            assert!(limit.credits, "{limit:?}");
            assert_eq!(limit.resets_at, at(1_790_208_000), "{limit:?}");
            assert_eq!(
                limit.window, None,
                "the window usage credits carry a turn past is no part of the plan's usage"
            );
        }
    }

    #[test]
    fn a_window_is_named_as_claudes_usage_endpoint_names_it() {
        for (streamed, read) in [
            ("five_hour", window::FIVE_HOURS),
            ("seven_day", window::SEVEN_DAYS),
            ("seven_day_opus", window::SEVEN_DAYS_OPUS),
            ("seven_day_sonnet", window::SEVEN_DAYS_SONNET),
            ("hourly_burst", "hourly_burst"),
        ] {
            let reported = interpret_all(&limit(json!({
                "status": "allowed",
                "rateLimitType": streamed,
                "utilization": 0.43,
                "isUsingOverage": false,
            })));
            let refused = interpret_all(&stream(&[
                limit(json!({"status": "rejected", "rateLimitType": streamed})),
                api_error(SESSION_LIMIT, None).to_string(),
            ]));

            assert_eq!(
                windows(&reported)
                    .iter()
                    .map(|window| window.name.as_str())
                    .collect::<Vec<_>>(),
                [read],
                "{streamed}"
            );
            assert_eq!(
                limits(&refused)
                    .iter()
                    .map(|limit| limit.window.as_ref().map(|window| window.name.as_str()))
                    .collect::<Vec<_>>(),
                [Some(read)],
                "{streamed}"
            );
            assert_eq!(
                limits(&refused)[0].message,
                throttled(streamed, SESSION_LIMIT),
                "the worker still reads the window as claude names it"
            );
        }
    }

    #[test]
    fn a_report_on_the_credits_window_is_no_window_of_the_plan() {
        let events = interpret_all(&limit(json!({
            "status": "allowed",
            "rateLimitType": OVERAGE_INCLUDED_WINDOW,
            "utilization": 1.0,
            "isUsingOverage": false,
        })));

        assert!(windows(&events).is_empty(), "{events:?}");
    }

    #[test]
    fn a_rejected_window_keeps_the_wording_the_worker_reads() {
        let events = interpret_all(&stream(&[
            limit(json!({
                "status": "rejected",
                "resetsAt": 1_790_208_000,
                "rateLimitType": "five_hour",
                "utilization": 1.0,
                "isUsingOverage": false,
            })),
            api_error(SESSION_LIMIT, None).to_string(),
        ]));

        let [AgentEvent::Limited(limit)] = events.as_slice() else {
            panic!("expected one limit, got {events:?}");
        };
        assert_eq!(limit.message, throttled("five_hour", SESSION_LIMIT));
        assert!(limit.message.contains(THROTTLED), "{limit:?}");
        assert!(!limit.credits, "{limit:?}");
        assert_eq!(limit.resets_at, at(1_790_208_000));
        assert_eq!(
            limit.window.as_ref().and_then(|window| window.used_percent),
            Some(100.0)
        );
    }

    #[test]
    fn a_failed_result_in_limit_words_is_a_limit_without_a_reset_time() {
        for line in [
            failed_with(SESSION_LIMIT, 400),
            failed_with("Request refused", 429),
            json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "result": LIMIT_HIT}),
        ] {
            let events = interpret_all(&line.to_string());

            let [AgentEvent::Limited(limit)] = events.as_slice() else {
                panic!("expected one limit for {line}, got {events:?}");
            };
            assert_eq!(Some(limit.message.as_str()), line["result"].as_str());
            assert_eq!(limit.resets_at, None, "{line}");
            assert_eq!(limit.window, None, "{line}");
            assert!(!limit.credits, "{line}");
        }
    }

    #[test]
    fn a_signed_out_result_is_a_failure_and_not_a_limit() {
        for line in [
            failed_with("Not logged in · Please run /login", 401),
            failed_with("Invalid API key · Please run /login", 401),
            json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "result": "OAuth token has expired. Please obtain a new token or refresh your existing token."}),
        ] {
            let events = interpret_all(&line.to_string());

            assert!(
                matches!(events.as_slice(), [AgentEvent::Failed(message)] if Some(message.as_str()) == line["result"].as_str()),
                "{line}: {events:?}"
            );
        }
    }

    #[test]
    fn a_reset_time_reads_as_seconds_or_a_timestamp() {
        for (resets_at, expected) in [
            (json!(1_790_208_000), at(1_790_208_000)),
            (json!("1790208000"), at(1_790_208_000)),
            (json!("2026-09-24T04:00:00Z"), at(1_790_222_400)),
            (json!("2026-09-24T16:00:00+12:00"), at(1_790_222_400)),
            (json!("next Tuesday"), None),
            (json!({"seconds": 1_790_208_000}), None),
            (Value::Null, None),
        ] {
            let events = interpret_all(&stream(&[
                limit(json!({
                    "status": "rejected",
                    "resetsAt": resets_at,
                    "rateLimitType": "five_hour",
                    "isUsingOverage": false,
                })),
                api_error(SESSION_LIMIT, None).to_string(),
            ]));

            let [AgentEvent::Limited(limit)] = events.as_slice() else {
                panic!("{resets_at}: a reset time it cannot read lost the refusal: {events:?}");
            };
            assert_eq!(limit.resets_at, expected, "{resets_at}");
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
                let events = interpret_all(&stream(&[
                    limit(info),
                    api_error(LIMIT_HIT, None).to_string(),
                ]));

                assert_eq!(failures(&events), [throttled(window, LIMIT_HIT)]);
            }
        }
    }

    #[test]
    fn a_refused_request_that_names_no_window_is_a_rate_limit_on_the_request() {
        let events = interpret_all(&stream(&[
            limit(json!({"status": "rejected"})),
            api_error(LIMIT_HIT, None).to_string(),
        ]));

        assert_eq!(failures(&events), [throttled("request", LIMIT_HIT)]);
    }

    /// claude refuses a request past the plan's window on an event that
    /// names no agent, then writes the refusal as the line of the agent that
    /// sent it. The main agent's refusal fails the turn there, in claude's
    /// words after the window, and the failed result says the same.
    #[test]
    fn the_main_agents_refused_window_still_fails_the_turn_as_a_rate_limit() {
        let mut reader = Reader::default();
        let verdicts: Vec<Vec<String>> = [
            limit(json!({
                "status": "rejected",
                "resetsAt": 1_790_208_000,
                "rateLimitType": "five_hour",
                "isUsingOverage": false,
            })),
            api_error(SESSION_LIMIT, None).to_string(),
            failed_result(SESSION_LIMIT).to_string(),
        ]
        .iter()
        .map(|line| {
            let mut events = Vec::new();
            reader.interpret(line, &mut events);
            failures(&events).into_iter().map(str::to_string).collect()
        })
        .collect();

        let refused = throttled("five_hour", SESSION_LIMIT);
        assert_eq!(verdicts, [Vec::new(), vec![refused.clone()], vec![refused]]);
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
            let events = interpret_all(&stream(&[
                limit(info.clone()),
                api_error(SESSION_LIMIT, None).to_string(),
            ]));

            assert!(
                matches!(events.as_slice(), [AgentEvent::Limited(limit)] if limit.message.starts_with(THROTTLED)),
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

        assert_eq!(
            failures(&events).first().copied(),
            Some(throttled("five_hour", SESSION_LIMIT).as_str()),
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

            let limits = limits(&events);
            assert!(!limits.is_empty(), "no limit in {events:?}");
            assert!(
                limits
                    .iter()
                    .all(|limit| limit.credits && limit.message == unfunded(words)),
                "{limits:?}"
            );
            assert_eq!(
                failures(&events).len(),
                limits.len(),
                "a refusal was not a limit: {events:?}"
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
            assert!(
                limits(&events).first().is_some_and(|limit| limit.credits),
                "{reason}: {events:?}"
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
                assert!(
                    limits(&events).iter().all(|limit| !limit.credits),
                    "{reason}, {kind}: {events:?}"
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

    /// claude hands a subagent's refusal to the main agent as the result of
    /// the call that started it, and the main agent answers without it.
    #[test]
    fn a_subagents_refusal_leaves_the_main_agent_to_answer() {
        for (kind, words) in [
            (MODEL_REQUIRES_USAGE_CREDITS, REQUIRES_CREDITS),
            (LONG_CONTEXT_CREDITS_REQUIRED, LONG_CONTEXT),
        ] {
            let events = interpret_all(&stream(&[
                delegated("fable").to_string(),
                refused_for_credits(),
                subagents(api_error(words, Some(kind))).to_string(),
                said(ANSWER).to_string(),
                succeeded(ANSWER).to_string(),
            ]));

            assert!(failures(&events).is_empty(), "{kind}: {events:?}");
            assert_eq!(text(&events), ANSWER, "{kind}");
            assert!(
                matches!(events.last(), Some(AgentEvent::Finished { .. })),
                "{kind}: {events:?}"
            );
        }
    }

    /// A main agent on Sonnet starts a subagent on Opus past the plan's
    /// weekly Opus limit. claude refuses the subagent's request on an event
    /// that names no agent, hands the refusal to the main agent as the
    /// result of the call that started it, and the main agent answers.
    #[test]
    fn a_subagents_refused_window_leaves_the_main_agent_to_answer() {
        let events = interpret_all(&stream(&[
            delegated("opus").to_string(),
            refused_past_the_opus_limit(),
            subagents(api_error(OPUS_LIMIT, None)).to_string(),
            said(OPUS_ANSWER).to_string(),
            succeeded(OPUS_ANSWER).to_string(),
        ]));

        assert!(failures(&events).is_empty(), "{events:?}");
        assert_eq!(text(&events), OPUS_ANSWER);
        assert!(
            matches!(events.last(), Some(AgentEvent::Finished { .. })),
            "{events:?}"
        );
    }

    /// A line the main agent writes after a refused window shows that its
    /// own request went through, so the refusal was a subagent's, and a
    /// later failure of the main agent's is judged by its own words.
    #[test]
    fn a_refused_window_the_main_agent_writes_past_was_not_its_own() {
        let events = interpret_all(&stream(&[
            delegated("opus").to_string(),
            refused_past_the_opus_limit(),
            subagents(api_error(OPUS_LIMIT, None)).to_string(),
            said("Opus is past its weekly limit, so I will review the change.").to_string(),
            api_error(OVERLOADED, None).to_string(),
            failed_with(OVERLOADED, 529).to_string(),
        ]));

        assert!(
            matches!(events.last(), Some(AgentEvent::Failed(message)) if message == OVERLOADED),
            "{events:?}"
        );
    }

    /// A subagent's words are its own conversation's, and what it reaches
    /// for is a step of its run, not a line of the main agent's reasoning.
    #[test]
    fn a_subagents_words_never_reach_the_answer() {
        let events = interpret_all(&stream(&[
            delegated("fable").to_string(),
            subagents(said("Reading the diff first.")).to_string(),
            subagents(called(
                "toolu_02Read",
                "Read",
                json!({"file_path": "/w/main.rs"}),
            ))
            .to_string(),
            subagents(said("The change is sound.")).to_string(),
            said(ANSWER).to_string(),
            succeeded(ANSWER).to_string(),
        ]));

        assert_eq!(text(&events), ANSWER);
        assert_eq!(tools(&events), ["Agent"]);
    }

    #[test]
    fn a_subagents_token_counts_are_not_the_turns() {
        let mut subagent = subagents(said("The change is sound."));
        subagent["message"]["usage"] = json!({"input_tokens": 190_000, "output_tokens": 900});

        let events = interpret_all(&stream(&[
            delegated("fable").to_string(),
            subagent.to_string(),
            said(ANSWER).to_string(),
        ]));

        let prompts: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Usage(usage) => Some(usage.prompt_tokens),
                _ => None,
            })
            .collect();
        assert_eq!(prompts, [12 + 3400]);
    }

    #[test]
    fn the_main_agents_refusal_still_fails_the_turn_beside_a_subagent() {
        for (kind, words, marker) in [
            (MODEL_REQUIRES_USAGE_CREDITS, REQUIRES_CREDITS, UNFUNDED),
            (
                LONG_CONTEXT_CREDITS_REQUIRED,
                LONG_CONTEXT,
                UNFUNDED_CONTEXT,
            ),
        ] {
            let events = interpret_all(&stream(&[
                delegated("fable").to_string(),
                subagents(called(
                    "toolu_02Read",
                    "Read",
                    json!({"file_path": "/w/main.rs"}),
                ))
                .to_string(),
                refused_for_credits(),
                api_error(words, Some(kind)).to_string(),
                failed_result(words).to_string(),
            ]));

            assert_eq!(
                failures(&events).first().copied(),
                Some(format!("{marker}: {words}").as_str()),
                "{kind}: {events:?}"
            );
            assert_eq!(text(&events), "", "{kind}");
        }
    }

    /// A turn usage credits carried past the plan's window says so once, with
    /// the window, however many events repeat it.
    #[test]
    fn a_turn_on_usage_credits_reports_the_window_they_carried_it_past_once() {
        let mut reader = Reader::default();
        let mut events = Vec::new();
        let mut reported = Vec::new();
        let answer = SESSION.lines().nth(4).expect("the session's answer");
        let finished = SESSION.lines().last().expect("the session's result");
        for line in [
            limit(
                json!({"status": "allowed", "rateLimitType": "five_hour", "isUsingOverage": false}),
            ),
            limit(json!({
                "status": "rejected",
                "rateLimitType": "five_hour",
                "overageStatus": "allowed",
                "isUsingOverage": true,
            })),
            answer.to_string(),
            limit(json!({
                "status": "rejected",
                "rateLimitType": "seven_day",
                "overageStatus": "allowed_warning",
                "isUsingOverage": true,
            })),
            finished.to_string(),
        ] {
            reader.interpret(&line, &mut events);
            reported.extend(reader.credits());
        }

        assert_eq!(reported, ["five_hour"]);
        assert!(failures(&events).is_empty(), "{events:?}");
    }

    #[test]
    fn a_turn_inside_the_plans_window_reports_no_usage_credits() {
        let mut reader = Reader::default();
        let mut events = Vec::new();
        let headroom = limit(json!({
            "status": "allowed_warning",
            "rateLimitType": "five_hour",
            "overageStatus": "allowed",
            "isUsingOverage": false,
        }));
        for line in std::iter::once(headroom.as_str()).chain(SESSION.lines()) {
            reader.interpret(line, &mut events);
            assert_eq!(reader.credits(), None, "{line}");
        }
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
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key provided"}"#;
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

    /// A turn whose main agent ran on one model and its subagent on a wider
    /// one, in the shape claude 2.1.300's result reports `modelUsage` in.
    const CONTEXT_WINDOW_STREAM: &str = include_str!("fixtures/claude/context-window.jsonl");

    fn occupancies(events: &[AgentEvent]) -> Vec<Occupancy> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Occupancy(occupancy) => Some(*occupancy),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_result_reports_the_main_models_window_and_its_last_request() {
        let events = interpret_all(CONTEXT_WINDOW_STREAM);

        assert_eq!(
            occupancies(&events),
            [Occupancy {
                window: 200_000,
                used: Some(3 + 120 + 16_500 + 9),
            }],
            "the subagent's wider model and request are not the main agent's: {events:?}"
        );
        let finished = events
            .iter()
            .position(|event| matches!(event, AgentEvent::Finished { .. }));
        let reported = events
            .iter()
            .position(|event| matches!(event, AgentEvent::Occupancy(_)));
        assert!(
            reported < finished,
            "reported before the turn ends: {events:?}"
        );
    }

    #[test]
    fn a_result_naming_no_main_model_reports_its_widest_window() {
        let stream: String = CONTEXT_WINDOW_STREAM
            .lines()
            .filter(|line| !line.contains(r#""parent_tool_use_id":null"#))
            .map(|line| format!("{line}\n"))
            .collect();

        assert_eq!(
            occupancies(&interpret_all(&stream)),
            [Occupancy {
                window: 1_000_000,
                used: None,
            }]
        );
    }

    #[test]
    fn a_result_without_model_usage_reports_no_window() {
        assert!(occupancies(&interpret_all(SESSION)).is_empty());
        let unusable = CONTEXT_WINDOW_STREAM.replace(
            r#""modelUsage":{"#,
            r#""modelUsage":"unreported","replaced":{"#,
        );
        assert!(occupancies(&interpret_all(&unusable)).is_empty());
        assert!(
            interpret_all(&unusable)
                .iter()
                .any(|event| matches!(event, AgentEvent::Finished { .. })),
            "a modelUsage in another shape still finishes the turn"
        );
    }

    /// A foreground subagent and a background one, beside a command left
    /// running in the background, the background subagent finishing after
    /// the turn's first result. Put together from the shapes claude 2.1.300's
    /// own code writes these events in: no signed-in run has recorded one.
    const SUBAGENTS_STREAM: &str = include_str!("fixtures/claude/subagents.jsonl");

    const SURVEY: &str = "toolu_01Survey";
    const REVIEW: &str = "toolu_04Review";
    const BUILD: &str = "toolu_05Build";
    const TASK: &str = "a1b2c3d4e";

    fn updates<'a>(events: &'a [AgentEvent], id: &str) -> Vec<&'a Subagent> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Subagent(subagent) if subagent.id == id => Some(subagent),
                _ => None,
            })
            .collect()
    }

    fn last_update<'a>(events: &'a [AgentEvent], id: &str) -> Option<&'a Subagent> {
        updates(events, id).last().copied()
    }

    fn step(id: &str, name: &str, subject: &str, status: StepStatus) -> Option<SubagentStep> {
        Some(SubagentStep {
            id: id.to_string(),
            name: name.to_string(),
            subject: Some(subject.to_string()),
            status,
        })
    }

    fn task(subtype: &str, mut fields: Value) -> String {
        fields["type"] = "system".into();
        fields["subtype"] = subtype.into();
        fields["session_id"] = "6f1".into();
        fields.to_string()
    }

    /// claude starting the subagent [`AGENT_CALL`] as task [`TASK`].
    fn begun() -> String {
        task(
            "task_started",
            json!({
                "task_id": TASK,
                "tool_use_id": AGENT_CALL,
                "description": "Ask for a review",
                "task_type": "local_agent",
                "is_backgrounded": false,
            }),
        )
    }

    fn returned_to(id: &str, content: &str, is_error: bool, report: Value) -> String {
        json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error}]},
            "parent_tool_use_id": null,
            "session_id": "6f1",
            "tool_use_result": report,
        })
        .to_string()
    }

    #[test]
    fn a_foreground_subagent_is_followed_from_its_call_through_its_steps_to_its_report() {
        let events = interpret_all(SUBAGENTS_STREAM);

        let expected = [
            Subagent {
                description: Some("Survey the crate".to_string()),
                kind: Some("Explore".to_string()),
                model: Some("haiku".to_string()),
                status: Some(SubagentStatus::Running),
                ..Subagent::new(SURVEY)
            },
            Subagent {
                description: Some("Survey the crate".to_string()),
                kind: Some("Explore".to_string()),
                background: Some(false),
                status: Some(SubagentStatus::Running),
                ..Subagent::new(SURVEY)
            },
            Subagent {
                last_tool: Some("Grep".to_string()),
                step: step("toolu_02Grep", "Grep", "pub mod", StepStatus::Running),
                ..Subagent::new(SURVEY)
            },
            Subagent {
                step: step("toolu_02Grep", "Grep", "pub mod", StepStatus::Succeeded),
                ..Subagent::new(SURVEY)
            },
            Subagent {
                last_tool: Some("Read".to_string()),
                step: step(
                    "toolu_03Read",
                    "Read",
                    "/w/src/missing.rs",
                    StepStatus::Running,
                ),
                ..Subagent::new(SURVEY)
            },
            Subagent {
                step: step(
                    "toolu_03Read",
                    "Read",
                    "/w/src/missing.rs",
                    StepStatus::Failed,
                ),
                ..Subagent::new(SURVEY)
            },
            Subagent {
                model: Some("claude-haiku-4-5".to_string()),
                status: Some(SubagentStatus::Completed),
                tool_uses: Some(2),
                duration_ms: Some(5400),
                tokens: Some(18_200),
                summary: Some("The crate has two modules: llm and tools.".to_string()),
                ..Subagent::new(SURVEY)
            },
        ];
        assert_eq!(
            updates(&events, SURVEY),
            expected.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_background_subagent_runs_on_past_the_first_result_until_claude_says_it_finished() {
        let events = interpret_all(SUBAGENTS_STREAM);

        let expected = [
            Subagent {
                description: Some("Review the change".to_string()),
                kind: Some("general-purpose".to_string()),
                background: Some(true),
                status: Some(SubagentStatus::Running),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                description: Some("Review the change".to_string()),
                kind: Some("general-purpose".to_string()),
                background: Some(true),
                status: Some(SubagentStatus::Running),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                model: Some("claude-sonnet-4-6".to_string()),
                background: Some(true),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                last_tool: Some("Bash".to_string()),
                step: step(
                    "toolu_06Test",
                    "Bash",
                    "cargo test -p zone_core",
                    StepStatus::Running,
                ),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                kind: Some("general-purpose".to_string()),
                tool_uses: Some(1),
                duration_ms: Some(30_000),
                tokens: Some(9100),
                last_tool: Some("Bash".to_string()),
                summary: Some("Running the crate's tests".to_string()),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                step: step(
                    "toolu_06Test",
                    "Bash",
                    "cargo test -p zone_core",
                    StepStatus::Succeeded,
                ),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                status: Some(SubagentStatus::Completed),
                ..Subagent::new(REVIEW)
            },
            Subagent {
                status: Some(SubagentStatus::Completed),
                tool_uses: Some(3),
                duration_ms: Some(61_000),
                tokens: Some(21_000),
                summary: Some("The change is sound and its tests pass.".to_string()),
                ..Subagent::new(REVIEW)
            },
        ];
        assert_eq!(
            updates(&events, REVIEW),
            expected.iter().collect::<Vec<_>>()
        );

        let finished = events
            .iter()
            .position(|event| matches!(event, AgentEvent::Finished { .. }))
            .expect("the turn's first result");
        let after: Vec<&Subagent> = updates(&events[finished..], REVIEW);
        assert_eq!(
            after,
            expected[3..].iter().collect::<Vec<_>>(),
            "news after the first result still comes through"
        );
    }

    #[test]
    fn a_command_run_in_the_background_is_no_subagent() {
        let events = interpret_all(SUBAGENTS_STREAM);

        assert!(updates(&events, BUILD).is_empty(), "{events:?}");
        let ids: std::collections::BTreeSet<&str> = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Subagent(subagent) => Some(subagent.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, std::collections::BTreeSet::from([SURVEY, REVIEW]));
    }

    #[test]
    fn a_subagents_calls_reach_the_observer_and_never_the_main_agents_reasoning() {
        let events = interpret_all(SUBAGENTS_STREAM);

        assert_eq!(tools(&events), ["Agent", "Agent", "Bash"]);
        let answer = text(&events);
        assert_eq!(
            answer,
            "The crate has two modules. A review is running in the background.The review found the change sound."
        );
        let observed: Vec<&str> = outputs(&events)
            .iter()
            .map(|output| output.call.function.name.as_str())
            .collect();
        assert_eq!(observed, ["Grep", "Read", "Agent", "Agent", "Bash", "Bash"]);
    }

    #[test]
    fn a_legacy_task_call_answered_without_a_report_completes_with_its_words() {
        let events = interpret_all(CONTEXT_WINDOW_STREAM);

        assert_eq!(
            updates(&events, "toolu_02A"),
            [
                &Subagent {
                    description: Some("Survey the crate".to_string()),
                    status: Some(SubagentStatus::Running),
                    ..Subagent::new("toolu_02A")
                },
                &Subagent {
                    status: Some(SubagentStatus::Completed),
                    summary: Some("There are four modules.".to_string()),
                    ..Subagent::new("toolu_02A")
                },
            ]
        );
        assert_eq!(tools(&events), ["Task"]);
    }

    #[test]
    fn a_task_update_moves_the_subagent_by_claudes_words_for_where_it_stands() {
        for (word, status) in [
            ("pending", SubagentStatus::Running),
            ("running", SubagentStatus::Running),
            ("paused", SubagentStatus::Running),
            ("completed", SubagentStatus::Completed),
            ("failed", SubagentStatus::Failed),
            ("killed", SubagentStatus::Stopped),
        ] {
            let events = interpret_all(&stream(&[
                delegated("sonnet").to_string(),
                begun(),
                task(
                    "task_updated",
                    json!({
                        "task_id": TASK,
                        "patch": {
                            "status": word,
                            "description": "Ask for a second review",
                            "error": "\u{1b}[31mThe review stalled\u{1b}[0m",
                            "is_backgrounded": true,
                        },
                    }),
                ),
            ]));

            assert_eq!(
                last_update(&events, AGENT_CALL),
                Some(&Subagent {
                    description: Some("Ask for a second review".to_string()),
                    background: Some(true),
                    status: Some(status),
                    error: Some("The review stalled".to_string()),
                    ..Subagent::new(AGENT_CALL)
                }),
                "{word}"
            );
        }
    }

    #[test]
    fn a_task_update_that_changes_nothing_shown_is_no_news() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            begun(),
            task(
                "task_updated",
                json!({"task_id": TASK, "patch": {"end_time": 1_793_000_061_000_u64}}),
            ),
            task(
                "task_updated",
                json!({"task_id": TASK, "patch": {"status": "invented"}}),
            ),
        ]));

        assert_eq!(updates(&events, AGENT_CALL).len(), 2, "{events:?}");
    }

    #[test]
    fn a_task_notification_finishes_the_subagent_as_claude_says_it_ended() {
        for (word, status) in [
            ("completed", SubagentStatus::Completed),
            ("failed", SubagentStatus::Failed),
            ("stopped", SubagentStatus::Stopped),
        ] {
            let events = interpret_all(&stream(&[
                delegated("sonnet").to_string(),
                begun(),
                task(
                    "task_notification",
                    json!({
                        "task_id": TASK,
                        "status": word,
                        "output_file": "",
                        "summary": "Reviewed the change",
                        "usage": {"total_tokens": 500, "tool_uses": 4, "duration_ms": 9000},
                    }),
                ),
            ]));

            assert_eq!(
                last_update(&events, AGENT_CALL),
                Some(&Subagent {
                    status: Some(status),
                    tool_uses: Some(4),
                    duration_ms: Some(9000),
                    tokens: Some(500),
                    summary: Some("Reviewed the change".to_string()),
                    ..Subagent::new(AGENT_CALL)
                }),
                "{word}"
            );
        }
    }

    #[test]
    fn a_failed_subagent_call_fails_the_subagent_in_its_cleaned_words() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            returned_to(
                AGENT_CALL,
                "\u{1b}[31mThe subagent ran out of turns\u{1b}[0m",
                true,
                json!("Error: The subagent ran out of turns"),
            ),
        ]));

        assert_eq!(
            last_update(&events, AGENT_CALL),
            Some(&Subagent {
                status: Some(SubagentStatus::Failed),
                error: Some("The subagent ran out of turns".to_string()),
                ..Subagent::new(AGENT_CALL)
            })
        );
    }

    #[test]
    fn a_launched_subagents_call_result_is_no_report_of_its_work() {
        for status in LAUNCHED {
            let events = interpret_all(&stream(&[
                delegated("sonnet").to_string(),
                returned_to(
                    AGENT_CALL,
                    "Launched.",
                    false,
                    json!({"status": status, "agentId": "a1"}),
                ),
            ]));

            assert_eq!(
                last_update(&events, AGENT_CALL),
                Some(&Subagent {
                    background: Some(true),
                    ..Subagent::new(AGENT_CALL)
                }),
                "{status}"
            );
        }

        let mut background = delegated("sonnet");
        background["message"]["content"][0]["input"]["run_in_background"] = true.into();
        let events = interpret_all(&stream(&[
            background.to_string(),
            returned_to(AGENT_CALL, "Launched.", false, json!(null)),
        ]));
        assert_eq!(updates(&events, AGENT_CALL).len(), 1, "{events:?}");
    }

    #[test]
    fn a_subagents_description_is_cleaned_before_it_is_shown() {
        let mut call = delegated("sonnet");
        call["message"]["content"][0]["input"]["description"] =
            "\u{1b}]0;title\u{7}Ask \u{1b}[1mfor\u{1b}[0m a review".into();
        call["message"]["content"][0]["input"]["subagent_type"] = "  ".into();

        let events = interpret_all(&call.to_string());

        assert_eq!(
            last_update(&events, AGENT_CALL),
            Some(&Subagent {
                description: Some("Ask for a review".to_string()),
                model: Some("sonnet".to_string()),
                status: Some(SubagentStatus::Running),
                ..Subagent::new(AGENT_CALL)
            })
        );
    }

    #[test]
    fn a_task_without_a_kind_is_a_subagent_only_when_a_known_agent_call_started_it() {
        let untyped = |id: &str| {
            task(
                "task_started",
                json!({"task_id": "t9", "tool_use_id": id, "description": "Something"}),
            )
        };
        let events = interpret_all(&stream(&[
            called("toolu_09Bash", "Bash", json!({"command": "sleep 60"})).to_string(),
            untyped("toolu_09Bash"),
            task(
                "task_progress",
                json!({"task_id": "t9", "usage": {"total_tokens": 1, "tool_uses": 1, "duration_ms": 1}}),
            ),
            delegated("sonnet").to_string(),
            untyped(AGENT_CALL),
        ]));

        assert!(updates(&events, "toolu_09Bash").is_empty(), "{events:?}");
        assert_eq!(
            last_update(&events, AGENT_CALL).and_then(|update| update.status),
            Some(SubagentStatus::Running)
        );
    }

    #[test]
    fn an_unknown_task_event_a_malformed_field_or_a_stranger_task_is_ignored() {
        let events = interpret_all(&stream(&[
            r#"{"type":"system","subtype":"init","session_id":"6f1","status":42,"usage":"lots","patch":[]}"#.to_string(),
            delegated("sonnet").to_string(),
            begun(),
            task("task_invented", json!({"task_id": TASK, "status": "completed"})),
            task(
                "task_progress",
                json!({"task_id": TASK, "usage": "lots", "last_tool_name": "Bash"}),
            ),
            task(
                "task_notification",
                json!({"task_id": "elsewhere", "status": "completed", "summary": "Not ours"}),
            ),
            task(
                "task_notification",
                json!({"task_id": TASK, "status": ["completed"], "summary": 7}),
            ),
            r#"{"type":"system","subtype":"task_notification","task_id":"#.to_string(),
        ]));

        assert!(
            matches!(events.first(), Some(AgentEvent::Session(id)) if id == "6f1"),
            "{events:?}"
        );
        assert_eq!(
            updates(&events, AGENT_CALL)[2..],
            [&Subagent {
                last_tool: Some("Bash".to_string()),
                ..Subagent::new(AGENT_CALL)
            }]
        );
        assert!(updates(&events, "elsewhere").is_empty());
    }

    #[test]
    fn a_call_that_spawned_a_teammate_only_launched_its_subagent() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            returned_to(
                AGENT_CALL,
                "Spawned successfully.",
                false,
                json!({"status": "teammate_spawned", "agentId": "a1"}),
            ),
        ]));

        assert_eq!(
            last_update(&events, AGENT_CALL),
            Some(&Subagent {
                background: Some(true),
                ..Subagent::new(AGENT_CALL)
            })
        );
    }

    /// claude's notification falls back to the task's description for its
    /// summary, which must not replace the report the call returned.
    #[test]
    fn a_task_event_never_replaces_the_report_its_subagents_call_returned() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            begun(),
            task(
                "task_progress",
                json!({"task_id": TASK, "summary": "Reading the diff", "usage": {"tool_uses": 1}}),
            ),
            returned_to(
                AGENT_CALL,
                "The change is sound.",
                false,
                json!({"status": "completed", "totalToolUseCount": 3}),
            ),
            task(
                "task_progress",
                json!({"task_id": TASK, "summary": "Wrapping up", "usage": {"tool_uses": 3}}),
            ),
            task(
                "task_notification",
                json!({
                    "task_id": TASK,
                    "status": "completed",
                    "summary": "Ask for a review",
                    "usage": {"total_tokens": 500, "tool_uses": 3, "duration_ms": 9000},
                }),
            ),
        ]));

        let summaries: Vec<&str> = updates(&events, AGENT_CALL)
            .into_iter()
            .filter_map(|update| update.summary.as_deref())
            .collect();
        assert_eq!(summaries, ["Reading the diff", "The change is sound."]);
        assert_eq!(
            last_update(&events, AGENT_CALL),
            Some(&Subagent {
                status: Some(SubagentStatus::Completed),
                tool_uses: Some(3),
                duration_ms: Some(9000),
                tokens: Some(500),
                ..Subagent::new(AGENT_CALL)
            })
        );
    }

    const NESTED_CALL: &str = "toolu_10Nested";
    const NESTED_TASK: &str = "n1";

    /// `line` as claude writes it for the agent `parent` names.
    fn within(parent: &str, mut line: Value) -> Value {
        line["parent_tool_use_id"] = parent.into();
        line
    }

    fn steps(events: &[AgentEvent]) -> Vec<(&str, &str, StepStatus)> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Subagent(Subagent {
                    id,
                    step: Some(step),
                    ..
                }) => Some((id.as_str(), step.id.as_str(), step.status)),
                _ => None,
            })
            .collect()
    }

    fn subagent_ids(events: &[AgentEvent]) -> std::collections::BTreeSet<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Subagent(subagent) => Some(subagent.id.as_str()),
                _ => None,
            })
            .collect()
    }

    /// An agent a subagent started is a step of that subagent's run, as is
    /// everything it reaches for, and its own task news is none of the
    /// reader's: the main agent never started it.
    #[test]
    fn an_agent_a_subagent_started_rolls_up_into_the_subagent_the_main_agent_started() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            begun(),
            within(
                AGENT_CALL,
                called(
                    NESTED_CALL,
                    "Agent",
                    json!({"description": "Dig deeper", "subagent_type": "Explore"}),
                ),
            )
            .to_string(),
            task(
                "task_started",
                json!({
                    "task_id": NESTED_TASK,
                    "tool_use_id": NESTED_CALL,
                    "description": "Dig deeper",
                    "task_type": "local_agent",
                    "owned_by_subagent": true,
                    "spawn_depth": 2,
                }),
            ),
            within(
                NESTED_CALL,
                called("toolu_11Grep", "Grep", json!({"pattern": "fn main"})),
            )
            .to_string(),
            task(
                "task_progress",
                json!({"task_id": NESTED_TASK, "tool_use_id": NESTED_CALL, "summary": "Digging"}),
            ),
            returned_to("toolu_11Grep", "src/main.rs", false, json!(null)),
            task(
                "task_notification",
                json!({"task_id": NESTED_TASK, "tool_use_id": NESTED_CALL, "status": "completed", "summary": "Dug"}),
            ),
            returned_to(
                NESTED_CALL,
                "Found it.",
                false,
                json!({"status": "completed", "totalToolUseCount": 1}),
            ),
        ]));

        assert_eq!(
            subagent_ids(&events),
            std::collections::BTreeSet::from([AGENT_CALL])
        );
        assert_eq!(
            steps(&events),
            [
                (AGENT_CALL, NESTED_CALL, StepStatus::Running),
                (AGENT_CALL, "toolu_11Grep", StepStatus::Running),
                (AGENT_CALL, "toolu_11Grep", StepStatus::Succeeded),
                (AGENT_CALL, NESTED_CALL, StepStatus::Succeeded),
            ]
        );
        assert!(
            updates(&events, AGENT_CALL)
                .iter()
                .all(|update| update.summary.is_none()
                    && update.status != Some(SubagentStatus::Completed)),
            "{events:?}"
        );
    }

    /// claude marks an agent a subagent started either way, and either is
    /// enough, even for a call the reader never saw made.
    #[test]
    fn a_task_claude_says_is_nested_is_never_a_subagent_of_the_main_agent() {
        for nested in [
            json!({"owned_by_subagent": true}),
            json!({"spawn_depth": 2}),
            json!({"owned_by_subagent": true, "spawn_depth": 3}),
        ] {
            let mut started = json!({
                "task_id": NESTED_TASK,
                "tool_use_id": NESTED_CALL,
                "description": "Dig deeper",
                "task_type": "local_agent",
            });
            for (field, value) in nested.as_object().expect("the nested fields") {
                started[field] = value.clone();
            }

            let events = interpret_all(&stream(&[
                task("task_started", started),
                task(
                    "task_notification",
                    json!({"task_id": NESTED_TASK, "status": "completed", "summary": "Dug"}),
                ),
            ]));

            assert!(subagent_ids(&events).is_empty(), "{nested}: {events:?}");
        }

        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            task(
                "task_started",
                json!({
                    "task_id": TASK,
                    "tool_use_id": AGENT_CALL,
                    "task_type": "local_agent",
                    "owned_by_subagent": false,
                    "spawn_depth": TOP_LEVEL_DEPTH,
                }),
            ),
        ]));
        assert_eq!(
            updates(&events, AGENT_CALL).len(),
            2,
            "a task the main agent started is still news of its subagent: {events:?}"
        );
    }

    /// A task claude names no depth for is still nested when the reader saw
    /// a subagent make the call that started it.
    #[test]
    fn a_task_started_by_a_subagents_agent_call_is_nested_without_saying_so() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            within(
                AGENT_CALL,
                called(NESTED_CALL, "Task", json!({"description": "Dig deeper"})),
            )
            .to_string(),
            task(
                "task_started",
                json!({"task_id": NESTED_TASK, "tool_use_id": NESTED_CALL, "task_type": "local_agent"}),
            ),
        ]));

        assert_eq!(
            subagent_ids(&events),
            std::collections::BTreeSet::from([AGENT_CALL])
        );
    }

    #[test]
    fn a_call_by_an_agent_no_known_subagent_descends_from_is_no_step() {
        let events = interpret_all(&stream(&[
            delegated("sonnet").to_string(),
            within(
                "toolu_99Unknown",
                called("toolu_12Stray", "Read", json!({"file_path": "/w/x.rs"})),
            )
            .to_string(),
            returned_to("toolu_12Stray", "fn x() {}", false, json!(null)),
        ]));

        assert_eq!(
            subagent_ids(&events),
            std::collections::BTreeSet::from([AGENT_CALL])
        );
        assert!(steps(&events).is_empty(), "{events:?}");
        let observed: Vec<&str> = outputs(&events)
            .iter()
            .map(|output| output.call.function.name.as_str())
            .collect();
        assert_eq!(
            observed,
            ["Read"],
            "the stray call is still the turn's work"
        );
    }

    /// A subagent's report is read from the tool's account of its call only
    /// when the call started one; any other account, of any shape, is left
    /// unread.
    #[test]
    fn a_tools_account_of_its_call_is_read_only_for_a_subagent() {
        let events = interpret_all(&stream(&[
            called("toolu_20Read", "Read", json!({"file_path": "/w/a.rs"})).to_string(),
            returned_to(
                "toolu_20Read",
                "fn a() {}",
                false,
                json!({"status": "completed", "file": {"content": "fn a() {}"}}),
            ),
            delegated("sonnet").to_string(),
            returned_to(AGENT_CALL, "Done.", false, json!([1, "two", {"three": 3}])),
        ]));

        assert!(updates(&events, "toolu_20Read").is_empty(), "{events:?}");
        assert_eq!(
            last_update(&events, AGENT_CALL),
            Some(&Subagent {
                status: Some(SubagentStatus::Completed),
                summary: Some("Done.".to_string()),
                ..Subagent::new(AGENT_CALL)
            }),
            "an account in no report's shape reads as none"
        );
        assert_eq!(outputs(&events).len(), 2);
    }
}
