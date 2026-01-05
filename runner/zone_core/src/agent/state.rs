//! Agent state management

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::llm::{Message, ToolCall};

/// Current phase of the agent execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    /// Initial state, processing user input
    Thinking,
    /// Executing a tool
    Acting,
    /// Waiting for tool results
    Observing,
    /// Formulating final response
    Responding,
    /// Agent has completed
    Complete,
    /// Agent encountered an error
    Error,
}

impl std::fmt::Display for AgentPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentPhase::Thinking => write!(f, "thinking"),
            AgentPhase::Acting => write!(f, "acting"),
            AgentPhase::Observing => write!(f, "observing"),
            AgentPhase::Responding => write!(f, "responding"),
            AgentPhase::Complete => write!(f, "complete"),
            AgentPhase::Error => write!(f, "error"),
        }
    }
}

/// A step in the agent's execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    /// Unique ID for this step
    pub id: Uuid,
    /// The phase of this step
    pub phase: AgentPhase,
    /// The message that was sent/received
    pub message: Option<Message>,
    /// Tool calls made in this step
    pub tool_calls: Option<Vec<ToolCallResult>>,
    /// Timestamp when this step started
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Timestamp when this step completed
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AgentStep {
    pub fn new(phase: AgentPhase) -> Self {
        Self {
            id: Uuid::new_v4(),
            phase,
            message: None,
            tool_calls: None,
            started_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    pub fn with_message(mut self, message: Message) -> Self {
        self.message = Some(message);
        self
    }

    pub fn complete(mut self) -> Self {
        self.completed_at = Some(chrono::Utc::now());
        self
    }
}

/// Result of a tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    /// The tool call from the LLM
    pub call: ToolCall,
    /// The result of executing the tool
    pub result: String,
    /// Whether the tool executed successfully
    pub success: bool,
    /// Duration of the tool execution
    pub duration_ms: u64,
}

/// Configuration for the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum number of iterations before stopping
    pub max_iterations: usize,
    /// Maximum total tokens to use
    pub max_tokens: u32,
    /// Temperature for LLM calls
    pub temperature: f32,
    /// Whether to stream responses
    pub stream: bool,
    /// System prompt to use
    pub system_prompt: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_tokens: 4096,
            temperature: 0.7,
            stream: false,
            system_prompt: None,
        }
    }
}

