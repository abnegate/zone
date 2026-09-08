//! Task execution worker
//!
//! Executes agentic tasks in the background using the same streaming agent
//! loop as chat, with a task budget and a sandboxed tool context.

use futures::StreamExt;
use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use uuid::Uuid;
use zone_core::agent::{AgentCallback, AgentPhase};
use zone_core::llm::{LlmClient, LlmConfig, Message as LlmMessage};
use zone_core::tools::ToolResult;

use crate::agent::{self, AgentEvent, AgentRun, ApprovalPolicy, ChatTools, LoopBudget};
use crate::db::tasks;
use crate::services::chat::session::{self, RunContext};
use crate::state::AppState;
use crate::workers::pr::{PrCreationResult, create_pr_for_task};
use zone_chat::capacity::Resolver;

// Max concurrent task executions
const MAX_CONCURRENT_TASKS: usize = 5;

// Timeout for task execution (1 hour)
const TASK_TIMEOUT_SECS: u64 = 3600;
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);
const HEARTBEAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

// LLM configuration defaults (overridable via environment variables)
fn default_temperature() -> f32 {
    std::env::var("ZONE_TASK_LLM_TEMPERATURE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.7)
}

fn default_max_tokens() -> u32 {
    std::env::var("ZONE_TASK_LLM_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8192)
}

fn default_model() -> String {
    std::env::var("ZONE_TASK_LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string())
}

// Global semaphore to limit concurrent task executions
static TASK_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_semaphore() -> &'static Arc<Semaphore> {
    TASK_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS)))
}

/// Callback that persists task execution events to the database
///
/// Events are persisted asynchronously in spawned tasks to avoid blocking
/// the agent loop.
#[derive(Clone)]
pub struct DatabaseTaskCallback {
    pool: PgPool,
    run_id: Uuid,
    owner: Option<Uuid>,
}

impl DatabaseTaskCallback {
    /// Create a new database task callback
    pub fn new(pool: PgPool, run_id: Uuid) -> Self {
        Self {
            pool,
            run_id,
            owner: None,
        }
    }
    async fn log(
        &self,
        phase: &str,
        agent: &str,
        level: &str,
        message: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<(), String> {
        match tasks::add_owned_task_run_log(
            &self.pool,
            self.run_id,
            self.owner,
            phase,
            agent,
            level,
            message,
            metadata,
        )
        .await
        {
            Ok(true) => Ok(()),
            Ok(false) => Err("Task execution lost its lease".to_string()),
            Err(error) => Err(format!("Could not persist task event: {error}")),
        }
    }
}

impl AgentCallback for DatabaseTaskCallback {
    fn on_phase_change(&self, phase: AgentPhase, message: Option<&str>) {
        let callback = self.clone();
        let phase = phase.to_string();
        let message = message
            .map(str::to_string)
            .unwrap_or_else(|| format!("Entering {phase} phase"));
        tokio::spawn(async move {
            if matches!(
                tasks::update_owned_task_run_progress(
                    &callback.pool,
                    callback.run_id,
                    callback.owner,
                    Some(&phase),
                    None
                )
                .await,
                Ok(Some(_))
            ) {
                let _ = callback.log(&phase, "agent", "info", &message, None).await;
            }
        });
    }

    fn on_tool_call(&self, name: &str, arguments: &str) {
        let callback = self.clone();
        let name = name.to_string();
        let arguments = arguments.to_string();
        tokio::spawn(async move {
            let _ = callback
                .log(
                    "acting",
                    "tool",
                    "info",
                    &format!("Executing tool: {name}"),
                    Some(serde_json::json!({"tool":name,"args":arguments})),
                )
                .await;
        });
    }

    fn on_tool_result(&self, name: &str, result: &ToolResult) {
        let callback = self.clone();
        let name = name.to_string();
        let result = result.clone();
        tokio::spawn(async move {
            let level = if result.success { "info" } else { "error" };
            let _ = callback.log("acting", "tool", level, &format!("Tool {name} finished"), Some(serde_json::json!({"tool":name,"success":result.success,"output":result.output,"error":result.error}))).await;
        });
    }

