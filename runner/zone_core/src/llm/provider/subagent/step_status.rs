use serde::{Deserialize, Serialize};

/// Where one of a subagent's own tool calls stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Running,
    Succeeded,
    Failed,
}

impl StepStatus {
    pub const ALL: [Self; 3] = [Self::Running, Self::Succeeded, Self::Failed];

    /// The name it is stored and sent under.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_status_is_named_in_snake_case_both_ways() {
        for status in StepStatus::ALL {
            let named = serde_json::to_value(status).expect("a status to serialise");
            assert_eq!(named, serde_json::json!(status.as_str()));
            assert_eq!(
                serde_json::from_value::<StepStatus>(named).ok(),
                Some(status)
            );
            assert_eq!(StepStatus::named(status.as_str()), Some(status));
        }
        assert_eq!(StepStatus::named("Succeeded"), None);
        assert_eq!(StepStatus::named("completed"), None);
    }
}
