//! ReAct agent loop implementation

use futures::future::join_all;
use std::collections::HashSet;
use std::time::Instant;
use thiserror::Error;

use crate::context::{self, ContextError, ContextSource, Entry, Policy};
use crate::llm::{LlmClient, LlmError, Message, RequestOptions, Role, ToolCall};
use crate::tools::{ToolContext, ToolRegistry, ToolResult};

use super::state::{AgentConfig, AgentPhase, AgentState, AgentStep, ToolCallResult};

/// Agent execution error
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),
    #[error("Context error: {0}")]
    Context(#[from] ContextError),
    #[error("Tool error: {0}")]
    Tool(String),
    #[error("Max iterations exceeded")]
    MaxIterations,
    #[error("Agent was cancelled")]
    Cancelled,
}

/// Callback for agent progress updates
pub trait AgentCallback: Send + Sync {
    /// Called when the agent phase changes
    fn on_phase_change(&self, phase: AgentPhase, message: Option<&str>);

    /// Called when a tool is executed
    fn on_tool_call(&self, tool_name: &str, args: &str);

    /// Called when a tool returns
    fn on_tool_result(&self, tool_name: &str, result: &ToolResult);

    /// Called when the agent produces a response
    fn on_response(&self, response: &str);
}

/// No-op callback for when progress tracking isn't needed
pub struct NoOpCallback;

impl AgentCallback for NoOpCallback {
    fn on_phase_change(&self, _phase: AgentPhase, _message: Option<&str>) {}
    fn on_tool_call(&self, _tool_name: &str, _args: &str) {}
    fn on_tool_result(&self, _tool_name: &str, _result: &ToolResult) {}
    fn on_response(&self, _response: &str) {}
}

/// The ReAct agent
pub struct Agent {
    llm: LlmClient,
    tools: ToolRegistry,
    config: AgentConfig,
    context: ToolContext,
    policy: Policy,
}

impl Agent {
    /// Create a new agent
    pub fn new(
        llm: LlmClient,
        tools: ToolRegistry,
        config: AgentConfig,
        context: ToolContext,
    ) -> Self {
        Self {
            llm,
            tools,
            policy: Policy {
                limit: None,
                reserved: config.max_tokens,
                source: ContextSource::Unknown,
            },
            config,
            context,
        }
    }

    /// Use a verified/configured effective capacity without guessing from the model name.
    pub fn with_context_policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    /// Run the agent with a user prompt
    pub async fn run(
        &self,
        prompt: impl Into<String>,
        callback: &dyn AgentCallback,
    ) -> Result<AgentState, AgentError> {
        let system_prompt = self
            .config
            .system_prompt
            .clone()
            .unwrap_or_else(|| default_system_prompt(&self.tools));

        let mut state = AgentState::new(prompt, Some(system_prompt));

        self.run_loop(&mut state, callback).await?;

        Ok(state)
    }

    /// Continue an existing agent state
    pub async fn continue_run(
        &self,
        state: &mut AgentState,
        user_message: impl Into<String>,
        callback: &dyn AgentCallback,
    ) -> Result<(), AgentError> {
        state.add_message(Message::user(user_message));
        state.phase = AgentPhase::Thinking;
        state.iteration = 0;
        state.finished = false;
        state.final_response = None;
        state.error = None;

        self.run_loop(state, callback).await
    }