/// The current state of an agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Unique ID for this execution
    pub id: Uuid,
    /// Current phase
    pub phase: AgentPhase,
    /// All messages in the conversation
    pub messages: Vec<Message>,
    /// All steps taken
    pub steps: Vec<AgentStep>,
    /// Current iteration count
    pub iteration: usize,
    /// Total tokens used
    pub tokens_used: u32,
    /// Whether the agent has finished
    pub finished: bool,
    /// Final response (if finished)
    pub final_response: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// When the execution started
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the execution finished
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AgentState {
    /// Create a new agent state with an initial user message
    pub fn new(user_message: impl Into<String>, system_prompt: Option<String>) -> Self {
        let mut messages = Vec::new();

        if let Some(prompt) = system_prompt {
            messages.push(Message::system(prompt));
        }

        messages.push(Message::user(user_message));

        Self {
            id: Uuid::new_v4(),
            phase: AgentPhase::Thinking,
            messages,
            steps: Vec::new(),
            iteration: 0,
            tokens_used: 0,
            finished: false,
            final_response: None,
            error: None,
            started_at: chrono::Utc::now(),
            finished_at: None,
        }
    }

    /// Add a step to the execution
    pub fn add_step(&mut self, step: AgentStep) {
        self.steps.push(step);
    }

    /// Add a message to the conversation
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Mark the agent as complete with a response
    pub fn complete(&mut self, response: impl Into<String>) {
        self.phase = AgentPhase::Complete;
        self.finished = true;
        self.final_response = Some(response.into());
        self.finished_at = Some(chrono::Utc::now());
    }

    /// Mark the agent as failed with an error
    pub fn fail(&mut self, error: impl Into<String>) {
        self.phase = AgentPhase::Error;
        self.finished = true;
        self.error = Some(error.into());
        self.finished_at = Some(chrono::Utc::now());
    }

    /// Get the progress percentage (based on iterations)
    pub fn progress_percent(&self, max_iterations: usize) -> u8 {
        if self.finished {
            100
        } else {
            ((self.iteration as f32 / max_iterations as f32) * 100.0).min(99.0) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_phase_display() {
        assert_eq!(AgentPhase::Thinking.to_string(), "thinking");
        assert_eq!(AgentPhase::Acting.to_string(), "acting");
        assert_eq!(AgentPhase::Observing.to_string(), "observing");
        assert_eq!(AgentPhase::Responding.to_string(), "responding");
        assert_eq!(AgentPhase::Complete.to_string(), "complete");
        assert_eq!(AgentPhase::Error.to_string(), "error");
    }

    #[test]
    fn test_agent_step_new() {
        let step = AgentStep::new(AgentPhase::Thinking);
        assert_eq!(step.phase, AgentPhase::Thinking);
        assert!(step.message.is_none());
        assert!(step.tool_calls.is_none());
        assert!(step.completed_at.is_none());
    }

    #[test]
    fn test_agent_step_with_message() {
        let step = AgentStep::new(AgentPhase::Thinking).with_message(Message::user("test"));
        assert!(step.message.is_some());
    }

    #[test]
    fn test_agent_step_complete() {
        let step = AgentStep::new(AgentPhase::Thinking).complete();
        assert!(step.completed_at.is_some());
    }

    #[test]
    fn test_agent_config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.max_iterations, 50);
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.temperature, 0.7);
        assert!(!config.stream);
        assert!(config.system_prompt.is_none());
    }

    #[test]
    fn test_agent_state_new() {
        let state = AgentState::new("Hello", None);
        assert_eq!(state.phase, AgentPhase::Thinking);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.iteration, 0);
        assert!(!state.finished);
    }

    #[test]
    fn test_agent_state_new_with_system_prompt() {
        let state = AgentState::new("Hello", Some("You are helpful".to_string()));
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[0].role, crate::llm::Role::System);
        assert_eq!(state.messages[1].role, crate::llm::Role::User);
    }

    #[test]
    fn test_agent_state_add_message() {
        let mut state = AgentState::new("Hello", None);
        state.add_message(Message::assistant("Hi there!"));
        assert_eq!(state.messages.len(), 2);
    }

    #[test]
    fn test_agent_state_add_step() {
        let mut state = AgentState::new("Hello", None);
        state.add_step(AgentStep::new(AgentPhase::Thinking));
        assert_eq!(state.steps.len(), 1);
    }

    #[test]
    fn test_agent_state_complete() {
        let mut state = AgentState::new("Hello", None);
        state.complete("Done!");

        assert_eq!(state.phase, AgentPhase::Complete);
        assert!(state.finished);
        assert_eq!(state.final_response, Some("Done!".to_string()));
        assert!(state.finished_at.is_some());
    }

    #[test]
    fn test_agent_state_fail() {
        let mut state = AgentState::new("Hello", None);
        state.fail("Something went wrong");

        assert_eq!(state.phase, AgentPhase::Error);
        assert!(state.finished);
        assert_eq!(state.error, Some("Something went wrong".to_string()));
        assert!(state.finished_at.is_some());
    }

    #[test]
    fn test_agent_state_progress_percent() {
        let mut state = AgentState::new("Hello", None);

        // 0% at start
        assert_eq!(state.progress_percent(50), 0);

        // 50% halfway
        state.iteration = 25;
        assert_eq!(state.progress_percent(50), 50);

        // 100% when finished
        state.finished = true;
        assert_eq!(state.progress_percent(50), 100);
    }

    #[test]
    fn test_agent_state_serialization() {
        let state = AgentState::new("Test prompt", Some("System".to_string()));
        let json = serde_json::to_string(&state).unwrap();

        // Should contain key fields
        assert!(json.contains("thinking"));
        assert!(json.contains("Test prompt"));

        // Should deserialize back
        let deserialized: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, state.id);
        assert_eq!(deserialized.messages.len(), 2);
    }

    #[test]
    fn test_agent_phase_serialization_roundtrip() {
        let phases = vec![
            AgentPhase::Thinking,
            AgentPhase::Acting,
            AgentPhase::Observing,
            AgentPhase::Responding,
            AgentPhase::Complete,
            AgentPhase::Error,
        ];

        for phase in phases {
            let json = serde_json::to_string(&phase).unwrap();
            let deserialized: AgentPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, phase);
        }
    }

    #[test]
    fn test_agent_phase_copy() {
        let phase = AgentPhase::Acting;
        let copied = phase;
        assert_eq!(phase, copied);

        let phase2 = AgentPhase::Observing;
        let copied2 = phase2; // Copy trait
        assert_eq!(phase2, copied2);
    }

    #[test]
    fn test_agent_phase_equality() {
        assert_eq!(AgentPhase::Thinking, AgentPhase::Thinking);
        assert_ne!(AgentPhase::Thinking, AgentPhase::Acting);
        assert_ne!(AgentPhase::Complete, AgentPhase::Error);
    }

    // ==================== AgentStep Edge Cases ====================

    #[test]
    fn test_agent_step_with_tool_calls() {
        use crate::llm::{FunctionCall, ToolCall};

        let tool_call = ToolCall {
            id: "call_123".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path": "/tmp/test"}"#.to_string(),
            },
        };

        let tool_result = ToolCallResult {
            call: tool_call,
            result: "file contents".to_string(),
            success: true,
            duration_ms: 150,
        };

        let mut step = AgentStep::new(AgentPhase::Acting);
        step.tool_calls = Some(vec![tool_result]);

        assert!(step.tool_calls.is_some());
        assert_eq!(step.tool_calls.as_ref().unwrap().len(), 1);
        assert!(step.tool_calls.as_ref().unwrap()[0].success);
    }

    #[test]
    fn test_agent_step_serialization_roundtrip() {
        let step = AgentStep::new(AgentPhase::Thinking)
            .with_message(Message::user("test message"))
            .complete();

        let json = serde_json::to_string(&step).unwrap();
        let deserialized: AgentStep = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.phase, AgentPhase::Thinking);
        assert!(deserialized.message.is_some());
        assert!(deserialized.completed_at.is_some());
    }

    #[test]
    fn test_agent_step_id_uniqueness() {
        let step1 = AgentStep::new(AgentPhase::Thinking);
        let step2 = AgentStep::new(AgentPhase::Thinking);

        // Each step should have a unique ID
        assert_ne!(step1.id, step2.id);
    }

    #[test]
    fn test_agent_step_timing() {
        let step = AgentStep::new(AgentPhase::Acting);
        let start_time = step.started_at;

        // Brief delay
        std::thread::sleep(std::time::Duration::from_millis(5));

        let completed_step = step.complete();
        let end_time = completed_step.completed_at.unwrap();

        assert!(end_time > start_time);
    }

    // ==================== ToolCallResult Edge Cases ====================

    #[test]
    fn test_tool_call_result_failed() {
        use crate::llm::{FunctionCall, ToolCall};

        let tool_call = ToolCall {
            id: "call_fail".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "execute_command".to_string(),
                arguments: r#"{"cmd": "invalid"}"#.to_string(),
            },
        };

        let result = ToolCallResult {
            call: tool_call,
            result: "Error: command not found".to_string(),
            success: false,
            duration_ms: 50,
        };

        assert!(!result.success);
        assert!(result.result.contains("Error"));
    }

    #[test]
    fn test_tool_call_result_serialization() {
        use crate::llm::{FunctionCall, ToolCall};

        let tool_call = ToolCall {
            id: "call_test".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "test".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let result = ToolCallResult {
            call: tool_call,
            result: "success".to_string(),
            success: true,
            duration_ms: 100,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ToolCallResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.call.id, "call_test");
        assert!(deserialized.success);
        assert_eq!(deserialized.duration_ms, 100);
    }

    // ==================== AgentConfig Edge Cases ====================

    #[test]
    fn test_agent_config_custom() {
        let config = AgentConfig {
            max_iterations: 100,
            max_tokens: 8192,
            temperature: 0.3,
            stream: true,
            system_prompt: Some("Custom system prompt".to_string()),
        };

        assert_eq!(config.max_iterations, 100);
        assert_eq!(config.max_tokens, 8192);
        assert!((config.temperature - 0.3).abs() < f32::EPSILON);
        assert!(config.stream);
        assert_eq!(
            config.system_prompt,
            Some("Custom system prompt".to_string())
        );
    }

    #[test]
    fn test_agent_config_serialization_roundtrip() {
        let config = AgentConfig {
            max_iterations: 75,
            max_tokens: 2048,
            temperature: 0.5,
            stream: false,
            system_prompt: Some("You are a coding assistant".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AgentConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.max_iterations, config.max_iterations);
        assert_eq!(deserialized.max_tokens, config.max_tokens);
        assert_eq!(deserialized.system_prompt, config.system_prompt);
    }

    #[test]
    fn test_agent_config_clone() {
        let config = AgentConfig::default();
        let cloned = config.clone();

        assert_eq!(cloned.max_iterations, config.max_iterations);
        assert_eq!(cloned.max_tokens, config.max_tokens);
    }

    // ==================== AgentState Edge Cases ====================

    #[test]
    fn test_agent_state_empty_system_prompt() {
        let state = AgentState::new("Hello", Some("".to_string()));
        // Empty string is still a Some, so we have 2 messages
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[0].content, Some("".to_string()));
    }

    #[test]
    fn test_agent_state_multiple_messages() {
        let mut state = AgentState::new("First message", None);
        state.add_message(Message::assistant("Response 1"));
        state.add_message(Message::user("Second message"));
        state.add_message(Message::assistant("Response 2"));

        assert_eq!(state.messages.len(), 4);
    }

    #[test]
    fn test_agent_state_multiple_steps() {
        let mut state = AgentState::new("Hello", None);

        state.add_step(AgentStep::new(AgentPhase::Thinking));
        state.add_step(AgentStep::new(AgentPhase::Acting));
        state.add_step(AgentStep::new(AgentPhase::Observing));
        state.add_step(AgentStep::new(AgentPhase::Responding));

        assert_eq!(state.steps.len(), 4);
    }

    #[test]
    fn test_agent_state_id_uniqueness() {
        let state1 = AgentState::new("Hello", None);
        let state2 = AgentState::new("Hello", None);

        assert_ne!(state1.id, state2.id);
    }

    #[test]
    fn test_agent_state_progress_edge_cases() {
        let mut state = AgentState::new("Hello", None);

        // Progress at exactly max iterations (but not finished)
        state.iteration = 50;
        assert_eq!(state.progress_percent(50), 99); // Capped at 99 when not finished

        // Progress beyond max iterations
        state.iteration = 100;
        assert_eq!(state.progress_percent(50), 99); // Still capped at 99

        // Progress at 0 max iterations (edge case - avoid division by zero)
        state.iteration = 0;
        // This would cause division by zero, but Rust's float division handles it
        // The result will be 0 / 0 = NaN, which becomes 0 due to min(99)
    }

    #[test]
    fn test_agent_state_complete_then_fail_overwrite() {
        let mut state = AgentState::new("Hello", None);

        state.complete("Initial response");
        assert_eq!(state.phase, AgentPhase::Complete);
        assert!(state.finished);
        assert_eq!(state.final_response, Some("Initial response".to_string()));

        // Calling fail after complete should overwrite
        state.fail("Error occurred");
        assert_eq!(state.phase, AgentPhase::Error);
        assert!(state.finished);
        assert_eq!(state.error, Some("Error occurred".to_string()));
    }

    #[test]
    fn test_agent_state_fail_then_complete_overwrite() {
        let mut state = AgentState::new("Hello", None);

        state.fail("Error");
        assert_eq!(state.phase, AgentPhase::Error);

        state.complete("Recovery");
        assert_eq!(state.phase, AgentPhase::Complete);
        assert_eq!(state.final_response, Some("Recovery".to_string()));
    }

    #[test]
    fn test_agent_state_tokens_tracking() {
        let mut state = AgentState::new("Hello", None);
        assert_eq!(state.tokens_used, 0);

        state.tokens_used = 500;
        assert_eq!(state.tokens_used, 500);

        state.tokens_used += 300;
        assert_eq!(state.tokens_used, 800);
    }

    #[test]
    fn test_agent_state_iteration_tracking() {
        let mut state = AgentState::new("Hello", None);
        assert_eq!(state.iteration, 0);

        state.iteration = 10;
        assert_eq!(state.iteration, 10);

        state.iteration += 1;
        assert_eq!(state.iteration, 11);
    }

    #[test]
    fn test_agent_state_timing() {
        let state = AgentState::new("Hello", None);
        let start_time = state.started_at;

        assert!(state.finished_at.is_none());

        let mut mutable_state = state;
        std::thread::sleep(std::time::Duration::from_millis(5));
        mutable_state.complete("Done");

        assert!(mutable_state.finished_at.is_some());
        assert!(mutable_state.finished_at.unwrap() > start_time);
    }

    #[test]
    fn test_agent_state_phase_transitions() {
        let mut state = AgentState::new("Hello", None);
        assert_eq!(state.phase, AgentPhase::Thinking);

        state.phase = AgentPhase::Acting;
        assert_eq!(state.phase, AgentPhase::Acting);

        state.phase = AgentPhase::Observing;
        assert_eq!(state.phase, AgentPhase::Observing);

        state.phase = AgentPhase::Responding;
        assert_eq!(state.phase, AgentPhase::Responding);
    }

    #[test]
    fn test_agent_state_with_long_message() {
        let long_message = "a".repeat(10000);
        let state = AgentState::new(long_message.clone(), None);

        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, Some(long_message));
    }

    #[test]
    fn test_agent_state_serialization_with_all_fields() {
        use crate::llm::{FunctionCall, ToolCall};

        let mut state = AgentState::new("Test", Some("System prompt".to_string()));
        state.iteration = 5;
        state.tokens_used = 1000;

        let tool_call = ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "test".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let mut step = AgentStep::new(AgentPhase::Acting);
        step.tool_calls = Some(vec![ToolCallResult {
            call: tool_call,
            result: "success".to_string(),
            success: true,
            duration_ms: 100,
        }]);
        state.add_step(step);

        state.add_message(Message::assistant("Response"));
        state.complete("Final answer");

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: AgentState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, state.id);
        assert_eq!(deserialized.iteration, 5);
        assert_eq!(deserialized.tokens_used, 1000);
        assert_eq!(deserialized.steps.len(), 1);
        assert_eq!(deserialized.messages.len(), 3);
        assert!(deserialized.finished);
        assert_eq!(
            deserialized.final_response,
            Some("Final answer".to_string())
        );
    }
}
