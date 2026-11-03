//! What one line of a coding agent's output means.

use abnegate_llm::{Limit, ToolCall, Usage, Window};

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
    /// The id of the session the agent runs this turn under, as it announced
    /// it: the one a later turn resumes.
    Session(String),
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
        assert!(!AgentEvent::Usage(Usage::new(1, 1)).terminal());
        assert!(!AgentEvent::Window(Window::new("five_hour").with_used_percent(43.0)).terminal());
        assert!(!AgentEvent::Session("6f1".to_string()).terminal());
        assert!(AgentEvent::Failed("nope".to_string()).terminal());
        assert!(AgentEvent::Limited(Limit::new("You've hit your session limit")).terminal());
        assert!(
            AgentEvent::Finished {
                finish_reason: None
            }
            .terminal()
        );
    }
}
