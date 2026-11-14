use abnegate_agent_cli::SubagentStep;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Steps {
    pub steps: Vec<SubagentStep>,
}
