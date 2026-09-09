//! Task execution worker
//!
//! Executes agentic tasks in the background using the same streaming agent
//! loop as chat, with a task budget, a sandboxed tool context and a bounded
//! retry policy. Models resolve through the same installed-model catalogue
//! chat uses, so a task never pins a name the deployment cannot serve.

use futures::StreamExt;
use sqlx::PgPool;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;
use zone_core::agent::{AgentCallback, AgentPhase};
use zone_core::llm::{LlmClient, LlmConfig, Message as LlmMessage};
use zone_core::tools::ToolResult;

use crate::agent::{self, AgentEvent, AgentRun, ApprovalPolicy, ChatTools, LoopBudget};
use crate::db::{ai_settings, tasks, workspaces};
use crate::services::chat::session::{self, RunContext};
use crate::services::stages;
use crate::state::AppState;
use crate::workers::evaluation::{EvaluationSettings, Evaluator, Verdict};
use crate::workers::pr::{PrCreationResult, create_pr_for_task};
use zone_chat::capacity::Resolver;

const MAX_CONCURRENT_TASKS: usize = 5;
const TASK_TIMEOUT: Duration = Duration::from_secs(3600);

/// Matches the sampling temperature `session::build` gives an interactive chat.
const TASK_TEMPERATURE: f32 = 0.7;

const RUN_COMPLETED: &str = "completed";
const RUN_FAILED: &str = "failed";

const SOURCE_AGENT: &str = "agent";
const SOURCE_TOOL: &str = "tool";
const SOURCE_RETRY: &str = "retry";

const LEVEL_INFO: &str = "info";
const LEVEL_WARNING: &str = "warning";
const LEVEL_ERROR: &str = "error";

const NO_MODEL: &str =
    "No completion model is installed or configured for this workspace, so the task cannot run";

static TASK_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_semaphore() -> &'static Arc<Semaphore> {
    TASK_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS)))
}

/// How a failed attempt may be recovered.
///
/// A terminal failure is one a byte-identical retry cannot survive: bad
/// credentials, a malformed or oversized request, a missing model, a refusal,
/// or a run that already burned its whole budget. Retrying those spends the
/// budget again and can duplicate the side effects the attempt already had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Terminal,
    Transient,
    RateLimited { retry_after: Option<Duration> },
}

impl Failure {
    fn label(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Transient => "transient",
            Self::RateLimited { .. } => "rate_limited",
        }
    }

    fn retry_after(self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => retry_after,
            Self::Terminal | Self::Transient => None,
        }
    }
}

/// A failed attempt: what went wrong, and the metric it should be counted as.
#[derive(Debug, Clone)]
struct Fault {
    failure: Failure,
    status: &'static str,
    message: String,
}

impl Fault {
    fn agent(message: String) -> Self {
        Self {
            failure: classify(&message),
            status: RUN_FAILED,
            message,
        }
    }

    fn timeout() -> Self {
        Self {
            failure: Failure::Terminal,
            status: "timeout",
            message: format!(
                "Task execution timed out after {} seconds",
                TASK_TIMEOUT.as_secs()
            ),
        }
    }

    fn overloaded() -> Self {
        Self {
            failure: Failure::Terminal,
            status: "semaphore_denied",
            message: "System overload - semaphore closed".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Retry(Duration),
    Terminal,
    Exhausted,
}

impl Decision {
    fn label(self) -> &'static str {
        match self {
            Self::Retry(_) => "retry",
            Self::Terminal => "terminal",
            Self::Exhausted => "exhausted",
        }
    }

    fn delay(self) -> Option<Duration> {
        match self {
            Self::Retry(delay) => Some(delay),
            Self::Terminal | Self::Exhausted => None,
        }
    }
}

/// Bounded exponential backoff for a background task run.
///
/// `attempts` counts the initial attempt, so three retries follow the first
/// try. Rate limits start from their own base because a provider window
/// outlasts a network blip, and a provider hint replaces the curve entirely.
#[derive(Debug, Clone, Copy)]
struct RetryPolicy {
    attempts: u32,
    base: Duration,
    ceiling: Duration,
    rate_limit_base: Duration,
    rate_limit_ceiling: Duration,
    jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 4,
            base: Duration::from_secs(2),
            ceiling: Duration::from_secs(60),
            rate_limit_base: Duration::from_secs(15),
            rate_limit_ceiling: Duration::from_secs(300),
            jitter: 0.25,
        }
    }
}

impl RetryPolicy {
    fn decide(&self, attempt: u32, failure: Failure, sample: f64) -> Decision {
        if matches!(failure, Failure::Terminal) {
            return Decision::Terminal;
        }
        if attempt >= self.attempts {
            return Decision::Exhausted;
        }
        Decision::Retry(self.delay(attempt, failure, sample))
    }

    /// Jitter only ever subtracts, so `ceiling` stays a true upper bound and a
    /// provider's own hint is never shortened into another rejection.
    fn delay(&self, attempt: u32, failure: Failure, sample: f64) -> Duration {
        if let Some(hint) = failure.retry_after() {
            return hint.min(self.rate_limit_ceiling);
        }
        let (base, ceiling) = match failure {
            Failure::RateLimited { .. } => (self.rate_limit_base, self.rate_limit_ceiling),
            Failure::Terminal | Failure::Transient => (self.base, self.ceiling),
        };
        let span = backoff(base, ceiling, attempt);
        span.mul_f64(1.0 - self.jitter.clamp(0.0, 1.0) * sample.clamp(0.0, 1.0))
    }
}

