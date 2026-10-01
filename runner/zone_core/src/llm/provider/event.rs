//! What one line of a coding agent's output means.

use super::limit::Limit;
use super::window::Window;
use crate::llm::{ToolCall, Usage};

/// A normalised event, whichever agent produced it.
///
/// A turn a usage limit refused is [`AgentEvent::Limited`] rather than
/// [`AgentEvent::Failed`], so whoever routes turns between sign-ins can tell
/// an exhausted account from a broken turn without reading prose. Its message
/// is still the agent's own wording, word for word as a failure would carry
/// it, because the task worker's retry policy reads that wording.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Text(String),
    Tool(ToolCall),
    Usage(Usage),
    /// How much of a usage window the account has spent, reported while the
    /// turn goes on.
    Window(Window),
    Limited(Limit),
    Failed(String),
    Finished {
        finish_reason: Option<String>,
    },
}

impl AgentEvent {
    /// Whether this event ends the run, successfully or not.
    ///
    /// An agent that exits zero without emitting one of these never finished
    /// its turn, and treating that as success would hand the caller a silently
    /// truncated answer.
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Failed(_) | Self::Limited(_) | Self::Finished { .. }
        )
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
        assert!(
            !AgentEvent::Window(Window {
                name: "five_hour".to_string(),
                used_percent: Some(43.0),
                used: None,
                limit: None,
                resets_at: None,
            })
            .terminal()
        );
        assert!(AgentEvent::Failed("nope".to_string()).terminal());
        assert!(
            AgentEvent::Limited(Limit {
                message: "You've hit your session limit".to_string(),
                resets_at: None,
                credits: false,
                window: None,
            })
            .terminal()
        );
        assert!(
            AgentEvent::Finished {
                finish_reason: None
            }
            .terminal()
        );
    }
}
