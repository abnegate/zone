//! The subagents a coding agent starts for itself.
//!
//! Claude Code's Agent tool and codex's collaborators both run work the reader
//! would otherwise never see. Each line of an agent's output can say something
//! new about one of them, so an update carries only what that line said.

mod status;
mod step;
mod step_status;

pub use status::SubagentStatus;
pub use step::SubagentStep;
pub use step_status::StepStatus;

/// What one line of an agent's output said about one of its own subagents.
/// Only `id` is always set; any other field left `None` did not change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subagent {
    /// Claude: the id of the Agent/Task call that started it. Codex: its thread id.
    pub id: String,
    pub description: Option<String>,
    /// Claude's `subagent_type`.
    pub kind: Option<String>,
    pub model: Option<String>,
    pub background: Option<bool>,
    pub status: Option<SubagentStatus>,
    pub tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
    pub tokens: Option<u64>,
    pub last_tool: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub step: Option<SubagentStep>,
}

impl Subagent {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_update_names_its_subagent_and_changes_nothing_else() {
        let subagent = Subagent::new("toolu_01Agent");

        assert_eq!(
            subagent,
            Subagent {
                id: "toolu_01Agent".to_string(),
                ..Subagent::default()
            }
        );
    }
}
