use serde::{Deserialize, Serialize};

use super::step_status::StepStatus;

/// One of a subagent's own tool calls, shown as a step of its run rather than
/// as a line of the main agent's reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentStep {
    /// The id of the tool call, which its result is paired with.
    pub id: String,
    pub name: String,
    /// What the call was about, as the main thread's activity line shows it.
    #[serde(default)]
    pub subject: Option<String>,
    pub status: StepStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_without_a_subject_still_reads() {
        let step: SubagentStep =
            serde_json::from_str(r#"{"id":"toolu_x","name":"Bash","status":"running"}"#)
                .expect("a step");

        assert_eq!(
            step,
            SubagentStep {
                id: "toolu_x".to_string(),
                name: "Bash".to_string(),
                subject: None,
                status: StepStatus::Running,
            }
        );
    }

    #[test]
    fn a_step_writes_its_missing_subject_as_null() {
        let step = SubagentStep {
            id: "toolu_x".to_string(),
            name: "Bash".to_string(),
            subject: None,
            status: StepStatus::Succeeded,
        };

        assert_eq!(
            serde_json::to_value(&step).expect("a step to serialise"),
            serde_json::json!({
                "id": "toolu_x",
                "name": "Bash",
                "subject": null,
                "status": "succeeded",
            })
        );
    }
}