fn backoff(base: Duration, ceiling: Duration, attempt: u32) -> Duration {
    base.saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)))
        .min(ceiling)
}

fn sample() -> f64 {
    rand::random_range(0.0..1.0)
}

/// One recorded attempt, written to the run's log so its history explains
/// what was tried and why it stopped.
#[derive(Debug, Clone)]
struct Attempt {
    number: u32,
    failure: Failure,
    decision: Decision,
    message: String,
}

impl Attempt {
    fn level(&self) -> &'static str {
        match self.decision {
            Decision::Retry(_) => LEVEL_WARNING,
            Decision::Terminal | Decision::Exhausted => LEVEL_ERROR,
        }
    }

    fn summary(&self, attempts: u32) -> String {
        match self.decision {
            Decision::Retry(delay) => format!(
                "Attempt {} of {} failed ({}); retrying in {:.1}s: {}",
                self.number,
                attempts,
                self.failure.label(),
                delay.as_secs_f64(),
                self.message
            ),
            Decision::Terminal => format!(
                "Attempt {} of {} hit a terminal error and will not be retried: {}",
                self.number, attempts, self.message
            ),
            Decision::Exhausted => format!(
                "Attempt {} of {} failed ({}) and no attempts remain: {}",
                self.number,
                attempts,
                self.failure.label(),
                self.message
            ),
        }
    }

    fn metadata(&self, attempts: u32) -> serde_json::Value {
        serde_json::json!({
            "attempt": self.number,
            "attempts": attempts,
            "classification": self.failure.label(),
            "outcome": self.decision.label(),
            "delay_ms": self.decision.delay().map(milliseconds),
            "retry_after_ms": self.failure.retry_after().map(milliseconds),
            "error": self.message,
        })
    }
}

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug)]
struct Completed {
    outcome: TaskOutcome,
    attempts: u32,
}

#[derive(Debug)]
struct Stopped {
    fault: Fault,
    attempts: u32,
    exhausted: bool,
}

const RATE_LIMIT_MARKERS: &[&str] = &[
    "quota exceeded",
    "rate limit",
    "rate_limit",
    "ratelimit",
    "resource exhausted",
    "retry-after",
    "retry_after",
    "too many requests",
];

const TERMINAL_MARKERS: &[&str] = &[
    "access denied",
    "authentication",
    "bad request",
    "budget exhausted",
    "canceled",
    "cancelled",
    "content policy",
    "content_filter",
    "content_policy",
    "context length",
    "context window",
    "invalid api key",
    "invalid model",
    "invalid request",
    "invalid_api_key",
    "invalid_request_error",
    "malformed",
    "maximum context",
    "model not found",
    "model_not_found",
    "no such model",
    "not supported",
    "permission denied",
    "unauthorized",
    "unsupported",
];

const TERMINAL_STATUSES: &[u16] = &[400, 401, 403, 404, 422];

const RETRY_AFTER_LABELS: &[&str] = &[
    "retry-after",
    "retry_after",
    "retry after",
    "retry in",
    "try again in",
];

/// Rate limits are matched first: a throttled request is recoverable even when
/// the provider dresses it up in the same vocabulary as a rejected one.
fn classify(message: &str) -> Failure {
    let lowered = message.to_ascii_lowercase();
    if RATE_LIMIT_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
        || standalone_status(&lowered, 429)
    {
        return Failure::RateLimited {
            retry_after: retry_after(&lowered),
        };
    }
    if TERMINAL_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
        || TERMINAL_STATUSES
            .iter()
            .any(|status| standalone_status(&lowered, *status))
    {
        return Failure::Terminal;
    }
    Failure::Transient
}

/// A status code only counts when it stands alone: not inside a longer number,
/// a request id, or a JSON value such as `"tokens":429`.
fn standalone_status(lowered: &str, status: u16) -> bool {
    let needle = status.to_string();
    let bytes = lowered.as_bytes();
    let mut from = 0;
    while let Some(offset) = lowered[from..].find(needle.as_str()) {
        let start = from + offset;
        let end = start + needle.len();
        let bounded = (start == 0 || !bytes[start - 1].is_ascii_alphanumeric())
            && (end >= bytes.len() || !bytes[end].is_ascii_alphanumeric());
        if bounded && !json_value(bytes, start) {
            return true;
        }
        from = start + 1;
    }
    false
}

fn json_value(bytes: &[u8], start: usize) -> bool {
    let colon = if start > 0 && bytes[start - 1] == b':' {
        start - 1
    } else if start > 1 && bytes[start - 1] == b' ' && bytes[start - 2] == b':' {
        start - 2
    } else {
        return false;
    };
    colon > 0 && bytes[colon - 1] == b'"'
}

fn retry_after(lowered: &str) -> Option<Duration> {
    RETRY_AFTER_LABELS
        .iter()
        .find_map(|label| lowered.split(label).nth(1).and_then(leading_duration))
}

