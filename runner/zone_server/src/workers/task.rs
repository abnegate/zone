//! Task execution worker
//!
//! Executes agentic tasks in the background using the zone_core agent loop,
//! persisting logs and progress to the database for monitoring.

use sqlx::PgPool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use uuid::Uuid;
use zone_core::agent::{Agent, AgentCallback, AgentConfig, AgentPhase};
use zone_core::llm::{LlmClient, LlmConfig};
use zone_core::tools::{ToolContext, ToolRegistry, ToolResult};

use crate::db::tasks;
use crate::state::AppState;
use crate::workers::pr::{PrCreationResult, create_pr_for_task};

// Max concurrent task executions
const MAX_CONCURRENT_TASKS: usize = 5;

// Timeout for task execution (1 hour)
const TASK_TIMEOUT_SECS: u64 = 3600;

// Tool configuration constants
const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10MB
const COMMAND_TIMEOUT_SECS: u64 = 300; // 5 minutes

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

// Safe environment variables to pass to agent tools
// Only include variables needed for basic command execution
const SAFE_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "TERM",
    "SHELL",
    "TZ",
    "TMPDIR",
    "XDG_RUNTIME_DIR",
];

// Global semaphore to limit concurrent task executions
static TASK_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_semaphore() -> &'static Arc<Semaphore> {
    TASK_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS)))
}