    /// Main agent loop
    async fn run_loop(
        &self,
        state: &mut AgentState,
        callback: &dyn AgentCallback,
    ) -> Result<(), AgentError> {
        let tool_definitions = self.tools.definitions();

        loop {
            // Check iteration limit
            if state.iteration >= self.config.max_iterations {
                state.fail("Maximum iterations exceeded");
                return Err(AgentError::MaxIterations);
            }

            state.iteration += 1;

            // Thinking phase - call the LLM
            state.phase = AgentPhase::Thinking;
            callback.on_phase_change(AgentPhase::Thinking, None);

            let mut step = AgentStep::new(AgentPhase::Thinking);

            let latest = state
                .messages
                .iter()
                .rposition(|message| message.role == Role::User);
            let entries: Vec<_> = state
                .messages
                .iter()
                .enumerate()
                .map(|(index, message)| Entry {
                    id: format!("{}:{index}", state.id),
                    message: message.clone(),
                    preserve: message.role == Role::System || Some(index) == latest,
                    consumed: index < state.consumed,
                })
                .collect();
            let prepared = context::prepare(
                &self.llm,
                &self.llm.config().default_model,
                &entries,
                Some(&tool_definitions),
                &self.policy,
                state.summary.as_ref(),
            )
            .await?;
            let response = self
                .llm
                .chat_with_options(
                    &self.llm.config().default_model,
                    &prepared.messages,
                    Some(&tool_definitions),
                    RequestOptions {
                        reserved: self.policy.reserved,
                    },
                )
                .await?;
            state.summary = prepared.summary;
            state.consumed = state.messages.len();

            // Update token usage
            if let Some(usage) = &response.usage {
                state.tokens_used = state.tokens_used.saturating_add(usage.total_tokens);
            }

            let choice = response
                .choices
                .first()
                .ok_or_else(|| AgentError::Tool("No response from LLM".to_string()))?;

            let mut message = choice.message.clone();
            let mut identifiers: HashSet<_> = state
                .messages
                .iter()
                .filter_map(|message| message.tool_calls.as_ref())
                .flatten()
                .map(|call| call.id.clone())
                .collect();
            if let Some(calls) = message.tool_calls.as_mut() {
                for call in calls {
                    if call.id.is_empty() || !identifiers.insert(call.id.clone()) {
                        loop {
                            let id = format!("zone_{}", uuid::Uuid::new_v4().simple());
                            if identifiers.insert(id.clone()) {
                                call.id = id;
                                break;
                            }
                        }
                    }
                }
            }

            // Check if the model wants to use tools
            if let Some(tool_calls) = &message.tool_calls
                && !tool_calls.is_empty()
            {
                // Acting phase - execute tools
                state.phase = AgentPhase::Acting;
                callback.on_phase_change(AgentPhase::Acting, None);

                // Add assistant message with tool calls
                state.add_message(message.clone());

                let mut tool_results = Vec::with_capacity(tool_calls.len());
                let mut rest = tool_calls.as_slice();
                while !rest.is_empty() {
                    let serial_at = rest
                        .iter()
                        .position(|call| self.tools.mutating(&call.function.name));
                    let parallel_end = serial_at.unwrap_or(rest.len());
                    if parallel_end > 0 {
                        let (parallel, tail) = rest.split_at(parallel_end);
                        for tool_call in parallel {
                            callback.on_tool_call(
                                &tool_call.function.name,
                                &tool_call.function.arguments,
                            );
                        }
                        if parallel.len() == 1 {
                            let start = Instant::now();
                            let result = self.execute_tool(&parallel[0]).await;
                            self.record_tool(
                                state,
                                callback,
                                &mut tool_results,
                                &parallel[0],
                                result,
                                start.elapsed().as_millis() as u64,
                            );
                        } else {
                            let executed = join_all(parallel.iter().map(|tool_call| async move {
                                let start = Instant::now();
                                let result = self.execute_tool(tool_call).await;
                                (tool_call, result, start.elapsed().as_millis() as u64)
                            }))
                            .await;
                            for (tool_call, result, duration_ms) in executed {
                                self.record_tool(
                                    state,
                                    callback,
                                    &mut tool_results,
                                    tool_call,
                                    result,
                                    duration_ms,
                                );
                            }
                        }
                        rest = tail;
                    }
                    if let Some(tool_call) = rest.first() {
                        callback
                            .on_tool_call(&tool_call.function.name, &tool_call.function.arguments);
                        let start = Instant::now();
                        let result = self.execute_tool(tool_call).await;
                        self.record_tool(
                            state,
                            callback,
                            &mut tool_results,
                            tool_call,
                            result,
                            start.elapsed().as_millis() as u64,
                        );
                        rest = &rest[1..];
                    }
                }

                step.tool_calls = Some(tool_results);
                step = step.complete();
                state.add_step(step);

                // Observing phase
                state.phase = AgentPhase::Observing;
                callback.on_phase_change(AgentPhase::Observing, None);

                // Continue the loop to let the LLM process tool results
                continue;
            }

            // No tool calls - check if this is a final response
            if let Some(content) = &message.content {
                state.add_message(message.clone());
                step.message = Some(message.clone());
                step = step.complete();
                state.add_step(step);

                // Check finish reason
                if choice.finish_reason.as_deref() == Some("stop") {
                    state.phase = AgentPhase::Responding;
                    callback.on_phase_change(AgentPhase::Responding, Some(content));
                    callback.on_response(content);

                    state.complete(content);
                    return Ok(());
                }
            }

            // If we get here without content or tool calls, something went wrong
            if message.content.is_none() && message.tool_calls.is_none() {
                state.fail("LLM returned empty response");
                return Err(AgentError::Tool("Empty response from LLM".to_string()));
            }
        }
    }

    fn record_tool(
        &self,
        state: &mut AgentState,
        callback: &dyn AgentCallback,
        tool_results: &mut Vec<ToolCallResult>,
        tool_call: &ToolCall,
        result: ToolResult,
        duration_ms: u64,
    ) {
        callback.on_tool_result(&tool_call.function.name, &result);
        let output = result.to_message();
        tool_results.push(ToolCallResult {
            call: tool_call.clone(),
            result: output.clone(),
            success: result.success,
            duration_ms,
        });
        state.add_message(Message::tool_result(&tool_call.id, output));
    }

    /// Execute a single tool call
    async fn execute_tool(&self, tool_call: &ToolCall) -> ToolResult {
        let params: serde_json::Value = match serde_json::from_str(&tool_call.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult::error(format!("Invalid tool arguments: {}", e));
            }
        };

        match self
            .tools
            .execute(&tool_call.function.name, params, &self.context)
            .await
        {
            Ok(result) => result,
            Err(e) => ToolResult::error(e.to_string()),
        }
    }
}

fn default_system_prompt(tools: &ToolRegistry) -> String {
    let mut names: Vec<&str> = tools.names();
    names.sort_unstable();
    let list = names
        .iter()
        .map(|name| format!("- {name}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut prompt = format!(
        "You are a helpful AI assistant that can read and write code, run commands, and help with software development tasks.\n\n\
You have access to the following tools:\n\
{list}\n\n\
When working on tasks:\n\
1. First understand the current state by reading relevant files\n\
2. Plan your approach before making changes\n\
3. Make changes incrementally and verify each step\n\
4. If a command fails, analyze the error and try a different approach\n\
5. Provide clear explanations of what you're doing\n\n\
Always be careful when modifying files and running commands. If you're unsure about something, explain your uncertainty and ask for clarification."
    );

    if let Some(guidance) = tools.mcp_guidance() {
        prompt.push_str("\n\n");
        prompt.push_str(&guidance);
    }

    prompt
}