fn leading_duration(tail: &str) -> Option<Duration> {
    let tail = tail.trim_start_matches([':', '=', ' ', '"', '\t']);
    let end = tail
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(tail.len());
    let value: f64 = tail[..end].parse().ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let unit = tail[end..].trim_start();
    let seconds = if unit.starts_with("ms") || unit.starts_with("millis") {
        value / 1000.0
    } else {
        value
    };
    Duration::try_from_secs_f64(seconds).ok()
}

/// Callback that persists task execution events to the database
///
/// Events are persisted asynchronously in spawned tasks to avoid blocking
/// the agent loop.
pub struct DatabaseTaskCallback {
    pool: PgPool,
    run_id: Uuid,
}

impl DatabaseTaskCallback {
    /// Create a new database task callback
    pub fn new(pool: PgPool, run_id: Uuid) -> Self {
        Self { pool, run_id }
    }
}

impl AgentCallback for DatabaseTaskCallback {
    fn on_phase_change(&self, phase: AgentPhase, message: Option<&str>) {
        let pool = self.pool.clone();
        let run_id = self.run_id;
        let phase_str = phase.to_string();
        let message_str = message.map(|s| s.to_string());

        tokio::spawn(async move {
            if let Err(e) =
                tasks::update_task_run_progress(&pool, run_id, Some(&phase_str), None).await
            {
                tracing::error!("Failed to update task run progress: {}", e);
            }

            let log_message =
                message_str.unwrap_or_else(|| format!("Entering {} phase", phase_str));

            if let Err(e) = tasks::add_task_run_log(
                &pool,
                run_id,
                &phase_str,
                SOURCE_AGENT,
                LEVEL_INFO,
                &log_message,
                None,
            )
            .await
            {
                tracing::error!("Failed to add task run log: {}", e);
            }
        });
    }

    fn on_tool_call(&self, tool_name: &str, args: &str) {
        let pool = self.pool.clone();
        let run_id = self.run_id;
        let tool_name = tool_name.to_string();
        let args = args.to_string();

        tokio::spawn(async move {
            let message = format!("Executing tool: {} with args: {}", tool_name, args);

            if let Err(e) = tasks::add_task_run_log(
                &pool,
                run_id,
                &AgentPhase::Acting.to_string(),
                SOURCE_TOOL,
                LEVEL_INFO,
                &message,
                Some(serde_json::json!({
                    "tool": tool_name,
                    "args": args,
                })),
            )
            .await
            {
                tracing::error!("Failed to add task run log: {}", e);
            }
        });
    }

    fn on_tool_result(&self, tool_name: &str, result: &ToolResult) {
        let pool = self.pool.clone();
        let run_id = self.run_id;
        let tool_name = tool_name.to_string();
        let result = result.clone();

        tokio::spawn(async move {
            let (log_level, message) = if result.success {
                (LEVEL_INFO, format!("Tool {} succeeded", tool_name))
            } else {
                (
                    LEVEL_ERROR,
                    format!("Tool {} failed: {:?}", tool_name, result.error),
                )
            };

            if let Err(e) = tasks::add_task_run_log(
                &pool,
                run_id,
                &AgentPhase::Acting.to_string(),
                SOURCE_TOOL,
                log_level,
                &message,
                Some(serde_json::json!({
                    "tool": tool_name,
                    "success": result.success,
                    "output": result.output,
                    "error": result.error,
                })),
            )
            .await
            {
                tracing::error!("Failed to add task run log: {}", e);
            }
        });
    }

    fn on_response(&self, response: &str) {
        let pool = self.pool.clone();
        let run_id = self.run_id;
        let response = response.to_string();

        tokio::spawn(async move {
            if let Err(e) = tasks::add_task_run_log(
                &pool,
                run_id,
                &AgentPhase::Responding.to_string(),
                SOURCE_AGENT,
                LEVEL_INFO,
                &response,
                None,
            )
            .await
            {
                tracing::error!("Failed to add task run log: {}", e);
            }
        });
    }
}