/// Get safe environment variables for agent tool execution
///
/// Returns only allowlisted environment variables to prevent
/// leaking sensitive data like API keys or database credentials.
fn get_safe_env() -> HashMap<String, String> {
    SAFE_ENV_VARS
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|val| (key.to_string(), val)))
        .collect()
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
            // Update task run progress
            if let Err(e) =
                tasks::update_task_run_progress(&pool, run_id, Some(&phase_str), None).await
            {
                tracing::error!("Failed to update task run progress: {}", e);
            }

            // Log phase change
            let log_message =
                message_str.unwrap_or_else(|| format!("Entering {} phase", phase_str));

            if let Err(e) = tasks::add_task_run_log(
                &pool,
                run_id,
                &phase_str,
                "agent",
                "info",
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
                "acting",
                "tool",
                "info",
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
                ("info", format!("Tool {} succeeded", tool_name))
            } else {
                (
                    "error",
                    format!("Tool {} failed: {:?}", tool_name, result.error),
                )
            };

            if let Err(e) = tasks::add_task_run_log(
                &pool,
                run_id,
                "acting",
                "tool",
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
                "responding",
                "agent",
                "info",
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
/// 1. Acquires semaphore permit to limit concurrent executions
/// 2. Updates run status to "running"
/// 3. Fetches task details from database
/// 4. Gathers context if source_ids are specified
/// 5. Initializes LLM client and agent
/// 6. Executes agent loop with DatabaseTaskCallback
/// 7. Updates status to "completed" or "failed"
///
/// All events are persisted to the database via DatabaseTaskCallback for monitoring.
pub async fn execute_task_run(state: &AppState, run_id: Uuid, task_id: Uuid) {
    // Acquire semaphore permit to limit concurrent executions
    let _permit = match get_semaphore().acquire().await {
        Ok(p) => p,
        Err(_) => {
            tracing::error!("Task semaphore closed for run {}", run_id);
            if let Err(e) = tasks::complete_task_run(
                state.db(),
                run_id,
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

    tracing::info!(
        "Starting task execution: run_id={}, task_id={}",
        run_id,
        task_id
    );

    // Fetch task details
    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracing::error!("Task {} not found", task_id);
            if let Err(e) =
                tasks::complete_task_run(state.db(), run_id, "failed", Some("Task not found"), None)
                    .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
            return;
        }
        Err(e) => {
            tracing::error!("Failed to fetch task {}: {}", task_id, e);
            if let Err(e) = tasks::complete_task_run(
                state.db(),
                run_id,
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

    // Build system prompt with context if available
    let mut system_prompt = String::from(
        "You are an AI coding assistant. You help users with software development tasks.\n\n",
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

    // Determine workspace path
    let workspace_path = if let Some(_github_url) = &task.github_repo_url {
        // For GitHub repos, we'd clone to a temp directory
        // For now, just use a temp directory
        std::env::temp_dir().join(format!("zone-task-{}", task_id))
    } else {
        // Use current directory or configured workspace
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"))
    };

    // Create tool context with safe environment
    // SECURITY: Only allowlisted env vars are passed to prevent credential leakage
    let tool_context = ToolContext {
        cwd: workspace_path.clone(),
        env: get_safe_env(),
        max_file_size: MAX_FILE_SIZE,
        command_timeout: COMMAND_TIMEOUT_SECS,
    };

    // Create tool registry with defaults
    let tools = ToolRegistry::with_defaults();

    // Get LLM configuration (from task, then environment, then defaults)
    let temperature = default_temperature();
    let max_tokens = default_max_tokens();
    let model = task.model_name.clone().unwrap_or_else(default_model);

    // Configure LLM client
    let llm_config = LlmConfig {
        base_url: state.config().litellm_host.clone(),
        api_key: state.config().litellm_key.clone(),
        default_model: model,
        temperature,
        max_tokens,
    };

    let llm = LlmClient::new(llm_config);

    // Configure agent
    let agent_config = AgentConfig {
        max_iterations: 50,
        max_tokens,
        temperature,
        stream: false,
        system_prompt: Some(system_prompt),
    };

    // Create agent
    let agent = Agent::new(llm, tools, agent_config, tool_context);

    // Create callback
    let callback = DatabaseTaskCallback::new(state.db().clone(), run_id);

    // Build task prompt
    let prompt = format!("# Task: {}\n\n{}", task.title, task.description);

    // Execute agent with timeout
    let agent_future = agent.run(prompt, &callback);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(TASK_TIMEOUT_SECS),
        agent_future,
    )
    .await;

    match result {
        Ok(Ok(agent_state)) => {
            // Agent completed successfully
            let summary = agent_state
                .final_response
                .clone()
                .unwrap_or_else(|| "Task completed".to_string());

            tracing::info!(
                "Task run {} completed: iterations={}, tokens={}",
                run_id,
                agent_state.iteration,
                agent_state.tokens_used
            );

            // Attempt PR creation if there are code changes
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
                "iterations": agent_state.iteration,
                "tokens_used": agent_state.tokens_used,
                "summary": summary,
            });

            if let Some(pr) = pr_info {
                artifacts["pr"] = pr;
            }

            if let Err(e) =
                tasks::complete_task_run(state.db(), run_id, "completed", None, Some(artifacts))
                    .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
        }
        Ok(Err(e)) => {
            // Agent failed with error
            tracing::error!("Task run {} failed: {}", run_id, e);
            if let Err(e) =
                tasks::complete_task_run(state.db(), run_id, "failed", Some(&e.to_string()), None)
                    .await
            {
                tracing::error!("CRITICAL: Failed to update run {} status: {}", run_id, e);
            }
        }
        Err(_) => {
            // Task timed out
            tracing::error!(
                "Task run {} timed out after {} seconds",
                run_id,
                TASK_TIMEOUT_SECS
            );
            if let Err(e) = tasks::complete_task_run(
                state.db(),
                run_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semaphore_initialization() {
        let sem = get_semaphore();
        // Should have MAX_CONCURRENT_TASKS permits available initially
        assert_eq!(sem.available_permits(), MAX_CONCURRENT_TASKS);
    }

    #[tokio::test]
    async fn test_database_task_callback_creation() {
        let pool = PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let run_id = Uuid::new_v4();
        let callback = DatabaseTaskCallback::new(pool, run_id);
        assert_eq!(callback.run_id, run_id);
    }

    // Integration tests are in zone_server/tests/task_execution_tests.rs
}
