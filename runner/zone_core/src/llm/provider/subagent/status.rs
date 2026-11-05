use serde::{Deserialize, Serialize};

/// Where a subagent's run stands. Each agent's own vocabulary is mapped onto
/// these four, so a reader sees one set whichever agent started it. `Stopped`
/// is a run that ended without finishing: it was stopped, or its turn ended
/// first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
    Stopped,
}

impl SubagentStatus {
    pub const ALL: [Self; 4] = [Self::Running, Self::Completed, Self::Failed, Self::Stopped];

    /// The name it is stored and sent under.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == name)
    }

    pub fn finished(self) -> bool {
        self != Self::Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_is_named_in_snake_case_both_ways() {
        for status in SubagentStatus::ALL {
            let named = serde_json::to_value(status).expect("a status to serialise");
            assert_eq!(named, serde_json::json!(status.as_str()));
            assert_eq!(
                serde_json::from_value::<SubagentStatus>(named).ok(),
                Some(status)
            );
            assert_eq!(SubagentStatus::named(status.as_str()), Some(status));
        }
        assert_eq!(SubagentStatus::named("Running"), None);
        assert_eq!(SubagentStatus::named("errored"), None);
    }

    #[test]
    fn only_a_running_subagent_is_unfinished() {
        for status in SubagentStatus::ALL {
            assert_eq!(
                status.finished(),
                status != SubagentStatus::Running,
                "{status:?}"
            );
        }
    }
}