/// Execute a task run
///
/// This function runs the complete task execution pipeline:
/// 1. Fetches task details from database
/// 2. Resolves the completion model against the installed catalogue
/// 3. Gathers context if source_ids are specified
/// 4. Runs the agent loop under the retry policy, one semaphore permit per
///    attempt, releasing the permit before any backoff
/// 5. Updates status to "completed" or "failed"
///
/// All events are persisted to the database via DatabaseTaskCallback for monitoring.
pub async fn execute_task_run(state: &AppState, run_id: Uuid, task_id: Uuid) {
    let mut obs = crate::metrics::TaskObs::new();

    tracing::info!(
        "Starting task execution: run_id={}, task_id={}",
        run_id,
        task_id
    );

    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => {
            obs.set_status("not_found");
            tracing::error!("Task {} not found", task_id);
            fail(state.db(), run_id, "Task not found", None).await;
            return;
        }
        Err(error) => {
            obs.set_status("error");
            tracing::error!("Failed to fetch task {}: {}", task_id, error);
            fail(
                state.db(),
                run_id,
                &format!("Failed to fetch task: {}", error),
                None,
            )
            .await;
            return;
        }
    };

    // SECURITY: tools run against this directory only, so a GitHub-backed task
    // never escapes into the server's own working tree.
    let workspace_path = if task.github_repo_url.is_some() {
        std::env::temp_dir().join(format!("zone-task-{}", task_id))
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"))
    };

    let evaluator = Evaluator::detect(
        &workspace_path,
        EvaluationSettings::from_process_environment(),
    );
    let baseline = if evaluator.is_active() {
        record_evaluation_log(
            state,
            run_id,
            LEVEL_INFO,
            &format!(
                "Measuring code quality with {} tool(s) before the agent starts",
                evaluator.tools().len()
            ),
            None,
        )
        .await;
        evaluator.baseline().await
    } else {
        Vec::new()
    };

    let model = resolve_model(state, &task).await;
    if stages::is_auto(&model) {
        obs.set_status(RUN_FAILED);
        tracing::error!("Task {} has no resolvable completion model", task_id);
        fail(state.db(), run_id, NO_MODEL, None).await;
        return;
    }

    let guidance = guidance(state, &task).await;
    let prompt = format!("# Task: {}\n\n{}", task.title, task.description);
    let policy = RetryPolicy::default();
    let pool = state.db();
    let model = model.as_str();
    let prompt = prompt.as_str();
    let guidance = guidance.as_str();
    let workspace = workspace_path.as_path();

    let result = run_with_policy(
        policy,
        get_semaphore(),
        move |_| attempt_run(state, run_id, model, prompt, guidance, workspace),
        move |attempt| record_attempt(pool, run_id, policy, attempt),
    )
    .await;

    match result {
        Ok(completed) => {
            obs.set_status(RUN_COMPLETED);
            let summary = if completed.outcome.summary.trim().is_empty() {
                "Task completed".to_string()
            } else {
                completed.outcome.summary
            };

            tracing::info!(
                "Task run {} completed: tool_calls={}, attempts={}",
                run_id,
                completed.outcome.tool_calls,
                completed.attempts
            );

            let pr_info = match create_pr_for_task(state, task_id, &workspace_path).await {
                PrCreationResult::Created {
                    pr_url,
                    branch_name,
                } => {
                    tracing::info!("Created PR for task {}: {}", task_id, pr_url);
                    Some(serde_json::json!({
                        "pr_url": pr_url,
                        "branch_name": branch_name,
                    }))
                }
                PrCreationResult::NoChanges => {
                    tracing::info!("No changes to create PR for task {}", task_id);
                    None
                }
                PrCreationResult::NoRepository => {
                    tracing::info!("No repository configured for task {}", task_id);
                    None
                }
                PrCreationResult::PrAlreadyExists { pr_url } => {
                    tracing::info!("PR already exists for task {}: {}", task_id, pr_url);
                    Some(serde_json::json!({
                        "pr_url": pr_url,
                        "pr_already_existed": true,
                    }))
                }
                PrCreationResult::Error(err) => {
                    tracing::warn!("Failed to create PR for task {}: {}", task_id, err);
                    Some(serde_json::json!({
                        "pr_error": err,
                    }))
                }
            };

            let mut artifacts = serde_json::json!({
                "tool_calls": completed.outcome.tool_calls,
                "summary": summary,
                "attempts": completed.attempts,
            });

            if let Some(pr) = pr_info {
                artifacts["pr"] = pr;
            }

            let evaluation = evaluator.compare(baseline).await;
            if !evaluation.deltas.is_empty() {
                let level = if evaluation.has_regressions() {
                    LEVEL_WARNING
                } else {
                    LEVEL_INFO
                };
                let artifact = evaluation.artifact();
                record_evaluation_log(
                    state,
                    run_id,
                    level,
                    &evaluation.summary,
                    Some(artifact.clone()),
                )
                .await;
                artifacts["evaluation"] = artifact;

                if evaluation.verdict == Verdict::Regressed {
                    tracing::warn!(
                        "Task run {} regressed code quality in: {}",
                        run_id,
                        evaluation.regressed_tools().join(", ")
                    );
                }
            }

            if let Err(error) =
                tasks::complete_task_run(state.db(), run_id, RUN_COMPLETED, None, Some(artifacts))
                    .await
            {
                tracing::error!(
                    "CRITICAL: Failed to update run {} status: {}",
                    run_id,
                    error
                );
            }
        }
        Err(stopped) => {
            obs.set_status(if stopped.exhausted {
                "exhausted"
            } else {
                stopped.fault.status
            });
            tracing::error!(
                "Task run {} failed after {} attempt(s): {}",
                run_id,
                stopped.attempts,
                stopped.fault.message
            );
            let artifacts = serde_json::json!({
                "attempts": stopped.attempts,
                "classification": stopped.fault.failure.label(),
                "stopped": if stopped.exhausted { "exhausted" } else { "terminal" },
            });
            fail(state.db(), run_id, &stopped.fault.message, Some(artifacts)).await;
        }
    }
}