    fn on_response(&self, response: &str) {
        let callback = self.clone();
        let response = response.to_string();
        tokio::spawn(async move {
            let _ = callback
                .log("responding", "agent", "info", &response, None)
                .await;
        });
    }
}

/// Execute a task run
///
/// This function runs the complete task execution pipeline:
/// 1. Acquires semaphore permit to limit concurrent executions
/// 2. Updates run status to "running"
/// 3. Fetches task details from database
/// 4. Gathers context if source_ids are specified
/// 5. Initializes LLM client and agent
/// 6. Executes agent loop with DatabaseTaskCallback
/// 7. Updates status to "completed" or "failed"
///
/// All events are persisted to the database via DatabaseTaskCallback for monitoring.
/// Run recovery is durable across restarts and independent of agent progress.
pub fn spawn_recovery(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(error) = tasks::sweep_task_runs(state.db()).await {
                tracing::error!(%error, "Could not recover orphaned task runs");
            }
        }
    });
}

pub async fn execute_task_run(state: &AppState, run_id: Uuid, task_id: Uuid) {
    let owner = Uuid::new_v4();
    let run = match tasks::get_task_run(state.db(), run_id).await {
        Ok(Some(run)) if run.task_id == task_id => run,
        _ => return,
    };
    if let Some(actor) = run.triggered_by {
        let allowed = match tasks::get_task(state.db(), task_id).await {
            Ok(Some(task)) => crate::db::workspace_members::has_role_or_higher(
                state.db(),
                actor,
                task.workspace_id,
                crate::db::workspace_members::WorkspaceRole::Member,
            )
            .await
            .unwrap_or(false),
            _ => false,
        };
        if !allowed {
            let _ = tasks::complete_task_run(
                state.db(),
                run_id,
                "failed",
                Some("Workspace write access required"),
                None,
            )
            .await;
            return;
        }
    }
    if !matches!(
        tasks::claim_task_run(state.db(), run.id, owner).await,
        Ok(true)
    ) {
        return;
    }
    let heartbeat = async {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !matches!(
                tokio::time::timeout(
                    HEARTBEAT_TIMEOUT,
                    tasks::heartbeat_task_run(state.db(), run_id, owner)
                )
                .await,
                Ok(Ok(true))
            ) {
                tracing::warn!(%run_id, "Task execution lost its lease; cancelling pipeline");
                return;
            }
        }
    };
    tokio::select! {
        biased;
        () = heartbeat => {},
        () = execute_owned_task_run(state, run_id, task_id, owner) => {},
    }
}

