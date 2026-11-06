use serde::Serialize;
use zone_core::llm::SubagentStep;

#[derive(Debug, Serialize)]
pub struct Steps {
    pub steps: Vec<SubagentStep>,
}