/// Runs attempts until one succeeds or the policy stops.
///
/// A permit is acquired per attempt and dropped before any backoff, so a
/// sleeping run never occupies a slot other runs are waiting on.
async fn run_with_policy<Run, Running, Record, Recording>(
    policy: RetryPolicy,
    permits: &Semaphore,
    mut run: Run,
    mut record: Record,
) -> Result<Completed, Stopped>
where
    Run: FnMut(u32) -> Running,
    Running: Future<Output = Result<TaskOutcome, Fault>>,
    Record: FnMut(Attempt) -> Recording,
    Recording: Future<Output = ()>,
{
    let mut number: u32 = 1;
    loop {
        let attempted = {
            let Ok(_permit) = permits.acquire().await else {
                return Err(Stopped {
                    fault: Fault::overloaded(),
                    attempts: number,
                    exhausted: false,
                });
            };
            run(number).await
        };
        let fault = match attempted {
            Ok(outcome) => {
                return Ok(Completed {
                    outcome,
                    attempts: number,
                });
            }
            Err(fault) => fault,
        };
        let decision = policy.decide(number, fault.failure, sample());
        record(Attempt {
            number,
            failure: fault.failure,
            decision,
            message: fault.message.clone(),
        })
        .await;
        match decision {
            Decision::Retry(delay) => {
                tracing::warn!(
                    attempt = number,
                    ?delay,
                    classification = fault.failure.label(),
                    error = %fault.message,
                    "Retrying task run"
                );
                tokio::time::sleep(delay).await;
                number += 1;
            }
            Decision::Terminal => {
                return Err(Stopped {
                    fault,
                    attempts: number,
                    exhausted: false,
                });
            }
            Decision::Exhausted => {
                return Err(Stopped {
                    fault,
                    attempts: number,
                    exhausted: true,
                });
            }
        }
    }
}

const EVALUATION_PHASE: &str = "evaluating";
const EVALUATION_AGENT: &str = "evaluation";

async fn record_evaluation_log(
    state: &AppState,
    run_id: Uuid,
    log_level: &str,
    message: &str,
    metadata: Option<serde_json::Value>,
) {
    if let Err(error) = tasks::add_task_run_log(
        state.db(),
        run_id,
        EVALUATION_PHASE,
        EVALUATION_AGENT,
        log_level,
        message,
        metadata,
    )
    .await
    {
        tracing::error!(
            "Failed to record evaluation log for run {}: {}",
            run_id,
            error
        );
    }
}

async fn attempt_run(
    state: &AppState,
    run_id: Uuid,
    model: &str,
    prompt: &str,
    guidance: &str,
    workspace: &Path,
) -> Result<TaskOutcome, Fault> {
    let tools = ChatTools::for_task(state, workspace.to_path_buf()).await;
    let mut system_prompt = agent::system_prompt(&tools, true);
    system_prompt.push_str(guidance);

    let capacity = Resolver::with_context(
        &state.config().litellm_host,
        &state.config().litellm_key,
        &state.config().ollama_host,
        Some(state.config().chat.context),
    )
    .resolve(model)
    .await;
    let policy = session::policy(&state.config().chat, &capacity);
    let mut llm = LlmClient::new(LlmConfig {
        base_url: state.config().litellm_host.clone(),
        api_key: state.config().litellm_key.clone(),
        default_model: model.to_string(),
        temperature: TASK_TEMPERATURE,
        max_tokens: policy.reserved,
    });
    if let Some(limit) = capacity.ollama {
        llm = llm.with_ollama_context(model, limit);
    }
    if capacity.reasoning
        && let Some(effort) = zone_core::llm::ReasoningEffort::Auto.resolve(prompt)
    {
        llm = llm.with_reasoning(model, effort);
    }

    let callback = DatabaseTaskCallback::new(state.db().clone(), run_id);
    let messages = vec![
        LlmMessage::system(system_prompt),
        LlmMessage::user(prompt.to_string()),
    ];
    let mut context = RunContext::from_messages(messages);
    context.policy = policy;
    context.reason = capacity.reason;

    match tokio::time::timeout(
        TASK_TIMEOUT,
        run_task_loop(llm, model.to_string(), tools, context, &callback),
    )
    .await
    {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(error)) => Err(Fault::agent(error)),
        Err(_) => Err(Fault::timeout()),
    }
}

/// Resolves the run's model the way a chat resolves its own: workspace settings
/// over org settings, then the installed catalogue, never a hardcoded name.
async fn resolve_model(state: &AppState, task: &tasks::TaskRow) -> String {
    let catalog = stages::Catalog::load(&state.config().ollama_host).await;
    let settings = match workspaces::get_workspace(state.db(), task.workspace_id).await {
        Ok(Some(workspace)) => ai_settings::get_effective_ai_settings(
            state.db(),
            workspace.organization_id,
            task.workspace_id,
        )
        .await
        .ok(),
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "Could not load workspace for task model selection");
            None
        }
    };
    stages::chat_model(
        task.model_name.as_deref().unwrap_or(stages::AUTO),
        &stages::Preferences::from_optional_settings(
            settings.as_ref(),
            &state.config().comfyui.classifier_model,
        ),
        &catalog,
        &format!("{}\n\n{}", task.title, task.description),
        false,
        true,
    )
}