async fn execute_owned_task_run(state: &AppState, run_id: Uuid, task_id: Uuid, owner: Uuid) {
    let mut obs = crate::metrics::TaskObs::new();

    // Acquire semaphore permit to limit concurrent executions
    let _permit = match get_semaphore().acquire().await {
        Ok(p) => p,
        Err(_) => {
            obs.set_status("semaphore_denied");
            tracing::error!("Task semaphore closed for run {}", run_id);
            if let Err(e) = tasks::complete_owned_task_run(
                state.db(),
                run_id,
                Some(owner),
                "failed",
                Some("System overload - semaphore closed"),
                None,
            )
            .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
            return;
        }
    };

    if !matches!(
        tasks::start_owned_task_run(state.db(), run_id, Some(owner)).await,
        Ok(true)
    ) {
        return;
    }

    tracing::info!(
        "Starting task execution: run_id={}, task_id={}",
        run_id,
        task_id
    );

    // Fetch task details
    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            obs.set_status("not_found");
            tracing::error!("Task {} not found", task_id);
            if let Err(e) = tasks::complete_owned_task_run(
                state.db(),
                run_id,
                Some(owner),
                "failed",
                Some("Task not found"),
                None,
            )
            .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
            return;
        }
        Err(e) => {
            obs.set_status("error");
            tracing::error!("Failed to fetch task {}: {}", task_id, e);
            if let Err(e) = tasks::complete_owned_task_run(
                state.db(),
                run_id,
                Some(owner),
                "failed",
                Some(&format!("Failed to fetch task: {}", e)),
                None,
            )
            .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
            return;
        }
    };

    let checkout =
        match crate::services::checkout::Checkout::prepare(state.db(), &task, run_id).await {
            Ok(checkout) => checkout,
            Err(error) => {
                obs.set_status("failed");
                if let Err(failure) = tasks::complete_owned_task_run(
                    state.db(),
                    run_id,
                    Some(owner),
                    "failed",
                    Some(&error),
                    None,
                )
                .await
                {
                    tracing::error!(%run_id, %failure, "Failed to record checkout failure");
                }
                return;
            }
        };
    let workspace_path = checkout.path().to_path_buf();

    let run = match tasks::get_task_run(state.db(), run_id).await {
        Ok(Some(run)) => run,
        _ => return,
    };
    let actor = task.created_by.and(run.triggered_by);
    let tools = ChatTools::for_task(state, workspace_path.clone(), task.workspace_id, actor)
        .await
        .with_task_lease(state.db().clone(), run_id, owner);
    let mut system_prompt = agent::system_prompt(&tools, true);
    system_prompt.push_str(
        "\n\nYou are completing a background coding task. Stay inside the sandboxed working directory.\n",
    );

    // Gather context if source_ids are specified
    if let Some(source_ids) = &task.source_ids
        && !source_ids.is_empty()
        && let Some(context_service) = state.context_service()
    {
        tracing::info!(
            "Gathering context from {} sources for task {}",
            source_ids.len(),
            task_id
        );

        // Build search query from task title and description
        let search_query = format!("{}\n\n{}", task.title, task.description);

        // Search for relevant context with source filtering
        match context_service
            .search(
                &search_query,
                20, // Limit to top 20 most relevant chunks
                Some(zone_context::embeddings::SearchFilters {
                    source_ids: Some(source_ids.clone()),
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(results) if !results.is_empty() => {
                system_prompt.push_str("\n# Relevant Context\n\n");
                system_prompt.push_str("The following context has been retrieved from the knowledge base to help with this task:\n\n");

                for (idx, result) in results.iter().enumerate() {
                    system_prompt.push_str(&format!(
                        "## Context {} (Relevance: {:.2})\n{}\n\n",
                        idx + 1,
                        result.similarity,
                        result.chunk_text
                    ));
                }

                tracing::info!(
                    "Added {} context chunks to task {} system prompt",
                    results.len(),
                    task_id
                );
            }
            Ok(_) => {
                tracing::info!("No relevant context found for task {}", task_id);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to gather context for task {}: {}. Proceeding without context.",
                    task_id,
                    e
                );
            }
        }
    }

    // Add acceptance criteria if available
    if let Some(criteria) = &task.acceptance_criteria {
        system_prompt.push_str(&format!("\n# Acceptance Criteria\n{}\n", criteria));
    }

    // Get LLM configuration (from task, then environment, then defaults)
    let temperature = default_temperature();
    let max_tokens = default_max_tokens();
    let model = task.model_name.clone().unwrap_or_else(default_model);

    let capacity = Resolver::with_context(
        &state.config().litellm_host,
        &state.config().litellm_key,
        &state.config().ollama_host,
        Some(state.config().chat.context),
    )
    .resolve(&model)
    .await;
    let settings = session::Settings {
        output: max_tokens,
        ..state.config().chat.clone()
    };
    let policy = session::policy(&settings, &capacity);
    let mut llm = LlmClient::new(LlmConfig {
        base_url: state.config().litellm_host.clone(),
        api_key: state.config().litellm_key.clone(),
        default_model: model.clone(),
        temperature,
        max_tokens: policy.reserved,
    });
    if let Some(limit) = capacity.ollama {
        llm = llm.with_ollama_context(&model, limit);
    }
    let prompt = format!("# Task: {}\n\n{}", task.title, task.description);
    if capacity.reasoning
        && let Some(effort) = zone_core::llm::ReasoningEffort::Auto.resolve(&prompt)
    {
        llm = llm.with_reasoning(&model, effort);
    }
    let callback = DatabaseTaskCallback {
        pool: state.db().clone(),
        run_id,
        owner: Some(owner),
    };
    let messages = vec![LlmMessage::system(system_prompt), LlmMessage::user(prompt)];

    let mut context = RunContext::from_messages(messages);
    context.policy = policy;
    context.reason = capacity.reason;
    let agent_future = run_task_loop(llm, model, tools, context, &callback);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(TASK_TIMEOUT_SECS),
        agent_future,
    )
    .await;

    match result {
        Ok(Ok(outcome)) => {
            obs.set_status("completed");
            let summary = if outcome.summary.trim().is_empty() {
                "Task completed".to_string()
            } else {
                outcome.summary
            };

            tracing::info!(
                "Task run {} completed: tool_calls={}",
                run_id,
                outcome.tool_calls
            );

            // Attempt PR creation if there are code changes
            if !matches!(
                tasks::heartbeat_task_run(state.db(), run_id, owner).await,
                Ok(true)
            ) {
                return;
            }
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

            // Build artifacts with PR info if available
            let mut artifacts = serde_json::json!({
                "tool_calls": outcome.tool_calls,
                "summary": summary,
            });

            if let Some(pr) = pr_info {
                artifacts["pr"] = pr;
            }

            if let Err(e) = tasks::complete_owned_task_run(
                state.db(),
                run_id,
                Some(owner),
                "completed",
                None,
                Some(artifacts),
            )
            .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
        }
        Ok(Err(e)) => {
            obs.set_status("failed");
            // Agent failed with error
            tracing::error!("Task run {} failed: {}", run_id, e);
            if let Err(e) = tasks::complete_owned_task_run(
                state.db(),
                run_id,
                Some(owner),
                "failed",
                Some(&e.to_string()),
                None,
            )
            .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
        }
        Err(_) => {
            obs.set_status("timeout");
            // Task timed out
            tracing::error!(
                "Task run {} timed out after {} seconds",
                run_id,
                TASK_TIMEOUT_SECS
            );
            if let Err(e) = tasks::complete_owned_task_run(
                state.db(),
                run_id,
                Some(owner),
                "failed",
                Some(&format!(
                    "Task execution timed out after {} seconds",
                    TASK_TIMEOUT_SECS
                )),
                None,
            )
            .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
        }
    }
}

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
                receipt,
                ..
            } => {
                if let Some(receipt) = receipt {
                    callback
                        .log(
                            "acting",
                            "tool",
                            "info",
                            "Workspace action receipt",
                            Some(serde_json::json!({"action_receipt": receipt})),
                        )
                        .await
                        .map_err(|error| format!("Could not persist action receipt: {error}"))?;
                }
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
                callback.log("acting","agent","info","Canonical conversation event",Some(serde_json::json!({"entry_id":entry.id,"message":entry.message,"mutations":entry.mutations}))).await.map_err(|error|error.to_string())?;
            }
            AgentEvent::Checkpoint { summary, .. } => {
                callback
                    .log(
                        "thinking",
                        "agent",
                        "info",
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

    #[test]
    fn test_semaphore_initialization() {
        assert!(Arc::ptr_eq(get_semaphore(), get_semaphore()));
    }

    #[tokio::test]
    async fn test_database_task_callback_creation() {
        let pool = PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let run_id = Uuid::new_v4();
        let callback = DatabaseTaskCallback::new(pool, run_id);
        assert_eq!(callback.run_id, run_id);
    }

    #[tokio::test]
    async fn capacity_waits_keep_heartbeats_and_stop_on_lease_loss() {
        let url = std::env::var("TEST_DATABASE_URL").expect("isolated TEST_DATABASE_URL");
        let pool = PgPool::connect(&url).await.unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug) VALUES ($1, 'Heartbeat test', $1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workspaces(id, organization_id, name, slug) VALUES ($1, $2, 'Heartbeat test', $1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        let task = tasks::create_task(
            &pool,
            workspace,
            &[],
            "Capacity",
            "Must not run",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        let run = tasks::create_task_run(&pool, task.id).await.unwrap();
        let permit = get_semaphore()
            .clone()
            .acquire_many_owned(MAX_CONCURRENT_TASKS as u32)
            .await
            .unwrap();
        let state = AppState::new(AppState::for_tests().config().clone(), pool.clone(), None);
        let execution = tokio::spawn(async move {
            execute_task_run(&state, run.id, task.id).await;
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let claimed: bool =
                    sqlx::query_scalar("SELECT owner IS NOT NULL FROM task_runs WHERE id = $1")
                        .bind(run.id)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                if claimed {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let first: chrono::DateTime<chrono::Utc> =
            sqlx::query_scalar("SELECT heartbeat_at FROM task_runs WHERE id = $1")
                .bind(run.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(16)).await;
        let refreshed: bool =
            sqlx::query_scalar("SELECT heartbeat_at > $2 FROM task_runs WHERE id = $1")
                .bind(run.id)
                .bind(first)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(refreshed, "capacity waiting must not look orphaned");
        assert_eq!(
            tasks::get_task(&pool, task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "queued"
        );
        sqlx::query("UPDATE task_runs SET owner = $2 WHERE id = $1")
            .bind(run.id)
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(16), execution)
            .await
            .unwrap()
            .unwrap();
        assert!(
            tasks::get_task_run_logs(&pool, run.id)
                .await
                .unwrap()
                .is_empty()
        );
        drop(permit);
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
    }

    // Integration tests are in zone_server/tests/task_execution_tests.rs

    #[tokio::test]
    async fn task_loop_persists_workspace_receipt_before_returning() {
        use crate::db::{organizations, users, workspace_members, workspaces};
        use serde_json::{Value, json};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, Request, ResponseTemplate};
        let database =
            std::env::var("TEST_DATABASE_URL").expect("explicit disposable TEST_DATABASE_URL");
        let pool = PgPool::connect(&database).await.unwrap();
        let organization = organizations::create_organization(
            &pool,
            "Task receipts",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Receipts",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let user = users::create_user(
            &pool,
            &format!("{}@example.com", Uuid::new_v4()),
            "unused",
            Some("Task actor"),
            false,
        )
        .await
        .unwrap();
        workspace_members::add_member(
            &pool,
            workspace.id,
            user.id,
            workspace_members::WorkspaceRole::Member,
            None,
        )
        .await
        .unwrap();
        let task = tasks::create_task(
            &pool,
            workspace.id,
            &[],
            "Receipt owner",
            "Run",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        let run = tasks::create_task_run(&pool, task.id).await.unwrap();
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let provider = MockServer::start().await;
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        Mock::given(method("POST")).and(path("/chat/completions")).respond_with(move |_: &Request| {
            let delta = if count.fetch_add(1, Ordering::SeqCst) == 0 {
                json!({"tool_calls":[{"index":0,"id":"receipt-call","type":"function","function":{"name":"create_task","arguments":r#"{"title":"Made by the scoped task","description":"durable result"}"#}}]})
            } else { json!({"content":"The task was created."}) };
            let chunk = json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let end = json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
            ResponseTemplate::new(200).insert_header("Content-Type", "text/event-stream").set_body_string(format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n"))
        }).mount(&provider).await;
        let tools =
            ChatTools::for_task(&state, std::env::temp_dir(), workspace.id, Some(user.id)).await;
        let callback = DatabaseTaskCallback::new(pool.clone(), run.id);
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            run_task_loop(
                LlmClient::new(LlmConfig {
                    base_url: provider.uri(),
                    ..LlmConfig::default()
                }),
                "test".into(),
                tools,
                RunContext::from_messages(vec![LlmMessage::user(
                    "Create a task titled Made by the scoped task with description durable result",
                )]),
                &callback,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(outcome.tool_calls, 1);
        let receipts: Vec<Value> = sqlx::query_scalar("SELECT metadata->'action_receipt' FROM task_run_logs WHERE task_run_id=$1 AND metadata ? 'action_receipt'").bind(run.id).fetch_all(&pool).await.unwrap();
        assert_eq!(
            receipts.len(),
            1,
            "task loop returned without its durable action receipt"
        );
        assert_eq!(receipts[0]["actor_id"], user.id.to_string());
        assert_eq!(receipts[0]["action"], "create_task");
        assert_eq!(receipts[0]["success"], true);
        let target = Uuid::parse_str(receipts[0]["target_id"].as_str().unwrap()).unwrap();
        let written = tasks::get_task(&pool, target).await.unwrap().unwrap();
        assert_eq!(written.workspace_id, workspace.id);
        assert_eq!(written.title, "Made by the scoped task");
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id=$1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn full_worker_scopes_actions_uses_checkout_and_cleans_up() {
        use crate::db::{organizations, users, workspace_members, workspaces};
        use serde_json::{Value, json};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, Request, ResponseTemplate};
        let database =
            std::env::var("TEST_DATABASE_URL").expect("explicit disposable TEST_DATABASE_URL");
        let pool = PgPool::connect(&database).await.unwrap();
        let organization = organizations::create_organization(
            &pool,
            "Task receipts",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Receipts",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let user = users::create_user(
            &pool,
            &format!("{}@example.com", Uuid::new_v4()),
            "unused",
            Some("Task actor"),
            false,
        )
        .await
        .unwrap();
        workspace_members::add_member(
            &pool,
            workspace.id,
            user.id,
            workspace_members::WorkspaceRole::Member,
            None,
        )
        .await
        .unwrap();
        let task = tasks::create_task_as(
            &pool,
            workspace.id,
            &[],
            "Receipt owner",
            "Run",
            None,
            None,
            true,
            None,
            Some(user.id),
        )
        .await
        .unwrap();
        let run = tasks::create_task_run_as(&pool, task.id, Some(user.id))
            .await
            .unwrap();
        let provider = MockServer::start().await;
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let observed = Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
        let checkout = observed.clone();
        Mock::given(method("POST")).and(path("/chat/completions")).respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            if let Some(messages) = body["messages"].as_array() {
                for message in messages {
                    if message["role"] == "tool"
                        && let Some(content) = message["content"].as_str() {
                            for line in content.lines() {
                                if line.starts_with('/') {
                                    let path = std::path::PathBuf::from(line.trim());
                                    assert!(path.file_name().unwrap().to_string_lossy().starts_with("zone-run-"), "worker used server cwd instead of a checkout: {}", path.display());
                                    assert!(path.is_dir(), "checkout disappeared during execution");
                                    *checkout.lock().unwrap() = Some(path);
                                }
                            }
                    }
                }
            }
            let delta = match count.fetch_add(1, Ordering::SeqCst) {
                0 => json!({"tool_calls":[{"index":0,"id":"cwd-call","type":"function","function":{"name":"run_command","arguments":r#"{"command":"pwd"}"#}}]}),
                1 => json!({"tool_calls":[{"index":0,"id":"write-call","type":"function","function":{"name":"write_file","arguments":r#"{"path":"sentinel","content":"isolated"}"#}}]}),
                2 => json!({"tool_calls":[{"index":0,"id":"receipt-call","type":"function","function":{"name":"create_task","arguments":r#"{"title":"Made by the scoped task","description":"durable result"}"#}}]}),
                _ => {
                    let directory = checkout.lock().unwrap().clone().expect("pwd did not return a checkout");
                    assert_eq!(std::fs::read_to_string(directory.join("sentinel")).unwrap(), "isolated");
                    json!({"content":"The task was created."})
                }
            };
            let chunk = json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let end = json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
            ResponseTemplate::new(200).insert_header("Content-Type", "text/event-stream").set_body_string(format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n"))
        }).mount(&provider).await;
        let mut config = crate::state::test_config();
        config.litellm_host = provider.uri();
        config.ollama_host = provider.uri();
        let state = AppState::new(config, pool.clone(), None);
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            execute_task_run(&state, run.id, task.id),
        )
        .await
        .unwrap();
        let completed = tasks::get_task_run(&pool, run.id).await.unwrap().unwrap();
        assert_eq!(
            completed.status, "completed",
            "{:?}",
            completed.error_message
        );
        assert_eq!(completed.artifacts.as_ref().unwrap()["tool_calls"], 3);
        let directory = observed.lock().unwrap().clone().unwrap();
        assert!(!directory.exists(), "finished checkout leaked");
        let early: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM task_run_logs l JOIN task_runs r ON r.id=l.task_run_id WHERE r.id=$1 AND l.metadata ? 'action_receipt' AND l.created_at <= r.completed_at)").bind(run.id).fetch_one(&pool).await.unwrap();
        assert!(early, "receipt must be durable before terminal state");
        let receipts: Vec<Value> = sqlx::query_scalar("SELECT metadata->'action_receipt' FROM task_run_logs WHERE task_run_id=$1 AND metadata ? 'action_receipt'").bind(run.id).fetch_all(&pool).await.unwrap();
        assert_eq!(
            receipts.len(),
            1,
            "task loop returned without its durable action receipt"
        );
        assert_eq!(receipts[0]["actor_id"], user.id.to_string());
        assert_eq!(receipts[0]["action"], "create_task");
        assert_eq!(receipts[0]["success"], true);
        let target = Uuid::parse_str(receipts[0]["target_id"].as_str().unwrap()).unwrap();
        let written = tasks::get_task(&pool, target).await.unwrap().unwrap();
        assert_eq!(written.workspace_id, workspace.id);
        assert_eq!(written.title, "Made by the scoped task");
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id=$1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
