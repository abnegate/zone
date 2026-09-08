//! What one line of a coding agent's output means.

use crate::llm::{ToolCall, Usage};

/// A normalised event, whichever agent produced it.
///
/// There is deliberately no throttling variant. The task worker already owns
/// the vocabulary that separates a throttled run from a rejected one, and it
/// reads that vocabulary out of the failure text. A second classifier here
/// would be a second place to keep in step with it, so a throttled agent
/// simply becomes a [`AgentEvent::Failed`] carrying the agent's own wording.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Text(String),
    Tool(ToolCall),
    Usage(Usage),
    Failed(String),
    Finished { finish_reason: Option<String> },
}

impl AgentEvent {
    /// Whether this event ends the run, successfully or not.
    ///
    /// An agent that exits zero without emitting one of these never finished
    /// its turn, and treating that as success would hand the caller a silently
    /// truncated answer.
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Failed(_) | Self::Finished { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_result_ends_the_run() {
        assert!(!AgentEvent::Text("hello".to_string()).terminal());
        assert!(
            !AgentEvent::Usage(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
            })
            .terminal()
        );
        assert!(AgentEvent::Failed("nope".to_string()).terminal());
        assert!(
            AgentEvent::Finished {
                finish_reason: None
            }
            .terminal()
        );
    }
}