async fn guidance(state: &AppState, task: &tasks::TaskRow) -> String {
    let mut guidance = String::from(
        "\n\nYou are completing a background coding task. Stay inside the sandboxed working directory.\n",
    );

    if let Some(source_ids) = &task.source_ids
        && !source_ids.is_empty()
        && let Some(context_service) = state.context_service()
    {
        tracing::info!(
            "Gathering context from {} sources for task {}",
            source_ids.len(),
            task.id
        );

        let search_query = format!("{}\n\n{}", task.title, task.description);

        match context_service
            .search(
                &search_query,
                20,
                Some(zone_context::embeddings::SearchFilters {
                    source_ids: Some(source_ids.clone()),
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(results) if !results.is_empty() => {
                guidance.push_str("\n# Relevant Context\n\n");
                guidance.push_str("The following context has been retrieved from the knowledge base to help with this task:\n\n");

                for (idx, result) in results.iter().enumerate() {
                    guidance.push_str(&format!(
                        "## Context {} (Relevance: {:.2})\n{}\n\n",
                        idx + 1,
                        result.similarity,
                        result.chunk_text
                    ));
                }

                tracing::info!(
                    "Added {} context chunks to task {} system prompt",
                    results.len(),
                    task.id
                );
            }
            Ok(_) => {
                tracing::info!("No relevant context found for task {}", task.id);
            }
            Err(error) => {
                tracing::warn!(
                    "Failed to gather context for task {}: {}. Proceeding without context.",
                    task.id,
                    error
                );
            }
        }
    }

    if let Some(criteria) = &task.acceptance_criteria {
        guidance.push_str(&format!("\n# Acceptance Criteria\n{}\n", criteria));
    }

    match crate::db::knowledge::standing_instructions_prompt(state.db(), task.workspace_id).await {
        Ok(instructions) => guidance.push_str(&instructions),
        Err(error) => tracing::warn!(
            task_id = %task.id,
            %error,
            "Failed to load standing instructions; continuing without them"
        ),
    }

    match crate::db::knowledge::learned_facts_prompt(state.db(), task.workspace_id).await {
        Ok(facts) => guidance.push_str(&facts),
        Err(error) => tracing::warn!(
            task_id = %task.id,
            %error,
            "Failed to load learned facts; continuing without them"
        ),
    }

    guidance
}

async fn record_attempt(pool: &PgPool, run_id: Uuid, policy: RetryPolicy, attempt: Attempt) {
    if let Err(error) = tasks::add_task_run_log(
        pool,
        run_id,
        &AgentPhase::Error.to_string(),
        SOURCE_RETRY,
        attempt.level(),
        &attempt.summary(policy.attempts),
        Some(attempt.metadata(policy.attempts)),
    )
    .await
    {
        tracing::error!("Failed to record task run attempt: {}", error);
    }
}

async fn fail(pool: &PgPool, run_id: Uuid, message: &str, artifacts: Option<serde_json::Value>) {
    if let Err(error) =
        tasks::complete_task_run(pool, run_id, RUN_FAILED, Some(message), artifacts).await
    {
        tracing::error!(
            "CRITICAL: Failed to update run {} status: {}",
            run_id,
            error
        );
    }
}

#[derive(Debug)]
struct TaskOutcome {
    summary: String,
    tool_calls: usize,
}

async fn run_task_loop(
    llm: LlmClient,
    model: String,
    tools: ChatTools,
    context: RunContext,
    callback: &DatabaseTaskCallback,
) -> Result<TaskOutcome, String> {
    callback.on_phase_change(AgentPhase::Thinking, None);
    let mut summary = String::new();
    let mut tool_calls = 0usize;
    let mut events = std::pin::pin!(agent::run_with_context(
        AgentRun {
            llm,
            model,
            tools,
            messages: Vec::new(),
            budget: LoopBudget::task(),
            approval: ApprovalPolicy::auto(),
        },
        context,
        true
    ));
    while let Some(event) = events.next().await {
        match event {
            AgentEvent::Chunk(text) => summary.push_str(&text),
            AgentEvent::ToolCallStarted {
                name, arguments, ..
            } => {
                callback.on_phase_change(AgentPhase::Acting, None);
                callback.on_tool_call(&name, &arguments);
            }
            AgentEvent::ToolCallCompleted {
                name,
                success,
                detail,
                ..
            } => {
                tool_calls += 1;
                let result = if success {
                    ToolResult::success(detail)
                } else {
                    ToolResult::error(detail)
                };
                callback.on_tool_result(&name, &result);
                callback.on_phase_change(AgentPhase::Observing, None);
            }
            AgentEvent::Canonical(entry) => {
                tasks::add_task_run_log(
                    &callback.pool,
                    callback.run_id,
                    &AgentPhase::Acting.to_string(),
                    SOURCE_AGENT,
                    LEVEL_INFO,
                    "Canonical conversation event",
                    Some(serde_json::json!({
                        "entry_id": entry.id,
                        "message": entry.message,
                        "mutations": entry.mutations,
                    })),
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            AgentEvent::Checkpoint { summary, .. } => {
                tasks::add_task_run_log(
                    &callback.pool,
                    callback.run_id,
                    &AgentPhase::Thinking.to_string(),
                    SOURCE_AGENT,
                    LEVEL_INFO,
                    "Conversation checkpoint",
                    Some(serde_json::to_value(summary).map_err(|error| error.to_string())?),
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            AgentEvent::Finalizing(reason) => {
                callback.on_phase_change(AgentPhase::Responding, Some(&reason));
            }
            AgentEvent::Consumed(_)
            | AgentEvent::Context(_)
            | AgentEvent::Usage(_)
            | AgentEvent::Image(_)
            | AgentEvent::Reasoning(_)
            | AgentEvent::ToolApprovalRequired { .. } => {}
            AgentEvent::Failed(error) => return Err(error),
        }
    }
    callback.on_phase_change(AgentPhase::Responding, Some(&summary));
    callback.on_response(&summary);
    Ok(TaskOutcome {
        summary,
        tool_calls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn outcome() -> TaskOutcome {
        TaskOutcome {
            summary: "done".to_string(),
            tool_calls: 0,
        }
    }

    fn retry_delay(policy: &RetryPolicy, attempt: u32, failure: Failure, sample: f64) -> Duration {
        match policy.decide(attempt, failure, sample) {
            Decision::Retry(delay) => delay,
            other => panic!("expected a retry, got {:?}", other),
        }
    }

    #[test]
    fn semaphore_starts_with_every_permit_available() {
        assert_eq!(get_semaphore().available_permits(), MAX_CONCURRENT_TASKS);
    }

    #[tokio::test]
    async fn database_callback_targets_its_own_run() {
        let pool = PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let run_id = Uuid::new_v4();
        let callback = DatabaseTaskCallback::new(pool, run_id);
        assert_eq!(callback.run_id, run_id);
    }

    #[test]
    fn hard_errors_classify_as_terminal() {
        for message in [
            "401 Unauthorized",
            "Invalid API key provided",
            "invalid_request_error: messages is malformed",
            "This model's maximum context length is 8192 tokens",
            "model not found: llama9",
            "Request blocked by content policy",
            "Run cancelled by the operator",
            "403 Forbidden",
            "404 no route for that deployment",
        ] {
            assert_eq!(
                classify(message),
                Failure::Terminal,
                "{message} must never be retried"
            );
        }
    }

    #[test]
    fn throttling_classifies_as_rate_limited() {
        for message in [
            "HTTP 429 Too Many Requests",
            "Rate limit exceeded for this deployment",
            "API quota exceeded for project",
            "resource exhausted, slow down",
        ] {
            assert!(
                matches!(classify(message), Failure::RateLimited { .. }),
                "{message} must back off rather than fail"
            );
        }
    }

    #[test]
    fn everything_else_classifies_as_transient() {
        for message in [
            "connection reset by peer",
            "503 Service Unavailable",
            "500 internal server error",
            "stream ended before the final chunk",
            "error sending request for url",
        ] {
            assert_eq!(
                classify(message),
                Failure::Transient,
                "{message} must be retried with backoff"
            );
        }
    }

    #[test]
    fn status_codes_only_count_when_they_stand_alone() {
        assert_eq!(classify("request 8842995 dropped"), Failure::Transient);
        assert_eq!(
            classify("{\"tokens\":429} stream ended"),
            Failure::Transient
        );
        assert!(matches!(
            classify("upstream returned 429"),
            Failure::RateLimited { .. }
        ));
    }

    #[test]
    fn terminal_failures_never_produce_a_retry() {
        let policy = RetryPolicy::default();
        for attempt in 1..=policy.attempts {
            assert_eq!(
                policy.decide(attempt, Failure::Terminal, 0.0),
                Decision::Terminal
            );
        }
    }

    #[test]
    fn run_timeouts_are_terminal() {
        let fault = Fault::timeout();
        assert_eq!(fault.failure, Failure::Terminal);
        assert_eq!(fault.status, "timeout");
        assert_eq!(
            RetryPolicy::default().decide(1, fault.failure, 0.0),
            Decision::Terminal
        );
    }

    #[test]
    fn rate_limits_back_off_further_than_transient_failures() {
        let policy = RetryPolicy::default();
        let throttled = classify("HTTP 429 Too Many Requests");
        assert_eq!(throttled, Failure::RateLimited { retry_after: None });
        let delay = retry_delay(&policy, 1, throttled, 0.0);
        assert_eq!(delay, policy.rate_limit_base);
        assert!(delay > retry_delay(&policy, 1, Failure::Transient, 0.0));
    }

    #[test]
    fn retry_after_hints_are_honoured_verbatim_and_bounded() {
        let policy = RetryPolicy::default();
        for message in [
            "429 rate limit exceeded; retry-after: 42",
            "Rate limited, please retry after 42 seconds",
            "429 slow down, try again in 42s",
        ] {
            assert_eq!(
                classify(message),
                Failure::RateLimited {
                    retry_after: Some(Duration::from_secs(42)),
                },
                "{message} carries a hint"
            );
            assert_eq!(
                retry_delay(&policy, 1, classify(message), 1.0),
                Duration::from_secs(42),
                "a hint must not be shortened by jitter"
            );
        }
        assert_eq!(
            retry_delay(
                &policy,
                1,
                classify("rate limit hit, retry-after: 86400"),
                0.0
            ),
            policy.rate_limit_ceiling
        );
        assert_eq!(
            classify("rate limit hit, retry-after: 500ms"),
            Failure::RateLimited {
                retry_after: Some(Duration::from_millis(500)),
            }
        );
    }

    #[test]
    fn backoff_grows_exponentially_within_its_bounds() {
        let policy = RetryPolicy::default();
        let floor = policy.base.mul_f64(1.0 - policy.jitter);
        for sample in [0.0, 0.5, 1.0] {
            let mut previous = Duration::ZERO;
            for attempt in 1..policy.attempts {
                let delay = retry_delay(&policy, attempt, Failure::Transient, sample);
                assert!(delay >= floor, "attempt {attempt} fell below the floor");
                assert!(
                    delay <= policy.ceiling,
                    "attempt {attempt} broke the ceiling"
                );
                assert!(delay >= previous, "attempt {attempt} did not grow");
                previous = delay;
            }
        }
        assert_eq!(
            retry_delay(&policy, 1, Failure::Transient, 0.0),
            policy.base
        );
        assert_eq!(
            retry_delay(&policy, 2, Failure::Transient, 0.0),
            policy.base * 2
        );
        assert_eq!(
            retry_delay(&policy, 1, Failure::Transient, 1.0),
            policy.base.mul_f64(1.0 - policy.jitter)
        );
        let long = RetryPolicy {
            attempts: 32,
            ..RetryPolicy::default()
        };
        assert_eq!(
            retry_delay(&long, 31, Failure::Transient, 0.0),
            long.ceiling,
            "an unbounded curve must still be capped"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_terminal_failure_runs_exactly_once() {
        let permits = Semaphore::new(MAX_CONCURRENT_TASKS);
        let runs = Arc::new(AtomicU32::new(0));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stopped = run_with_policy(
            RetryPolicy::default(),
            &permits,
            |_| {
                let runs = Arc::clone(&runs);
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    Err(Fault::agent("401 Unauthorized: invalid api key".into()))
                }
            },
            |attempt| {
                let recorded = Arc::clone(&recorded);
                async move { recorded.lock().unwrap().push(attempt) }
            },
        )
        .await
        .expect_err("a terminal failure must stop the run");

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(stopped.attempts, 1);
        assert!(!stopped.exhausted);
        assert_eq!(stopped.fault.failure, Failure::Terminal);
        let recorded = recorded.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].decision, Decision::Terminal);
        assert_eq!(recorded[0].level(), LEVEL_ERROR);
    }

    #[tokio::test(start_paused = true)]
    async fn transient_failures_stop_at_the_attempt_cap() {
        let policy = RetryPolicy::default();
        let permits = Semaphore::new(MAX_CONCURRENT_TASKS);
        let runs = Arc::new(AtomicU32::new(0));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stopped = run_with_policy(
            policy,
            &permits,
            |_| {
                let runs = Arc::clone(&runs);
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    Err(Fault::agent("connection reset by peer".into()))
                }
            },
            |attempt| {
                let recorded = Arc::clone(&recorded);
                async move { recorded.lock().unwrap().push(attempt) }
            },
        )
        .await
        .expect_err("the attempt cap must stop the run");

        assert_eq!(runs.load(Ordering::SeqCst), policy.attempts);
        assert_eq!(stopped.attempts, policy.attempts);
        assert!(stopped.exhausted);

        let recorded = recorded.lock().unwrap();
        assert_eq!(recorded.len() as u32, policy.attempts);
        assert_eq!(recorded.last().unwrap().decision, Decision::Exhausted);
        for (index, attempt) in recorded.iter().enumerate() {
            assert_eq!(attempt.number as usize, index + 1);
            assert_eq!(attempt.failure, Failure::Transient);
            assert!(
                attempt
                    .summary(policy.attempts)
                    .contains("connection reset")
            );
        }
        assert_eq!(
            recorded[0].metadata(policy.attempts)["classification"],
            "transient"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_recovered_run_reports_the_attempts_it_took() {
        let permits = Semaphore::new(MAX_CONCURRENT_TASKS);
        let completed = run_with_policy(
            RetryPolicy::default(),
            &permits,
            |number| async move {
                if number < 3 {
                    Err(Fault::agent("429 Too Many Requests".into()))
                } else {
                    Ok(outcome())
                }
            },
            |_| async {},
        )
        .await
        .expect("a transient failure must recover");

        assert_eq!(completed.attempts, 3);
        assert_eq!(permits.available_permits(), MAX_CONCURRENT_TASKS);
    }

    #[tokio::test(start_paused = true)]
    async fn a_backoff_never_holds_its_permit() {
        let permits = Arc::new(Semaphore::new(1));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<()>();
        let observer = tokio::spawn({
            let permits = Arc::clone(&permits);
            async move {
                receiver.recv().await;
                permits.available_permits()
            }
        });

        let completed = run_with_policy(
            RetryPolicy::default(),
            &permits,
            move |number| {
                let sender = sender.clone();
                async move {
                    if number == 1 {
                        let _ = sender.send(());
                        Err(Fault::agent("connection reset by peer".into()))
                    } else {
                        Ok(outcome())
                    }
                }
            },
            |_| async {},
        )
        .await
        .expect("the retry must acquire a fresh permit");

        assert_eq!(completed.attempts, 2);
        assert_eq!(
            observer.await.unwrap(),
            1,
            "the permit must be free while the run backs off"
        );
        assert_eq!(permits.available_permits(), 1);
    }
}
